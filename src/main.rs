#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod desktop;
mod protocol;
mod winhttp;

use config::{NotificationPosition, Settings};
use slint::{
    CloseRequestResponse, ComponentHandle, Model, ModelRc, PhysicalPosition, PhysicalSize,
    SharedString, Timer, TimerMode, VecModel,
};
use std::{cell::RefCell, rc::Rc, time::Duration};
use winhttp::{ClientConfig, Controller, Event};

slint::include_modules!();

const HISTORY_LIMIT: usize = 64;
const POPUP_WIDTH: f32 = 380.0;
const POPUP_HEIGHT: f32 = 132.0;
const POPUP_DURATION: Duration = Duration::from_secs(6);

thread_local! {
    static POPUP_TIMER: Timer = Timer::default();
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let tray = AppTray::new()?;
    let popup = NotificationPopup::new()?;
    let settings = Settings::load();

    ui.set_server_url(settings.server_url.clone().into());
    ui.set_topic(settings.topic.clone().into());
    ui.set_token(settings.token.clone().into());
    ui.set_notifications_enabled(settings.notify);
    ui.set_sound_enabled(settings.sound);
    ui.set_notification_position_index(settings.notification_position.index());
    ui.set_notifications(ModelRc::from(Rc::new(VecModel::<NotificationItem>::default())));

    let controller = Rc::new(RefCell::new(Controller::default()));

    {
        let popup_weak = popup.as_weak();
        popup.on_dismiss(move || {
            if let Some(popup) = popup_weak.upgrade() {
                let _ = popup.hide();
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let popup_weak = popup.as_weak();
        let controller = Rc::clone(&controller);
        ui.on_toggle_subscription(move || {
            if controller.borrow().is_running() {
                controller.borrow_mut().stop();
                if let Some(ui) = ui_weak.upgrade() {
                    ui.set_connected(false);
                    ui.set_status_text("Disconnected".into());
                }
                return;
            }
            let Some(ui) = ui_weak.upgrade() else { return };
            let config = client_config(&ui);
            let ui_weak_events = ui.as_weak();
            let popup_weak_events = popup_weak.clone();
            let result = controller.borrow_mut().start(config, move |event| {
                let ui_weak = ui_weak_events.clone();
                let popup_weak = popup_weak_events.clone();
                let _ = slint::invoke_from_event_loop(move || {
                    apply_event(&ui_weak, &popup_weak, event);
                });
            });
            match result {
                Ok(()) => ui.set_status_text("Starting".into()),
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        let popup_weak = popup.as_weak();
        ui.on_publish(move |title, body| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let config = client_config(&ui);
            ui.set_status_text("Publishing".into());
            let ui_weak_events = ui.as_weak();
            let popup_weak_events = popup_weak.clone();
            if let Err(error) =
                winhttp::publish(config, title.to_string(), body.to_string(), move |event| {
                    let ui_weak = ui_weak_events.clone();
                    let popup_weak = popup_weak_events.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        apply_event(&ui_weak, &popup_weak, event);
                    });
                })
            {
                ui.set_status_text(error.to_string().into());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_save_settings(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let settings = settings_from_ui(&ui);
                let status = match settings.save() {
                    Ok(()) => "Settings saved".to_owned(),
                    Err(error) => format!("Could not save settings: {error}"),
                };
                ui.set_status_text(status.into());
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.window().on_close_requested(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let _ = settings_from_ui(&ui).save();
                ui.set_status_text("Running in the notification area".into());
            }
            CloseRequestResponse::HideWindow
        });
    }

    {
        let ui_weak = ui.as_weak();
        tray.on_show_window(move || {
            if let Some(ui) = ui_weak.upgrade() {
                let _ = ui.show();
                ui.window().request_redraw();
            }
        });
    }

    {
        let controller = Rc::clone(&controller);
        tray.on_quit(move || {
            POPUP_TIMER.with(|timer| timer.stop());
            controller.borrow_mut().stop();
            let _ = slint::quit_event_loop();
        });
    }

    {
        let controller = Rc::clone(&controller);
        ui.on_quit(move || {
            POPUP_TIMER.with(|timer| timer.stop());
            controller.borrow_mut().stop();
            let _ = slint::quit_event_loop();
        });
    }

    tray.set_tray_visible(true);
    ui.run()
}

fn client_config(ui: &AppWindow) -> ClientConfig {
    ClientConfig {
        server_url: ui.get_server_url().to_string(),
        topic: ui.get_topic().to_string(),
        token: ui.get_token().to_string(),
    }
}

fn settings_from_ui(ui: &AppWindow) -> Settings {
    Settings {
        server_url: ui.get_server_url().to_string(),
        topic: ui.get_topic().to_string(),
        token: ui.get_token().to_string(),
        notify: ui.get_notifications_enabled(),
        sound: ui.get_sound_enabled(),
        notification_position: NotificationPosition::from_index(
            ui.get_notification_position_index(),
        ),
    }
}

fn apply_event(
    ui_weak: &slint::Weak<AppWindow>,
    popup_weak: &slint::Weak<NotificationPopup>,
    event: Event,
) {
    let Some(ui) = ui_weak.upgrade() else { return };
    match event {
        Event::Status(status) => ui.set_status_text(status.into()),
        Event::Connected(connected) => {
            ui.set_connected(connected);
            if connected {
                ui.set_status_text("Connected".into());
            }
        }
        Event::Message(message) => {
            let title = display_title(&message.title);
            let popup_body = truncate(&message.body, 512);
            let topic = truncate(&message.topic, 64);
            let mut rows: Vec<NotificationItem> = ui.get_notifications().iter().collect();
            if rows.len() >= HISTORY_LIMIT {
                rows.remove(0);
            }
            rows.push(NotificationItem {
                topic: topic.clone().into(),
                title: title.clone().into(),
                message: truncate(&message.body, 4096).into(),
                time: format_time(message.time),
                priority: i32::from(message.priority),
            });
            ui.set_notifications(ModelRc::from(Rc::new(VecModel::from(rows))));
            ui.set_status_text("Message received".into());

            if ui.get_notifications_enabled() {
                show_popup(
                    popup_weak,
                    &title,
                    &popup_body,
                    &topic,
                    NotificationPosition::from_index(ui.get_notification_position_index()),
                    ui.get_sound_enabled(),
                );
            }
        }
        Event::Published => ui.set_status_text("Published".into()),
        Event::Error(error) => ui.set_status_text(error.into()),
    }
}

fn show_popup(
    popup_weak: &slint::Weak<NotificationPopup>,
    title: &str,
    body: &str,
    topic: &str,
    position: NotificationPosition,
    play_sound: bool,
) {
    let Some(popup) = popup_weak.upgrade() else { return };
    popup.set_notification_title(title.into());
    popup.set_notification_message(body.into());
    popup.set_notification_topic(topic.into());

    let scale = popup.window().scale_factor().max(1.0);
    let width = (POPUP_WIDTH * scale).round() as u32;
    let height = (POPUP_HEIGHT * scale).round() as u32;
    popup.window().set_size(PhysicalSize::new(width, height));
    let (x, y) = desktop::popup_origin(width as i32, height as i32, position);
    popup.window().set_position(PhysicalPosition::new(x, y));
    let _ = popup.show();
    popup.window().request_redraw();

    if play_sound {
        desktop::play_notification_sound();
    }

    POPUP_TIMER.with(|timer| {
        let popup_weak = popup.as_weak();
        timer.start(TimerMode::SingleShot, POPUP_DURATION, move || {
            if let Some(popup) = popup_weak.upgrade() {
                let _ = popup.hide();
            }
        });
    });
}

fn display_title(title: &str) -> String {
    if title.trim().is_empty() { "Notification".to_owned() } else { truncate(title, 256) }
}

fn truncate(value: &str, max_chars: usize) -> String {
    let mut chars = value.chars();
    let mut output: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        output.push('…');
    }
    output
}

fn format_time(unix_seconds: i64) -> SharedString {
    if unix_seconds <= 0 {
        return "now".into();
    }
    format!("Unix {unix_seconds}").into()
}

#[cfg(test)]
mod tests {
    use super::truncate;

    #[test]
    fn truncates_on_character_boundary() {
        assert_eq!(truncate("abcdef", 3), "abc…");
        assert_eq!(truncate("åäö", 3), "åäö");
    }
}
