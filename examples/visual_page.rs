#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::rc::Rc;

slint::include_modules!();

fn main() -> Result<(), slint::PlatformError> {
    select_software_backend()?;

    let page = visual_page_argument();
    let ui = AppWindow::new()?;
    ui.set_page(page);
    ui.set_topic("visual-validation".into());
    ui.set_status_text(format!("Visual validation page {page}").into());
    ui.set_audio_outputs(ModelRc::from(Rc::new(VecModel::from(vec![
        SharedString::from("System default"),
    ]))));
    ui.show()?;
    ui.window().request_redraw();

    slint::run_event_loop()
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
        .clamp(0, 3)
}
