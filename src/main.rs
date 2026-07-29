#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod audio;
mod config;
mod notification;
mod protocol;
mod timefmt;
mod winhttp;

use audio::DEFAULT_OUTPUT_LABEL;
use config::Settings;
use notification::Presenter;
use protocol::{Message, truncate, truncate_owned};
use slint::{CloseRequestResponse, ComponentHandle, Model, ModelRc, SharedString, Timer, VecModel};
use std::{
    cell::RefCell,
    collections::VecDeque,
    ffi::OsStr,
    rc::Rc,
    sync::{Arc, Mutex},
    time::Duration,
};
use winhttp::{ClientConfig, Controller, Event};

slint::include_modules!();

const HISTORY_LIMIT: usize = 64;
const HISTORY_BODY_LIMIT: usize = 2048;

type SharedState = Arc<Mutex<RuntimeState>>;
type UiBridge = Arc<Mutex<Option<slint::Weak<AppWindow>>>>;
type UiOwner = Rc<RefCell<Option<AppWindow>>>;

thread_local! {
    static PRESENTER: RefCell<Presenter> = RefCell::new(Presenter::new());
}

#[derive(Clone)]
struct HistoryRecord {
    topic: String,
    title: String,
    message: String,
    meta: String,
    priority: i32,
}

struct RuntimeState {
    settings: Settings,
    token: String,
    connected: bool,
    status: String,
    history: VecDeque<HistoryRecord>,
    audio_outputs: Vec<String>,
}

impl RuntimeState {
    fn new(settings: Settings) -> Self {
        Self {
            settings,
            token: String::new(),
            connected: false,
            status: "Ready".to_owned(),
            history: VecDeque::with_capacity(HISTORY_LIMIT),
            audio_outputs: audio::output_names(),
        }
    }
}

fn main() -> Result<(), slint::PlatformError> {
    select_software_backend()?;

    let smoke_test = has_argument("--smoke-test");
    let background = has_argument("--background");
    let state = Arc::new(Mutex::new(RuntimeState::new(Settings::load())));
    let bridge = Arc::new(Mutex::new(None));
    let owner = Rc::new(RefCell::new(None));
    let controller = Rc::new(RefCell::new(Controller::default()));
    let tray = AppTray::new()?;

    configure_tray(
        &tray,
        Rc::clone(&owner),
        Arc::clone(&bridge),
        Arc::clone(&state),
        Rc::clone(&controller),
    );
    tray.set_connected(false);
    tray.set_tray_visible(true);

    if !background {
        open_window(&owner, &bridge, &state, &tray, Rc::clone(&controller))?;
    }

    let auto_connect = {
        let state = state.lock().expect("runtime state poisoned");
        state.settings.auto_connect && !state.settings.topic.is_empty()
    };
    if auto_connect {
        start_subscription(&state, &bridge, &tray, &controller);
    }

    if smoke_test {
        Timer::single_shot(Duration::from_secs(3), || {
            let _ = slint::quit_event_loop();
        });
    }

    slint::run_event_loop()
}

fn select_software_backend() -> Result<(), slint::PlatformError> {
    slint::BackendSelector::new()
        .backend_name("winit".to_owned())
        .renderer_name("software".to_owned())
        .with_winit_window_attributes_hook(|attributes| attributes.with_transparent(false))
        .select()
}

fn has_argument(expected: &str) -> bool {
    std::env::args_os().any(|argument| argument == OsStr::new(expected))
}

fn open_window(
    owner: &UiOwner,
    bridge: &UiBridge,
    state: &SharedState,
    tray: &AppTray,
    controller: Rc<RefCell<Controller>>,
) -> Result<(), slint::PlatformError> {
    if let Some(ui) = owner.borrow().as_ref().map(ComponentHandle::clone_strong) {
        ui.show()?;
        ui.window().request_redraw();
        return Ok(());
    }

    let ui = AppWindow::new()?;
    hydrate_ui(&ui, state);
    configure_window(
        &ui,
        Rc::downgrade(owner),
        Arc::clone(bridge),
        Arc::clone(state),
        tray.as_weak(),
        controller,
    );
    if let Ok(mut current) = bridge.lock() {
        *current = Some(ui.as_weak());
    }
    ui.show()?;
    *owner.borrow_mut() = Some(ui);
    Ok(())
}

