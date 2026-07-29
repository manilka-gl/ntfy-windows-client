use crate::{NotificationPopup, audio};
use slint::{ComponentHandle, PhysicalPosition, Timer, TimerMode};
use std::{cell::RefCell, rc::Rc, time::Duration};
use windows_sys::Win32::{
    Foundation::RECT,
    UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW},
};

const MARGIN: i32 = 16;
const DISPLAY_TIME: Duration = Duration::from_millis(6200);

pub struct Presenter {
    popup: Rc<RefCell<Option<NotificationPopup>>>,
    timer: Timer,
}

impl Presenter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            popup: Rc::new(RefCell::new(None)),
            timer: Timer::default(),
        }
    }

    pub fn play_sound(&self, output_name: &str) {
        audio::play(output_name);
    }

    pub fn show(
        &self,
        title: &str,
        body: &str,
        meta: &str,
        placement: i32,
        play_sound: bool,
        output_name: &str,
    ) {
        let popup = {
            let mut slot = self.popup.borrow_mut();
            if slot.is_none() {
                let Ok(popup) = NotificationPopup::new() else {
                    if play_sound {
                        self.play_sound(output_name);
                    }
                    return;
                };
                let weak = popup.as_weak();
                popup.on_dismiss(move || {
                    if let Some(popup) = weak.upgrade() {
                        let _ = popup.hide();
                    }
                });
                *slot = Some(popup);
            }
            slot.as_ref().expect("popup initialized").clone_strong()
        };

        popup.set_popup_title(title.into());
        popup.set_popup_body(body.into());
        popup.set_popup_meta(meta.into());
        if popup.show().is_err() {
            return;
        }
        position_popup(&popup, placement);
        popup.window().request_redraw();

        if play_sound {
            self.play_sound(output_name);
        }

        let slot = Rc::clone(&self.popup);
        self.timer
            .start(TimerMode::SingleShot, DISPLAY_TIME, move || {
                if let Some(popup) = slot.borrow_mut().take() {
                    let _ = popup.hide();
                }
            });
    }
}

fn position_popup(popup: &NotificationPopup, placement: i32) {
    let work = work_area();
    let size = popup.window().size();
    let width = i32::try_from(size.width).unwrap_or(i32::MAX);
    let height = i32::try_from(size.height).unwrap_or(i32::MAX);
    let available_width = (work.right - work.left - width).max(0);
    let available_height = (work.bottom - work.top - height).max(0);
    let placement = placement.clamp(0, 8);
    let column = placement % 3;
    let row = placement / 3;

    let x = match column {
        0 => work.left + MARGIN,
        1 => work.left + available_width / 2,
        _ => work.left + available_width - MARGIN,
    };
    let y = match row {
        0 => work.top + MARGIN,
        1 => work.top + available_height / 2,
        _ => work.top + available_height - MARGIN,
    };
    popup.window().set_position(PhysicalPosition::new(x, y));
}

fn work_area() -> RECT {
    let mut area = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA, 0, (&raw mut area).cast(), 0) };
    if ok == 0 {
        RECT {
            left: 0,
            top: 0,
            right: 1920,
            bottom: 1080,
        }
    } else {
        area
    }
}
