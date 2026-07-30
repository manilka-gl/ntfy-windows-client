use crate::{NotificationPopup, audio, memory};
use slint::{ComponentHandle, PhysicalPosition, Timer, TimerMode};
use std::{cell::RefCell, rc::Rc, time::Duration};
use windows_sys::Win32::{
    Foundation::RECT,
    UI::WindowsAndMessaging::{SPI_GETWORKAREA, SystemParametersInfoW},
};

const MARGIN: i32 = 16;
const BASE_DISPLAY_TIME_MS: u64 = 4200;
const DISPLAY_TIME_PER_LINE_MS: u64 = 420;
const MAX_DISPLAY_TIME_MS: u64 = 9000;
const POPUP_LOGICAL_WIDTH: f32 = 380.0;
const POPUP_MIN_LOGICAL_HEIGHT: f32 = 112.0;
const POPUP_MAX_LOGICAL_HEIGHT: f32 = 248.0;
const POPUP_FIXED_LOGICAL_HEIGHT: f32 = 78.0;
const POPUP_BODY_LINE_HEIGHT: f32 = 17.0;
const POPUP_BODY_COLUMNS: usize = 48;
const POPUP_MAX_BODY_LINES: usize = 10;
const MIN_VALID_POPUP_EDGE: u32 = 64;

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
                let slot_for_dismiss = Rc::downgrade(&self.popup);
                popup.on_dismiss(move || {
                    if let Some(slot) = slot_for_dismiss.upgrade()
                        && let Some(popup) = slot.borrow_mut().take()
                    {
                        let _ = popup.hide();
                    }
                    memory::trim_working_set();
                });
                *slot = Some(popup);
            }
            slot.as_ref().expect("popup initialized").clone_strong()
        };

        let body_lines = visual_line_count(body, POPUP_BODY_COLUMNS).min(POPUP_MAX_BODY_LINES);
        popup.set_popup_height_px(popup_logical_height_for_lines(body_lines));
        popup.set_popup_title(title.into());
        popup.set_popup_body(body.into());
        popup.set_popup_meta(meta.into());
        if popup.show().is_err() {
            return;
        }

        // Position immediately using a reliable logical-size fallback, then
        // repeat on the next event-loop turn after winit reports final pixels.
        position_popup(&popup, placement);
        let popup_for_position = popup.clone_strong();
        Timer::single_shot(Duration::ZERO, move || {
            position_popup(&popup_for_position, placement);
            popup_for_position.window().request_redraw();
        });

        if play_sound {
            self.play_sound(output_name);
        }

        let display_time = display_time_for_lines(body_lines);
        let slot = Rc::clone(&self.popup);
        self.timer
            .start(TimerMode::SingleShot, display_time, move || {
                if let Some(popup) = slot.borrow_mut().take() {
                    let _ = popup.hide();
                }
                memory::trim_working_set();
            });
    }
}

fn visual_line_count(text: &str, columns: usize) -> usize {
    let columns = columns.max(1);
    text.lines()
        .map(|line| {
            let characters = line.chars().count().max(1);
            characters.div_ceil(columns)
        })
        .sum::<usize>()
        .max(1)
}

fn popup_logical_height_for_lines(body_lines: usize) -> f32 {
    let body_lines = body_lines.clamp(1, POPUP_MAX_BODY_LINES) as f32;
    (POPUP_FIXED_LOGICAL_HEIGHT + body_lines * POPUP_BODY_LINE_HEIGHT)
        .clamp(POPUP_MIN_LOGICAL_HEIGHT, POPUP_MAX_LOGICAL_HEIGHT)
}

fn display_time_for_lines(body_lines: usize) -> Duration {
    let milliseconds = BASE_DISPLAY_TIME_MS
        .saturating_add((body_lines as u64).saturating_mul(DISPLAY_TIME_PER_LINE_MS))
        .min(MAX_DISPLAY_TIME_MS);
    Duration::from_millis(milliseconds)
}

fn position_popup(popup: &NotificationPopup, placement: i32) {
    let work = work_area();
    let measured = popup.window().size();
    let scale = popup.window().scale_factor().max(1.0);
    let width = physical_edge(measured.width, POPUP_LOGICAL_WIDTH, scale);
    let height = physical_edge(measured.height, popup.get_popup_height_px(), scale);

    let min_x = work.left.saturating_add(MARGIN);
    let max_x = work
        .right
        .saturating_sub(width)
        .saturating_sub(MARGIN)
        .max(min_x);
    let min_y = work.top.saturating_add(MARGIN);
    let max_y = work
        .bottom
        .saturating_sub(height)
        .saturating_sub(MARGIN)
        .max(min_y);

    let placement = placement.clamp(0, 8);
    let column = placement % 3;
    let row = placement / 3;

    let x = match column {
        0 => min_x,
        1 => min_x + (max_x - min_x) / 2,
        _ => max_x,
    };
    let y = match row {
        0 => min_y,
        1 => min_y + (max_y - min_y) / 2,
        _ => max_y,
    };
    popup.window().set_position(PhysicalPosition::new(x, y));
}

fn physical_edge(measured: u32, logical: f32, scale: f32) -> i32 {
    let pixels = if measured >= MIN_VALID_POPUP_EDGE {
        measured
    } else {
        (logical * scale).ceil().max(1.0) as u32
    };
    i32::try_from(pixels).unwrap_or(i32::MAX)
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

#[cfg(test)]
mod tests {
    use super::{
        POPUP_MAX_LOGICAL_HEIGHT, POPUP_MIN_LOGICAL_HEIGHT, display_time_for_lines, physical_edge,
        popup_logical_height_for_lines, visual_line_count,
    };

    #[test]
    fn popup_size_falls_back_before_window_metrics_are_ready() {
        assert_eq!(physical_edge(0, 380.0, 1.0), 380);
        assert_eq!(physical_edge(0, 380.0, 1.5), 570);
        assert_eq!(physical_edge(500, 380.0, 1.5), 500);
    }

    #[test]
    fn popup_height_grows_with_wrapped_message_lines() {
        let short = popup_logical_height_for_lines(1);
        let medium = popup_logical_height_for_lines(5);
        let long = popup_logical_height_for_lines(20);

        assert_eq!(short, POPUP_MIN_LOGICAL_HEIGHT);
        assert!(medium > short);
        assert_eq!(long, POPUP_MAX_LOGICAL_HEIGHT);
    }

    #[test]
    fn explicit_and_wrapped_lines_are_counted() {
        assert_eq!(visual_line_count("one line", 48), 1);
        assert_eq!(visual_line_count("first\nsecond", 48), 2);
        assert_eq!(visual_line_count(&"x".repeat(97), 48), 3);
    }

    #[test]
    fn longer_notifications_remain_visible_longer() {
        assert!(display_time_for_lines(8) > display_time_for_lines(1));
    }
}
