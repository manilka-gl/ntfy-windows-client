use std::{
    ffi::c_void,
    mem::{size_of, size_of_val},
    ptr, thread,
    time::{Duration, Instant},
};

pub const DEFAULT_OUTPUT_LABEL: &str = "System default";

const MMSYSERR_NOERROR: u32 = 0;
const WAVE_FORMAT_PCM: u16 = 1;
const WAVE_MAPPER: u32 = u32::MAX;
const WHDR_DONE: u32 = 0x0000_0001;
const SAMPLE_RATE: u32 = 16_000;
const TONE_MILLISECONDS: u32 = 140;
const OUTPUT_START_STAGGER: Duration = Duration::from_millis(28);
const PLAYBACK_TIMEOUT: Duration = Duration::from_secs(1);

#[repr(C)]
#[derive(Default)]
struct WaveOutCapsW {
    manufacturer_id: u16,
    product_id: u16,
    driver_version: u32,
    name: [u16; 32],
    formats: u32,
    channels: u16,
    reserved: u16,
    support: u32,
}

#[repr(C)]
struct WaveFormatEx {
    format_tag: u16,
    channels: u16,
    samples_per_second: u32,
    average_bytes_per_second: u32,
    block_align: u16,
    bits_per_sample: u16,
    extra_size: u16,
}

#[repr(C)]
struct WaveHeader {
    data: *mut i8,
    buffer_length: u32,
    bytes_recorded: u32,
    user: usize,
    flags: u32,
    loops: u32,
    next: *mut WaveHeader,
    reserved: usize,
}

type WaveOut = *mut c_void;

#[link(name = "winmm")]
unsafe extern "system" {
    fn waveOutGetNumDevs() -> u32;
    fn waveOutGetDevCapsW(device_id: usize, caps: *mut WaveOutCapsW, size: u32) -> u32;
    fn waveOutOpen(
        output: *mut WaveOut,
        device_id: u32,
        format: *const WaveFormatEx,
        callback: usize,
        instance: usize,
        flags: u32,
    ) -> u32;
    fn waveOutPrepareHeader(output: WaveOut, header: *mut WaveHeader, size: u32) -> u32;
    fn waveOutWrite(output: WaveOut, header: *mut WaveHeader, size: u32) -> u32;
    fn waveOutUnprepareHeader(output: WaveOut, header: *mut WaveHeader, size: u32) -> u32;
    fn waveOutReset(output: WaveOut) -> u32;
    fn waveOutClose(output: WaveOut) -> u32;
}

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBeep(kind: u32) -> i32;
}

struct PreparedOutput {
    handle: WaveOut,
    header: Box<WaveHeader>,
    written: bool,
}

#[must_use]
pub fn output_names() -> Vec<String> {
    let mut outputs = vec![DEFAULT_OUTPUT_LABEL.to_owned()];
    let count = unsafe { waveOutGetNumDevs() };
    for device_id in 0..count {
        let mut caps = WaveOutCapsW::default();
        let result = unsafe {
            waveOutGetDevCapsW(
                device_id as usize,
                &raw mut caps,
                size_of::<WaveOutCapsW>() as u32,
            )
        };
        if result == MMSYSERR_NOERROR {
            let name = caps_name(&caps);
            if !name.is_empty() && !outputs.iter().any(|existing| existing == &name) {
                outputs.push(name);
            }
        }
    }
    outputs
}

pub fn play(output_names: &[String]) {
    let outputs = normalized_outputs(output_names);
    let _ = thread::Builder::new()
        .name("ntfy-sound".to_owned())
        .stack_size(192 * 1024)
        .spawn(move || {
            if play_outputs(&outputs).is_err() {
                unsafe {
                    MessageBeep(0x0000_0040);
                }
            }
        });
}

fn normalized_outputs(output_names: &[String]) -> Vec<String> {
    if output_names.is_empty() {
        return vec![String::new()];
    }
    let mut outputs = Vec::with_capacity(output_names.len());
    for output in output_names {
        let output = if output.is_empty() || output == DEFAULT_OUTPUT_LABEL {
            String::new()
        } else {
            output.clone()
        };
        if !outputs.iter().any(|existing| existing == &output) {
            outputs.push(output);
        }
    }
    outputs
}

fn play_outputs(output_names: &[String]) -> Result<(), ()> {
    let mut samples = tone_samples();
    let format = WaveFormatEx {
        format_tag: WAVE_FORMAT_PCM,
        channels: 1,
        samples_per_second: SAMPLE_RATE,
        average_bytes_per_second: SAMPLE_RATE * 2,
        block_align: 2,
        bits_per_sample: 16,
        extra_size: 0,
    };
    let header_size = size_of::<WaveHeader>() as u32;
    let mut prepared = Vec::with_capacity(output_names.len());

    for output_name in output_names {
        if let Some(output) = prepare_output(output_name, &format, &mut samples, header_size) {
            prepared.push(output);
        }
    }
    if prepared.is_empty() {
        return Err(());
    }

    let mut wrote_any = false;
    for (index, output) in prepared.iter_mut().enumerate() {
        if index > 0 {
            thread::sleep(OUTPUT_START_STAGGER);
        }
        output.written = unsafe {
            waveOutWrite(output.handle, output.header.as_mut(), header_size) == MMSYSERR_NOERROR
        };
        wrote_any |= output.written;
    }

    if wrote_any {
        wait_for_completion(&prepared);
    }
    cleanup_outputs(&mut prepared, header_size);

    wrote_any.then_some(()).ok_or(())
}

