use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
use slint::platform::{PlatformError, WindowAdapter};
use slint::{
    ComponentHandle, ModelRc, PhysicalSize, Rgb8Pixel, SharedPixelBuffer, SharedString, VecModel,
};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;

slint::include_modules!();

thread_local! {
    static WINDOW: Rc<MinimalSoftwareWindow> =
        MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
}

struct SnapshotPlatform;

impl slint::platform::Platform for SnapshotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, PlatformError> {
        Ok(WINDOW.with(Clone::clone))
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_dir = output_directory();
    fs::create_dir_all(&output_dir)?;

    slint::platform::set_platform(Box::new(SnapshotPlatform))?;
    let window = WINDOW.with(Clone::clone);
    window.set_size(PhysicalSize::new(1536, 960));

    let ui = AppWindow::new()?;
    configure_ui(&ui);
    ui.show()?;

    let names = [
        "dashboard",
        "topics",
        "history",
        "rules",
        "servers",
        "settings",
        "about",
        "settings-expanded",
    ];

    for (page, name) in names.iter().enumerate() {
        ui.set_page(page as i32);
        ui.window().request_redraw();
        slint::platform::update_timers_and_animations();

        let mut pixels = SharedPixelBuffer::<Rgb8Pixel>::new(1536, 960);
        let rendered = window.draw_if_needed(|renderer| {
            let stride = pixels.width() as usize;
            renderer.render(pixels.make_mut_slice(), stride);
        });
        if !rendered {
            return Err(format!("page {page} did not request a redraw").into());
        }

        let file_name = format!("nocturne-{:02}-{name}.bmp", page + 1);
        write_bmp(&output_dir.join(file_name), &pixels)?;
    }

    ui.hide()?;
    Ok(())
}

fn output_directory() -> PathBuf {
    std::env::args_os()
        .find_map(|argument| {
            argument
                .to_string_lossy()
                .strip_prefix("--output-dir=")
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("output/visual-snapshots"))
}

fn configure_ui(ui: &AppWindow) {
    ui.set_server_url("https://ntfy.sh".into());
    ui.set_topic("ops-deploys".into());
    ui.set_connected(true);
    ui.set_unread_count(19);
    ui.set_topic_count(6);
    ui.set_server_count(2);
    ui.set_status_text("All systems operational".into());
    ui.set_audio_outputs(string_model(&[
        "System default",
        "Speakers (Realtek USB Audio)",
        "Headphones (Bluetooth)",
    ]));
    ui.set_additional_audio_outputs(string_model(&[
        "None",
        "System default",
        "Speakers (Realtek USB Audio)",
        "Headphones (Bluetooth)",
    ]));
    ui.set_notifications(ModelRc::from(Rc::new(VecModel::from(
        sample_notifications(),
    ))));
}

fn write_bmp(path: &Path, pixels: &SharedPixelBuffer<Rgb8Pixel>) -> std::io::Result<()> {
    let width = pixels.width();
    let height = pixels.height();
    let row_bytes = width * 3;
    let padded_row_bytes = (row_bytes + 3) & !3;
    let pixel_bytes = padded_row_bytes * height;
    let file_size = 54 + pixel_bytes;

    let mut writer = BufWriter::new(File::create(path)?);
    writer.write_all(b"BM")?;
    writer.write_all(&file_size.to_le_bytes())?;
    writer.write_all(&[0; 4])?;
    writer.write_all(&54_u32.to_le_bytes())?;
    writer.write_all(&40_u32.to_le_bytes())?;
    writer.write_all(&(width as i32).to_le_bytes())?;
    writer.write_all(&(height as i32).to_le_bytes())?;
    writer.write_all(&1_u16.to_le_bytes())?;
    writer.write_all(&24_u16.to_le_bytes())?;
    writer.write_all(&0_u32.to_le_bytes())?;
    writer.write_all(&pixel_bytes.to_le_bytes())?;
    writer.write_all(&2835_i32.to_le_bytes())?;
    writer.write_all(&2835_i32.to_le_bytes())?;
    writer.write_all(&0_u32.to_le_bytes())?;
    writer.write_all(&0_u32.to_le_bytes())?;

    let source = pixels.as_slice();
    let padding = vec![0_u8; (padded_row_bytes - row_bytes) as usize];
    for y in (0..height).rev() {
        let row_start = (y * width) as usize;
        for pixel in &source[row_start..row_start + width as usize] {
            writer.write_all(&[pixel.b, pixel.g, pixel.r])?;
        }
        writer.write_all(&padding)?;
    }
    writer.flush()
}
fn string_model(values: &[&str]) -> ModelRc<SharedString> {
    let rows = values
        .iter()
        .copied()
        .map(SharedString::from)
        .collect::<Vec<_>>();
    ModelRc::from(Rc::new(VecModel::from(rows)))
}

fn notification(
    topic: &str,
    title: &str,
    message: &str,
    meta: &str,
    priority: i32,
) -> NotificationItem {
    NotificationItem {
        topic: topic.into(),
        title: title.into(),
        message: message.into(),
        meta: meta.into(),
        priority,
    }
}

fn sample_notifications() -> Vec<NotificationItem> {
    vec![
        notification(
            "Server Alerts",
            "Disk pressure on node-04",
            "/var at 96% — evicting pods. Threshold 90% breached for 6 minutes.",
            "00:07 · priority 5 · infrastructure",
            5,
        ),
        notification(
            "Deployments",
            "api-gateway v2.14.1 deployed",
            "Rolled out to production in 3m 18s. Health checks are green.",
            "00:02 · priority 3 · deploy",
            3,
        ),
        notification(
            "Doorbell",
            "Someone at the front door",
            "Motion detected at the front entrance. Camera snapshot is available.",
            "23:41 · priority 4 · home",
            4,
        ),
        notification(
            "CI Builds",
            "Build #4821 passed",
            "All 328 tests passed for commit 8d31f42 on main.",
            "23:20 · priority 2 · ci",
            2,
        ),
        notification(
            "Backups",
            "Nightly backup complete",
            "Encrypted backup uploaded successfully. 18.4 GB in 12m 09s.",
            "22:05 · priority 3 · backup",
            3,
        ),
        notification(
            "Deployments",
            "Rollback triggered on web-01",
            "Error rate crossed 5%. Previous stable release restored.",
            "21:38 · priority 4 · deploy",
            4,
        ),
        notification(
            "Price Watch",
            "ETF close",
            "Global equity ETF closed at 112.84 (+0.62%).",
            "20:00 · priority 1 · finance",
            1,
        ),
        notification(
            "Server Alerts",
            "Certificate expires in 6 days",
            "Renew the certificate for status.example.net before Tuesday.",
            "18:12 · priority 4 · security",
            4,
        ),
        notification(
            "Deployments",
            "Scheduled release window opens",
            "Change window approved for 18:00–19:30 UTC.",
            "18:00 · priority 3 · deploy",
            3,
        ),
        notification(
            "CI Builds",
            "Nightly image published",
            "Container image registry.example.net/app:nightly is available.",
            "03:10 · priority 2 · ci",
            2,
        ),
    ]
}
