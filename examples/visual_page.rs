#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    select_software_backend()?;

    let page = visual_page_argument();
    let ui = AppWindow::new()?;
    ui.set_page(page);
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
    ui.show()?;
    ui.window().request_redraw();

    slint::run_event_loop()
}

fn string_model(values: &[&str]) -> ModelRc<SharedString> {
    let rows = values
        .iter()
        .map(|value| SharedString::from(*value))
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

fn select_software_backend() -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .backend_name("winit".to_owned())
        .renderer_name("software".to_owned())
        .with_winit_window_attributes_hook(|attributes| attributes.with_transparent(false))
        .select()
}

fn visual_page_argument() -> i32 {
    std::env::args()
        .find_map(|argument| {
            argument
                .strip_prefix("--page=")
                .and_then(|value| value.parse::<i32>().ok())
        })
        .unwrap_or(0)
        .clamp(0, 7)
}
