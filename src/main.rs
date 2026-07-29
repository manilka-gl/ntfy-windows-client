#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod protocol;
mod toast;
mod winhttp;

use config::Settings;
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::{cell::RefCell, rc::Rc};
use winhttp::{ClientConfig, Controller, Event};

slint::include_modules!();

const HISTORY_LIMIT: usize = 64;

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let tray = AppTray::new()?;
    let settings = Settings::load();
    ui.set_server_url(settings.server_url.clone().into());
    ui.set_topic(settings.topic.clone().into());
    ui.set_token(settings.token.clone().into());
    ui.set_notifications_enabled(settings.notify);

    ui.set_notifications(ModelRc::from(Rc::new(VecModel::<NotificationItem>::default())));
    let controller = Rc::new(RefCell::new(Controller::default()));

    {
        let ui_weak = ui.as_weak();
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
            let result = controller.borrow_mut().start(config, move |event| {
                let ui_weak = ui_weak_events.clone();
                let _ = slint::invoke_from_event_loop(move || apply_event(&ui_weak, event));
            });
            match result {
                Ok(()) => ui.set_status_text("Starting".into()),
                Err(error) => ui.set_status_text(error.to_string().into()),
            }
        });
    }

    {
        let ui_weak = ui.as_weak();
        ui.on_publish(move |title, body| {
            let Some(ui) = ui_weak.upgrade() else { return };
            let config = client_config(&ui);
            ui.set_status_text("Publishing".into());
            let ui_weak_events = ui.as_weak();
            if let Err(error) = winhttp::publish(
                config,
                title.to_string(),
                body.to_string(),
                move |event| {
                    let ui_weak = ui_weak_events.clone();
                    let _ = slint::invoke_from_event_loop(move || {
                        apply_event(&ui_weak, event);
                    });
                },
            ) {
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
                let _ = ui.hide();
            }
            CloseRequestResponse::KeepWindowShown
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
            controller.borrow_mut().stop();
            let _ = slint::quit_event_loop();
        });
    }

    {
        let controller = Rc::clone(&controller);
        ui.on_quit(move || {
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
        notify: ui.get_notifications_enabled(),
    }
}

fn settings_from_ui(ui: &AppWindow) -> Settings {
    Settings {
        server_url: ui.get_server_url().to_string(),
        topic: ui.get_topic().to_string(),
        token: ui.get_token().to_string(),
        notify: ui.get_notifications_enabled(),
    }
}

fn apply_event(ui_weak: &slint::Weak<AppWindow>, event: Event) {
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
            let mut rows: Vec<NotificationItem> = ui.get_notifications().iter().collect();
            if rows.len() >= HISTORY_LIMIT {
                rows.remove(0);
            }
            rows.push(NotificationItem {
                topic: message.topic.into(),
                title: display_title(&message.title).into(),
                message: truncate(&message.body, 4096).into(),
                time: format_time(message.time),
                priority: i32::from(message.priority),
            });
            ui.set_notifications(ModelRc::from(Rc::new(VecModel::from(rows))));
            ui.set_status_text("Message received".into());
        }
        Event::Published => ui.set_status_text("Published".into()),
        Event::Error(error) => ui.set_status_text(error.into()),
    }
}

fn display_title(title: &str) -> String {
    if title.trim().is_empty() {
        "Notification".to_owned()
    } else {
        truncate(title, 256)
    }
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