fn tone_samples() -> Vec<i16> {
    let sample_count = (SAMPLE_RATE * TONE_MILLISECONDS / 1000) as usize;
    let mut samples = Vec::with_capacity(sample_count);
    for index in 0..sample_count {
        let phase = index as f32 * 2.0 * std::f32::consts::PI * 880.0 / SAMPLE_RATE as f32;
        let envelope = 1.0 - index as f32 / sample_count as f32;
        samples.push((phase.sin() * envelope * 7_000.0) as i16);
    }
    samples
}

fn prepare_output(
    output_name: &str,
    format: &WaveFormatEx,
    samples: &mut [i16],
    header_size: u32,
) -> Option<PreparedOutput> {
    let device_id = device_id(output_name)?;
    let mut handle = ptr::null_mut();
    if unsafe { waveOutOpen(&raw mut handle, device_id, format, 0, 0, 0) } != MMSYSERR_NOERROR {
        return None;
    }

    let mut header = Box::new(WaveHeader {
        data: samples.as_mut_ptr().cast::<i8>(),
        buffer_length: u32::try_from(size_of_val(samples)).unwrap_or(u32::MAX),
        bytes_recorded: 0,
        user: 0,
        flags: 0,
        loops: 0,
        next: ptr::null_mut(),
        reserved: 0,
    });
    if unsafe { waveOutPrepareHeader(handle, header.as_mut(), header_size) } != MMSYSERR_NOERROR {
        unsafe {
            waveOutClose(handle);
        }
        return None;
    }

    Some(PreparedOutput {
        handle,
        header,
        written: false,
    })
}

fn wait_for_completion(outputs: &[PreparedOutput]) {
    let deadline = Instant::now() + PLAYBACK_TIMEOUT;
    while Instant::now() < deadline {
        let complete = outputs.iter().all(|output| {
            !output.written
                || unsafe { ptr::read_volatile(&raw const output.header.flags) } & WHDR_DONE != 0
        });
        if complete {
            return;
        }
        thread::sleep(Duration::from_millis(4));
    }
}

fn cleanup_outputs(outputs: &mut [PreparedOutput], header_size: u32) {
    for output in outputs.iter_mut() {
        let done = unsafe { ptr::read_volatile(&raw const output.header.flags) } & WHDR_DONE != 0;
        if output.written && !done {
            unsafe {
                waveOutReset(output.handle);
            }
        }
        unsafe {
            waveOutUnprepareHeader(output.handle, output.header.as_mut(), header_size);
            waveOutClose(output.handle);
        }
    }
}

fn device_id(output_name: &str) -> Option<u32> {
    if output_name.is_empty() {
        Some(WAVE_MAPPER)
    } else {
        find_device(output_name)
    }
}

fn find_device(output_name: &str) -> Option<u32> {
    let count = unsafe { waveOutGetNumDevs() };
    (0..count).find(|device_id| {
        let mut caps = WaveOutCapsW::default();
        let result = unsafe {
            waveOutGetDevCapsW(
                *device_id as usize,
                &raw mut caps,
                size_of::<WaveOutCapsW>() as u32,
            )
        };
        result == MMSYSERR_NOERROR && caps_name(&caps) == output_name
    })
}

fn caps_name(caps: &WaveOutCapsW) -> String {
    let end = caps
        .name
        .iter()
        .position(|character| *character == 0)
        .unwrap_or(caps.name.len());
    String::from_utf16_lossy(&caps.name[..end])
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_OUTPUT_LABEL, WAVE_MAPPER, device_id, normalized_outputs};

    #[test]
    fn default_output_label_is_stable() {
        assert_eq!(DEFAULT_OUTPUT_LABEL, "System default");
    }

    #[test]
    fn default_output_uses_wave_mapper() {
        assert_eq!(device_id(""), Some(WAVE_MAPPER));
    }

    #[test]
    fn output_targets_are_deduplicated_and_defaulted() {
        assert_eq!(normalized_outputs(&[]), vec![""]);
        assert_eq!(
            normalized_outputs(&[
                DEFAULT_OUTPUT_LABEL.to_owned(),
                DEFAULT_OUTPUT_LABEL.to_owned(),
                "Speakers".to_owned(),
                "Speakers".to_owned(),
            ]),
            vec!["", "Speakers"]
        );
    }
}