fn hydrate_ui(ui: &AppWindow, state: &SharedState) {
    let state = state.lock().expect("runtime state poisoned");
    ui.set_server_url(state.settings.server_url.clone().into());
    ui.set_topic(state.settings.topic.clone().into());
    ui.set_token(state.token.clone().into());
    ui.set_notifications_enabled(state.settings.notifications_enabled);
    ui.set_sound_enabled(state.settings.sound_enabled);
    ui.set_placement_index(i32::from(state.settings.placement.min(8)));
    ui.set_auto_connect(state.settings.auto_connect);
    ui.set_connected(state.connected);
    ui.set_status_text(state.status.clone().into());
    set_audio_outputs(ui, &state.audio_outputs, &state.settings.audio_output);
    let rows = state
        .history
        .iter()
        .cloned()
        .map(notification_item)
        .collect::<Vec<_>>();
    ui.set_notifications(ModelRc::from(Rc::new(VecModel::from(rows))));
}

fn configure_window(
    ui: &AppWindow,
    owner: std::rc::Weak<RefCell<Option<AppWindow>>>,
    bridge: UiBridge,
    state: SharedState,
    tray: slint::Weak<AppTray>,
    controller: Rc<RefCell<Controller>>,
) {
    let ui_weak = ui.as_weak();
    let state_for_subscription = Arc::clone(&state);
    let bridge_for_subscription = Arc::clone(&bridge);
    let tray_for_subscription = tray.clone();
    let controller_for_subscription = Rc::clone(&controller);
    ui.on_toggle_subscription(move || {
        if let Some(ui) = ui_weak.upgrade() {
            capture_connection_fields(&ui, &state_for_subscription);
        }
        if let Some(tray) = tray_for_subscription.upgrade() {
            toggle_subscription(
                &state_for_subscription,
                &bridge_for_subscription,
                &tray,
                &controller_for_subscription,
            );
        }
    });

    let ui_weak = ui.as_weak();
    let state_for_publish = Arc::clone(&state);
    let bridge_for_publish = Arc::clone(&bridge);
    ui.on_publish(move |title, body| {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        capture_connection_fields(&ui, &state_for_publish);
        let config = client_config_from_state(&state_for_publish);
        set_status(&state_for_publish, &bridge_for_publish, "Publishing");
        let state = Arc::clone(&state_for_publish);
        let bridge = Arc::clone(&bridge_for_publish);
        let result = winhttp::publish(config, title.to_string(), body.to_string(), move |event| {
            let state = Arc::clone(&state);
            let bridge = Arc::clone(&bridge);
            let _ = slint::invoke_from_event_loop(move || {
                apply_event(&state, &bridge, None, event);
            });
        });
        if let Err(error) = result {
            set_status(&state_for_publish, &bridge_for_publish, &error.to_string());
        }
    });

    let ui_weak = ui.as_weak();
    let state_for_save = Arc::clone(&state);
    let bridge_for_save = Arc::clone(&bridge);
    ui.on_save_settings(move || {
        let Some(ui) = ui_weak.upgrade() else {
            return;
        };
        let settings = settings_from_ui(&ui, &state_for_save);
        let status = match settings.save() {
            Ok(()) => {
                let mut state = state_for_save.lock().expect("runtime state poisoned");
                state.settings = settings;
                state.token = ui.get_token().trim().to_owned();
                "Settings saved. Token stays in memory only.".to_owned()
            }
            Err(error) => format!("Could not save settings: {error}"),
        };
        set_status(&state_for_save, &bridge_for_save, &status);
    });

    let ui_weak = ui.as_weak();
    let state_for_clear = Arc::clone(&state);
    ui.on_clear_notifications(move || {
        state_for_clear
            .lock()
            .expect("runtime state poisoned")
            .history
            .clear();
        if let Some(ui) = ui_weak.upgrade() {
            let model = ui.get_notifications();
            if let Some(model) = model.as_any().downcast_ref::<VecModel<NotificationItem>>() {
                model.clear();
            }
        }
    });

    let ui_weak = ui.as_weak();
    let state_for_audio = Arc::clone(&state);
    ui.on_refresh_audio_outputs(move || {
        let outputs = audio::output_names();
        let selected = ui_weak
            .upgrade()
            .map(|ui| selected_audio_output(&ui, &state_for_audio))
            .unwrap_or_default();
        {
            let mut state = state_for_audio.lock().expect("runtime state poisoned");
            state.audio_outputs = outputs;
        }
        if let Some(ui) = ui_weak.upgrade() {
            let state = state_for_audio.lock().expect("runtime state poisoned");
            set_audio_outputs(&ui, &state.audio_outputs, &selected);
        }
    });

    let ui_weak = ui.as_weak();
    let state_for_close = Arc::clone(&state);
    let bridge_for_close = Arc::clone(&bridge);
    ui.window().on_close_requested(move || {
        if let Some(ui) = ui_weak.upgrade() {
            capture_all_fields(&ui, &state_for_close);
        }
        if let Ok(mut current) = bridge_for_close.lock() {
            *current = None;
        }
        if let Some(owner) = owner.upgrade() {
            Timer::single_shot(Duration::ZERO, move || {
                owner.borrow_mut().take();
            });
        }
        CloseRequestResponse::HideWindow
    });

    ui.on_quit(move || {
        controller.borrow_mut().stop();
        let _ = slint::quit_event_loop();
    });
}

