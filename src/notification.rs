use crate::NotificationPopup;
use slint::{ComponentHandle, PhysicalPosition, Timer, TimerMode};
use std::time::Duration;
use windows_sys::Win32::{
    Foundation::RECT,
    UI::WindowsAndMessaging::{MB_ICONASTERISK, SPI_GETWORKAREA, SystemParametersInfoW},
};

#[link(name = "user32")]
unsafe extern "system" {
    fn MessageBeep(u_type: u32) -> i32;
}

const MARGIN: i32 = 14;
const DISPLAY_TIME: Duration = Duration::from_millis(6500);

pub struct Presenter {
    popup: NotificationPopup,
    timer: Timer,
}

impl Presenter {
    pub fn new() -> Result<Self, slint::PlatformError> {
        let popup = NotificationPopup::new()?;
        let weak = popup.as_weak();
        popup.on_dismiss(move || {
            if let Some(popup) = weak.upgrade() {
                let _ = popup.hide();
            }
        });
        Ok(Self {
            popup,
            timer: Timer::default(),
        })
    }

    pub fn play_sound(&self) {
        unsafe {
            MessageBeep(MB_ICONASTERISK as u32);
        }
    }

    pub fn show(&self, title: &str, body: &str, meta: &str, placement: i32, play_sound: bool) {
        self.popup.set_popup_title(title.into());
        self.popup.set_popup_body(body.into());
        self.popup.set_popup_meta(meta.into());
        if self.popup.show().is_err() {
            return;
        }
        self.position(placement);
        if play_sound {
            self.play_sound();
        }

        let weak = self.popup.as_weak();
        self.timer
            .start(TimerMode::SingleShot, DISPLAY_TIME, move || {
                if let Some(popup) = weak.upgrade() {
                    let _ = popup.hide();
                }
            });
    }

    fn position(&self, placement: i32) {
        let work = work_area();
        let size = self.popup.window().size();
        let width = i32::try_from(size.width).unwrap_or(i32::MAX);
        let height = i32::try_from(size.height).unwrap_or(i32::MAX);
        let available_width = (work.right - work.left - width).max(0);
        let available_height = (work.bottom - work.top - height).max(0);
        let column = placement.clamp(0, 8) % 3;
        let row = placement.clamp(0, 8) / 3;

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
        self.popup
            .window()
            .set_position(PhysicalPosition::new(x, y));
    }
}

fn work_area() -> RECT {
    let mut area = RECT {
        left: 0,
        top: 0,
        right: 1920,
        bottom: 1080,
    };
    let ok = unsafe { SystemParametersInfoW(SPI_GETWORKAREA as u32, 0, (&raw mut area).cast(), 0) };
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
