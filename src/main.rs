#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod config;
mod notification;
mod protocol;
mod timefmt;
mod winhttp;

use config::Settings;
use notification::Presenter;
use protocol::{Message, truncate, truncate_owned};
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, Timer, TimerMode, VecModel};
use std::{cell::RefCell, ffi::OsStr, rc::Rc, time::Duration};
use winhttp::{ClientConfig, Controller, Event};

slint::include_modules!();

const HISTORY_LIMIT: usize = 100;
const HISTORY_BODY_LIMIT: usize = 4096;

thread_local! {
    static PRESENTER: RefCell<Option<Presenter>> = const { RefCell::new(None) };
}

fn main() -> Result<(), slint::PlatformError> {
    let ui = AppWindow::new()?;
    let tray = AppTray::new()?;
    PRESENTER.with(|slot| {
        *slot.borrow_mut() = Some(Presenter::new()?);
        Ok::<(), slint::PlatformError>(())
    })?;
    let controller = Rc::new(RefCell::new(Controller::default()));
    let settings = Settings::load();
    let smoke_test = std::env::args_os().any(|argument| argument == OsStr::new("--smoke-test"));

    ui.set_server_url(settings.server_url.clone().into());
    ui.set_topic(settings.topic.clone().into());
    ui.set_notifications_enabled(settings.notifications_enabled);
    ui.set_sound_enabled(settings.sound_enabled);
    ui.set_placement_index(i32::from(settings.placement.min(8)));
    ui.set_auto_connect(settings.auto_connect);
    ui.set_notifications(ModelRc::from(Rc::new(
        VecModel::<NotificationItem>::default(),
    )));

    configure_subscription(&ui, &tray, Rc::clone(&controller));
    configure_publish(&ui);
    configure_settings(&ui);
    configure_clear(&ui);
    configure_window_close(&ui);
    configure_tray(&ui, &tray, Rc::clone(&controller));
    configure_quit(&ui, Rc::clone(&controller));

    tray.set_connected(false);
    tray.set_tray_visible(true);

    if settings.auto_connect && !settings.topic.is_empty() {
        ui.invoke_toggle_subscription();
    }

    if smoke_test {
        let timer = Timer::default();
        timer.start(TimerMode::SingleShot, Duration::from_secs(3), || {
            let _ = slint::quit_event_loop();
        });
        return ui.run();
    }

    ui.run()
}

fn configure_subscription(ui: &AppWindow, tray: &AppTray, controller: Rc<RefCell<Controller>>) {
    let ui_weak = ui.as_weak();
    let tray_weak = tray.as_weak();
    ui.on_toggle_subscription(move || {
        if controller.borrow().is_running() {
            controller.borrow_mut().stop();
            if let Some(ui) = ui_weak.upgrade() {
                ui.set_connected(false);
                ui.set_status_text("Disconnected".into());
            }
            if let Some(tray) = tray_weak.upgrade() {
                tray.set_connected(false);
            }
            return;
        }

        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let config = client_config(&ui);
        ui.set_status_text("Starting subscription".into());
        let ui_events = ui.as_weak();
        let tray_events = tray_weak.clone();
        let result = controller.borrow_mut().start(config, move |event| {
            let ui_weak = ui_events.clone();
            let tray_weak = tray_events.clone();
            let _ = slint::invoke_from_event_loop(move || {
                apply_event(&ui_weak, Some(&tray_weak), event);
            });
        });
        if let Err(error) = result {
            ui.set_status_text(error.to_string().into());
        }
    });
}

fn configure_publish(ui: &AppWindow) {
    let ui_weak = ui.as_weak();
    ui.on_publish(move |title, body| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let config = client_config(&ui);
        ui.set_status_text("Publishing".into());
        let ui_events = ui.as_weak();
        let result = winhttp::publish(config, title.to_string(), body.to_string(), move |event| {
            let ui_weak = ui_events.clone();
            let _ = slint::invoke_from_event_loop(move || {
                apply_event(&ui_weak, None, event);
            });
        });
        if let Err(error) = result {
            ui.set_status_text(error.to_string().into());
        }
    });
}

fn configure_settings(ui: &AppWindow) {
    let ui_weak = ui.as_weak();
    ui.on_save_settings(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let status = match settings_from_ui(&ui).save() {
                Ok(()) => "Settings saved. The bearer token stays in memory only.".to_owned(),
                Err(error) => format!("Could not save settings: {error}"),
            };
            ui.set_status_text(status.into());
        }
    });
}

fn configure_clear(ui: &AppWindow) {
    let ui_weak = ui.as_weak();
    ui.on_clear_notifications(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let model = ui.get_notifications();
            if let Some(model) = model.as_any().downcast_ref::<VecModel<NotificationItem>>() {
                model.clear();
            }
        }
    });
}

fn configure_window_close(ui: &AppWindow) {
    let ui_weak = ui.as_weak();
    ui.window().on_close_requested(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let _ = ui.hide();
            ui.set_status_text("Running in the system tray".into());
        }
        CloseRequestResponse::KeepWindowShown
    });
}