fn configure_tray(
    tray: &AppTray,
    owner: UiOwner,
    bridge: UiBridge,
    state: SharedState,
    controller: Rc<RefCell<Controller>>,
) {
    let tray_weak = tray.as_weak();
    let owner_for_show = Rc::clone(&owner);
    let bridge_for_show = Arc::clone(&bridge);
    let state_for_show = Arc::clone(&state);
    let controller_for_show = Rc::clone(&controller);
    tray.on_show_window(move || {
        if let Some(tray) = tray_weak.upgrade() {
            let _ = open_window(
                &owner_for_show,
                &bridge_for_show,
                &state_for_show,
                &tray,
                Rc::clone(&controller_for_show),
            );
        }
    });

    let tray_weak = tray.as_weak();
    let state_for_toggle = Arc::clone(&state);
    let bridge_for_toggle = Arc::clone(&bridge);
    let controller_for_toggle = Rc::clone(&controller);
    tray.on_toggle_subscription(move || {
        if let Some(tray) = tray_weak.upgrade() {
            toggle_subscription(
                &state_for_toggle,
                &bridge_for_toggle,
                &tray,
                &controller_for_toggle,
            );
        }
    });

    tray.on_quit(move || {
        controller.borrow_mut().stop();
        let _ = slint::quit_event_loop();
    });
}

fn toggle_subscription(
    state: &SharedState,
    bridge: &UiBridge,
    tray: &AppTray,
    controller: &Rc<RefCell<Controller>>,
) {
    if controller.borrow().is_running() {
        controller.borrow_mut().stop();
        set_connected(state, bridge, Some(tray), false);
        set_status(state, bridge, "Disconnected");
    } else {
        start_subscription(state, bridge, tray, controller);
    }
}

fn start_subscription(
    state: &SharedState,
    bridge: &UiBridge,
    tray: &AppTray,
    controller: &Rc<RefCell<Controller>>,
) {
    let config = client_config_from_state(state);
    set_status(state, bridge, "Starting subscription");
    let state_for_events = Arc::clone(state);
    let bridge_for_events = Arc::clone(bridge);
    let tray_for_events = tray.as_weak();
    let result = controller.borrow_mut().start(config, move |event| {
        let state = Arc::clone(&state_for_events);
        let bridge = Arc::clone(&bridge_for_events);
        let tray = tray_for_events.clone();
        let _ = slint::invoke_from_event_loop(move || {
            apply_event(&state, &bridge, Some(&tray), event);
        });
    });
    if let Err(error) = result {
        set_status(state, bridge, &error.to_string());
    }
}

fn apply_event(
    state: &SharedState,
    bridge: &UiBridge,
    tray: Option<&slint::Weak<AppTray>>,
    event: Event,
) {
    match event {
        Event::Status(status) => set_status(state, bridge, &status),
        Event::Connected(connected) => {
            let tray = tray.and_then(slint::Weak::upgrade);
            set_connected(state, bridge, tray.as_ref(), connected);
            if connected {
                set_status(state, bridge, "Connected");
            }
        }
        Event::Message(message) => receive_message(state, bridge, message),
        Event::Published => set_status(state, bridge, "Published"),
        Event::Error(error) => set_status(state, bridge, &error),
    }
}

fn receive_message(state: &SharedState, bridge: &UiBridge, message: Message) {
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
    let preferences = notification_preferences(state, bridge);
    let popup_body = preferences.notifications.then(|| truncate(&body, 420));
    let record = HistoryRecord {
        topic,
        title: title.clone(),
        message: truncate_owned(body, HISTORY_BODY_LIMIT),
        meta: meta.clone(),
        priority: i32::from(priority),
    };

    {
        let mut state = state.lock().expect("runtime state poisoned");
        state.history.push_front(record.clone());
        if state.history.len() > HISTORY_LIMIT {
            state.history.pop_back();
        }
        state.status = "Message received".to_owned();
    }

    if let Some(ui) = current_ui(bridge) {
        let model = ui.get_notifications();
        if let Some(model) = model.as_any().downcast_ref::<VecModel<NotificationItem>>() {
            model.insert(0, notification_item(record));
            if model.row_count() > HISTORY_LIMIT {
                model.remove(HISTORY_LIMIT);
            }
        }
        ui.set_status_text("Message received".into());
    }

    PRESENTER.with(|presenter| {
        let presenter = presenter.borrow();
        if let Some(body) = popup_body {
            presenter.show(
                &title,
                &body,
                &meta,
                preferences.placement,
                preferences.sound,
                &preferences.audio_output,
            );
        } else if preferences.sound {
            presenter.play_sound(&preferences.audio_output);
        }
    });
}

