#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use ntfy_windows_client::{
    config::{self, Settings},
    ntfy::{self, Connection, Event},
    timefmt, toast,
};
use slint::{ComponentHandle, Model, VecModel};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
    mpsc,
};

slint::include_modules!();

const MAX_MESSAGES: usize = 200;

fn main() -> Result<(), slint::PlatformError> {
    let ui = MainWindow::new()?;
    let settings = Settings::load().unwrap_or_default();
    ui.set_server(settings.server.clone().into());
    ui.set_topic(settings.topic.clone().into());
    ui.set_notifications_enabled(settings.notifications_enabled);
    ui.set_messages(slint::ModelRc::new(VecModel::default()));

    let generation = Arc::new(AtomicU64::new(0));
    let active_connection = Arc::new(Mutex::new(None::<Connection>));
    let (event_sender, event_receiver) = mpsc::sync_channel::<Event>(256);
    ntfy::spawn_subscription_worker(
        Arc::clone(&active_connection),
        Arc::clone(&generation),
        event_sender.clone(),
    );

    configure_connect_callback(&ui, Arc::clone(&generation), Arc::clone(&active_connection));
    configure_publish_callback(
        &ui,
        Arc::clone(&active_connection),
        Arc::new(AtomicBool::new(false)),
    );
    configure_clear_callback(&ui);
    configure_event_pump(&ui, event_receiver, Arc::clone(&generation));

    if !settings.topic.is_empty() {
        ui.invoke_connect_requested(
            settings.server.into(),
            settings.topic.into(),
            "".into(),
            settings.notifications_enabled,
        );
    }

    ui.run()
}

fn configure_connect_callback(
    ui: &MainWindow,
    generation: Arc<AtomicU64>,
    active_connection: Arc<Mutex<Option<Connection>>>,
) {
    let weak = ui.as_weak();
    ui.on_connect_requested(move |server, topic, token, notifications_enabled| {
        let (server, topic) = match config::validate(&server, &topic) {
            Ok(value) => value,
            Err(error) => {
                if let Some(ui) = weak.upgrade() {
                    ui.set_status(error.into());
                    ui.set_connected(false);
                }
                return;
            }
        };

        let connection = Connection {
            server: server.clone(),
            topic: topic.clone(),
            token: token.trim().to_owned(),
        };
        let Ok(mut current) = active_connection.lock() else {
            if let Some(ui) = weak.upgrade() {
                ui.set_status("Internal connection state error".into());
                ui.set_connected(false);
            }
            return;
        };
        *current = Some(connection);
        drop(current);
        generation.fetch_add(1, Ordering::AcqRel);

        let settings = Settings {
            server,
            topic,
            notifications_enabled,
        };
        let save_result = settings.save();

        if let Some(ui) = weak.upgrade() {
            clear_model(&ui);
            ui.set_connected(false);
            ui.set_status(
                save_result
                    .map_or_else(
                        |error| format!("Connecting (settings not saved: {error})"),
                        |()| "Connecting…".to_owned(),
                    )
                    .into(),
            );
        }
    });
}

fn configure_publish_callback(
    ui: &MainWindow,
    active: Arc<Mutex<Option<Connection>>>,
    publishing: Arc<AtomicBool>,
) {
    let weak = ui.as_weak();
    ui.on_publish_requested(move |title, body| {
        if publishing.swap(true, Ordering::AcqRel) {
            if let Some(ui) = weak.upgrade() {
                ui.set_status("A publish request is already running".into());
            }
            return;
        }
        let connection = active.lock().ok().and_then(|value| value.clone());
        let Some(connection) = connection else {
            if let Some(ui) = weak.upgrade() {
                ui.set_status("Connect to a topic before publishing".into());
            }
            publishing.store(false, Ordering::Release);
            return;
        };
        if body.trim().is_empty() {
            if let Some(ui) = weak.upgrade() {
                ui.set_status("Message cannot be empty".into());
            }
            publishing.store(false, Ordering::Release);
            return;
        }

        if let Some(ui) = weak.upgrade() {
            ui.set_status("Publishing…".into());
        }
        let weak_for_thread = weak.clone();
        let publishing_for_thread = Arc::clone(&publishing);
        std::thread::Builder::new()
            .name("ntfy-publish".to_owned())
            .spawn(move || {
                let result = ntfy::publish(&connection, &title, &body);
                publishing_for_thread.store(false, Ordering::Release);
                let _ = weak_for_thread.upgrade_in_event_loop(move |ui| match result {
                    Ok(()) => ui.set_status("Published".into()),
                    Err(error) => ui.set_status(format!("Publish failed: {error}").into()),
                });
            })
            .expect("failed to start publish thread");
    });
}

fn configure_clear_callback(ui: &MainWindow) {
    let weak = ui.as_weak();
    ui.on_clear_requested(move || {
        if let Some(ui) = weak.upgrade() {
            clear_model(&ui);
        }
    });
}

fn configure_event_pump(
    ui: &MainWindow,
    receiver: mpsc::Receiver<Event>,
    generation: Arc<AtomicU64>,
) {
    let weak = ui.as_weak();
    std::thread::Builder::new()
        .name("ntfy-ui-events".to_owned())
        .spawn(move || {
            for event in receiver {
                if event.generation() != generation.load(Ordering::Acquire) {
                    continue;
                }
                let weak_for_event = weak.clone();
                if weak_for_event
                    .upgrade_in_event_loop(move |ui| handle_event(&ui, event))
                    .is_err()
                {
                    break;
                }
            }
        })
        .expect("failed to start UI event thread");
}

fn handle_event(ui: &MainWindow, event: Event) {
    match event {
        Event::Connected { .. } => {
            ui.set_connected(true);
            ui.set_status("Connected".into());
        }
        Event::Status {
            message: status, ..
        } => {
            ui.set_connected(false);
            ui.set_status(status.into());
        }
        Event::Message { message, .. } => {
            let title = if message.title.is_empty() {
                format!("ntfy · {}", message.topic)
            } else {
                message.title.clone()
            };
            let meta = format!(
                "{} · priority {}{}",
                timefmt::format_unix_utc(message.timestamp),
                message.priority,
                if message.tags.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", message.tags.join(", "))
                }
            );
            let item = NotificationItem {
                title: title.clone().into(),
                message: message.body.clone().into(),
                meta: meta.into(),
                priority: message.priority,
            };
            let model = ui.get_messages();
            if let Some(model) = model.as_any().downcast_ref::<VecModel<NotificationItem>>() {
                model.insert(0, item);
                if model.row_count() > MAX_MESSAGES {
                    model.remove(MAX_MESSAGES);
                }
            }
            if ui.get_notifications_enabled() {
                toast::show(&title, &message.body);
            }
        }
    }
}

fn clear_model(ui: &MainWindow) {
    let model = ui.get_messages();
    if let Some(model) = model.as_any().downcast_ref::<VecModel<NotificationItem>>() {
        model.clear();
    }
}