fn configure_tray(ui: &AppWindow, tray: &AppTray, controller: Rc<RefCell<Controller>>) {
    let ui_weak = ui.as_weak();
    tray.on_show_window(move || {
        if let Some(ui) = ui_weak.upgrade() {
            let _ = ui.show();
            ui.window().request_redraw();
        }
    });

    let ui_weak = ui.as_weak();
    let tray_weak = tray.as_weak();
    let controller_for_toggle = Rc::clone(&controller);
    tray.on_toggle_subscription(move || {
        if let Some(ui) = ui_weak.upgrade() {
            ui.invoke_toggle_subscription();
        } else if controller_for_toggle.borrow().is_running() {
            controller_for_toggle.borrow_mut().stop();
            if let Some(tray) = tray_weak.upgrade() {
                tray.set_connected(false);
            }
        }
    });

    tray.on_quit(move || {
        controller.borrow_mut().stop();
        let _ = slint::quit_event_loop();
    });
}

fn configure_quit(ui: &AppWindow, controller: Rc<RefCell<Controller>>) {
    ui.on_quit(move || {
        controller.borrow_mut().stop();
        let _ = slint::quit_event_loop();
    });
}

fn client_config(ui: &AppWindow) -> ClientConfig {
    ClientConfig {
        server_url: ui.get_server_url().trim().trim_end_matches('/').to_owned(),
        topic: ui.get_topic().trim().to_owned(),
        token: ui.get_token().trim().to_owned(),
    }
}

fn settings_from_ui(ui: &AppWindow) -> Settings {
    Settings {
        server_url: ui.get_server_url().trim().trim_end_matches('/').to_owned(),
        topic: ui.get_topic().trim().to_owned(),
        notifications_enabled: ui.get_notifications_enabled(),
        sound_enabled: ui.get_sound_enabled(),
        placement: u8::try_from(ui.get_placement_index().clamp(0, 8)).unwrap_or(2),
        auto_connect: ui.get_auto_connect(),
    }
}

fn with_presenter(action: impl FnOnce(&Presenter)) {
    PRESENTER.with(|slot| {
        if let Some(presenter) = slot.borrow().as_ref() {
            action(presenter);
        }
    });
}

fn apply_event(
    ui_weak: &slint::Weak<AppWindow>,
    tray_weak: Option<&slint::Weak<AppTray>>,
    event: Event,
) {
    let Some(ui) = ui_weak.upgrade() else {
        return;
    };
    match event {
        Event::Status(status) => ui.set_status_text(status.into()),
        Event::Connected(connected) => {
            ui.set_connected(connected);
            if let Some(tray) = tray_weak.and_then(slint::Weak::upgrade) {
                tray.set_connected(connected);
            }
            if connected {
                ui.set_status_text("Connected".into());
            }
        }
        Event::Message(message) => {
            let Message {
                id: _,
                time,
                topic,
                title: incoming_title,
                body,
                priority,
                tags,
            } = message;
            let title = if incoming_title.trim().is_empty() {
                format!("ntfy · {topic}")
            } else {
                incoming_title
            };
            let time = timefmt::format_unix_utc(time);
            let tags = if tags.is_empty() {
                String::new()
            } else {
                format!(" · {}", tags.join(", "))
            };
            let meta = format!("{time} · priority {priority}{tags}");
            let notifications_enabled = ui.get_notifications_enabled();
            let sound_enabled = ui.get_sound_enabled();
            let placement = ui.get_placement_index();
            let popup_body = notifications_enabled.then(|| truncate(&body, 500));
            let history_body = truncate_owned(body, HISTORY_BODY_LIMIT);

            let model = ui.get_notifications();
            if let Some(model) = model.as_any().downcast_ref::<VecModel<NotificationItem>>() {
                model.insert(
                    0,
                    NotificationItem {
                        topic: topic.into(),
                        title: title.clone().into(),
                        message: history_body.into(),
                        meta: meta.clone().into(),
                        priority: i32::from(priority),
                    },
                );
                if model.row_count() > HISTORY_LIMIT {
                    model.remove(HISTORY_LIMIT);
                }
            }

            if let Some(popup_body) = popup_body {
                with_presenter(|presenter| {
                    presenter.show(&title, &popup_body, &meta, placement, sound_enabled);
                });
            } else if sound_enabled {
                with_presenter(|presenter| presenter.play_sound());
            }
            ui.set_status_text("Message received".into());
        }
        Event::Published => ui.set_status_text("Published".into()),
        Event::Error(error) => ui.set_status_text(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::{HISTORY_BODY_LIMIT, HISTORY_LIMIT};

    #[test]
    fn history_is_bounded() {
        const {
            assert!(HISTORY_LIMIT <= 200);
            assert!(HISTORY_BODY_LIMIT <= 16 * 1024);
        };
    }
}