struct NotificationPreferences {
    notifications: bool,
    sound: bool,
    placement: i32,
    audio_output: String,
}

fn notification_preferences(state: &SharedState, bridge: &UiBridge) -> NotificationPreferences {
    if let Some(ui) = current_ui(bridge) {
        return NotificationPreferences {
            notifications: ui.get_notifications_enabled(),
            sound: ui.get_sound_enabled(),
            placement: ui.get_placement_index().clamp(0, 8),
            audio_output: selected_audio_output(&ui, state),
        };
    }
    let state = state.lock().expect("runtime state poisoned");
    NotificationPreferences {
        notifications: state.settings.notifications_enabled,
        sound: state.settings.sound_enabled,
        placement: i32::from(state.settings.placement.min(8)),
        audio_output: state.settings.audio_output.clone(),
    }
}

fn capture_connection_fields(ui: &AppWindow, state: &SharedState) {
    let mut state = state.lock().expect("runtime state poisoned");
    state.settings.server_url = ui.get_server_url().trim().trim_end_matches('/').to_owned();
    state.settings.topic = ui.get_topic().trim().to_owned();
    state.token = ui.get_token().trim().to_owned();
}

fn capture_all_fields(ui: &AppWindow, state: &SharedState) {
    let settings = settings_from_ui(ui, state);
    let mut state = state.lock().expect("runtime state poisoned");
    state.settings = settings;
    state.token = ui.get_token().trim().to_owned();
}

fn settings_from_ui(ui: &AppWindow, state: &SharedState) -> Settings {
    Settings {
        server_url: ui.get_server_url().trim().trim_end_matches('/').to_owned(),
        topic: ui.get_topic().trim().to_owned(),
        notifications_enabled: ui.get_notifications_enabled(),
        sound_enabled: ui.get_sound_enabled(),
        audio_output: selected_audio_output(ui, state),
        placement: u8::try_from(ui.get_placement_index().clamp(0, 8)).unwrap_or(2),
        auto_connect: ui.get_auto_connect(),
    }
}

fn client_config_from_state(state: &SharedState) -> ClientConfig {
    let state = state.lock().expect("runtime state poisoned");
    ClientConfig {
        server_url: state.settings.server_url.clone(),
        topic: state.settings.topic.clone(),
        token: state.token.clone(),
    }
}

fn set_status(state: &SharedState, bridge: &UiBridge, status: &str) {
    state.lock().expect("runtime state poisoned").status = status.to_owned();
    if let Some(ui) = current_ui(bridge) {
        ui.set_status_text(status.into());
    }
}

fn set_connected(state: &SharedState, bridge: &UiBridge, tray: Option<&AppTray>, connected: bool) {
    state.lock().expect("runtime state poisoned").connected = connected;
    if let Some(ui) = current_ui(bridge) {
        ui.set_connected(connected);
    }
    if let Some(tray) = tray {
        tray.set_connected(connected);
    }
}

fn current_ui(bridge: &UiBridge) -> Option<AppWindow> {
    bridge
        .lock()
        .ok()
        .and_then(|current| current.as_ref().and_then(slint::Weak::upgrade))
}

fn selected_audio_output(ui: &AppWindow, state: &SharedState) -> String {
    let index = usize::try_from(ui.get_audio_output_index()).unwrap_or(0);
    let state = state.lock().expect("runtime state poisoned");
    match state.audio_outputs.get(index) {
        Some(name) if name != DEFAULT_OUTPUT_LABEL => name.clone(),
        _ => String::new(),
    }
}

fn set_audio_outputs(ui: &AppWindow, outputs: &[String], selected: &str) {
    let index = if selected.is_empty() {
        0
    } else {
        outputs
            .iter()
            .position(|name| name == selected)
            .unwrap_or(0)
    };
    let model = outputs
        .iter()
        .map(|output| SharedString::from(output.as_str()))
        .collect::<Vec<_>>();
    ui.set_audio_outputs(ModelRc::from(Rc::new(VecModel::from(model))));
    ui.set_audio_output_index(i32::try_from(index).unwrap_or(0));
}

fn notification_item(record: HistoryRecord) -> NotificationItem {
    NotificationItem {
        topic: record.topic.into(),
        title: record.title.into(),
        message: record.message.into(),
        meta: record.meta.into(),
        priority: record.priority,
    }
}

#[cfg(test)]
mod tests {
    use super::{HISTORY_BODY_LIMIT, HISTORY_LIMIT};

    #[test]
    fn retained_history_is_small_and_bounded() {
        const {
            assert!(HISTORY_LIMIT <= 64);
            assert!(HISTORY_BODY_LIMIT <= 2048);
        }
    }
}
