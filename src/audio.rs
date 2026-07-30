use std::{
    ffi::c_void,
    mem::size_of,
    ptr, thread,
    time::{Duration, Instant},
};

pub const DEFAULT_OUTPUT_LABEL: &str = "System default";

const MMSYSERR_NOERROR: u32 = 0;
const WAVE_FORMAT_PCM: u16 = 1;
const WHDR_DONE: u32 = 0x0000_0001;
const SAMPLE_RATE: u32 = 16_000;
const TONE_MILLISECONDS: u32 = 140;

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
            let end = caps
                .name
                .iter()
                .position(|character| *character == 0)
                .unwrap_or(caps.name.len());
            let name = String::from_utf16_lossy(&caps.name[..end]);
            if !name.is_empty() && !outputs.iter().any(|existing| existing == &name) {
                outputs.push(name);
            }
        }
    }
    outputs
}

pub fn play(output_names: &[String]) {
    for (index, output_name) in normalized_outputs(output_names).into_iter().enumerate() {
        let _ = thread::Builder::new()
            .name(format!("ntfy-sound-{index}"))
            .stack_size(128 * 1024)
            .spawn(move || {
                if output_name.is_empty() || play_tone(&output_name).is_err() {
                    unsafe {
                        MessageBeep(0x0000_0040);
                    }
                }
            });
    }
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

fn play_tone(output_name: &str) -> Result<(), ()> {
    let device_id = find_device(output_name).ok_or(())?;
    let format = WaveFormatEx {
        format_tag: WAVE_FORMAT_PCM,
        channels: 1,
        samples_per_second: SAMPLE_RATE,
        average_bytes_per_second: SAMPLE_RATE * 2,
        block_align: 2,
        bits_per_sample: 16,
        extra_size: 0,
    };
    let sample_count = (SAMPLE_RATE * TONE_MILLISECONDS / 1000) as usize;
    let mut samples = Vec::with_capacity(sample_count);
    for index in 0..sample_count {
        let phase = index as f32 * 2.0 * std::f32::consts::PI * 880.0 / SAMPLE_RATE as f32;
        let envelope = 1.0 - index as f32 / sample_count as f32;
        samples.push((phase.sin() * envelope * 7_000.0) as i16);
    }

    let mut output = ptr::null_mut();
    if unsafe { waveOutOpen(&raw mut output, device_id, &raw const format, 0, 0, 0) }
        != MMSYSERR_NOERROR
    {
        return Err(());
    }

    let mut header = WaveHeader {
        data: samples.as_mut_ptr().cast::<i8>(),
        buffer_length: (samples.len() * size_of::<i16>()) as u32,
        bytes_recorded: 0,
        user: 0,
        flags: 0,
        loops: 0,
        next: ptr::null_mut(),
        reserved: 0,
    };
    let header_size = size_of::<WaveHeader>() as u32;
    let prepared = unsafe { waveOutPrepareHeader(output, &raw mut header, header_size) };
    if prepared != MMSYSERR_NOERROR {
        unsafe {
            waveOutClose(output);
        }
        return Err(());
    }

    let written = unsafe { waveOutWrite(output, &raw mut header, header_size) };
    if written == MMSYSERR_NOERROR {
        let deadline = Instant::now() + Duration::from_secs(1);
        while Instant::now() < deadline
            && unsafe { ptr::read_volatile(&raw const header.flags) } & WHDR_DONE == 0
        {
            thread::sleep(Duration::from_millis(5));
        }
        if unsafe { ptr::read_volatile(&raw const header.flags) } & WHDR_DONE == 0 {
            unsafe {
                waveOutReset(output);
            }
        }
    }

    unsafe {
        waveOutUnprepareHeader(output, &raw mut header, header_size);
        waveOutClose(output);
    }
    (written == MMSYSERR_NOERROR).then_some(()).ok_or(())
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
        if result != MMSYSERR_NOERROR {
            return false;
        }
        let end = caps
            .name
            .iter()
            .position(|character| *character == 0)
            .unwrap_or(caps.name.len());
        String::from_utf16_lossy(&caps.name[..end]) == output_name
    })
}

#[cfg(test)]
mod tests {
    use super::{DEFAULT_OUTPUT_LABEL, normalized_outputs};

    #[test]
    fn default_output_label_is_stable() {
        assert_eq!(DEFAULT_OUTPUT_LABEL, "System default");
    }

    #[test]
    fn output_targets_are_deduplicated_and_defaulted() {
        assert_eq!(normalized_outputs(&[]), vec![""]);
        assert_eq!(
            normalized_outputs(&[
                DEFAULT_OUTPUT_LABEL.to_owned(),
                String::new(),
                "Headphones".to_owned(),
                "Headphones".to_owned(),
            ]),
            vec!["", "Headphones"]
        );
    }
}
