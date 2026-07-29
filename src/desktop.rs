use crate::config::NotificationPosition;
use std::mem::size_of;
use windows_sys::Win32::{
    Foundation::POINT,
    Graphics::Gdi::{GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint},
    UI::WindowsAndMessaging::{GetCursorPos, MB_ICONASTERISK, MessageBeep},
};

const EDGE_MARGIN: i32 = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct WorkArea {
    left: i32,
    top: i32,
    right: i32,
    bottom: i32,
}

pub fn play_notification_sound() {
    unsafe {
        MessageBeep(MB_ICONASTERISK as u32);
    }
}

pub fn popup_origin(width: i32, height: i32, position: NotificationPosition) -> (i32, i32) {
    origin_in_area(current_work_area(), width, height, position)
}

fn current_work_area() -> WorkArea {
    unsafe {
        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let monitor = MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST as u32);
        let mut info =
            MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        if !monitor.is_null() && GetMonitorInfoW(monitor, &mut info) != 0 {
            return WorkArea {
                left: info.rcWork.left,
                top: info.rcWork.top,
                right: info.rcWork.right,
                bottom: info.rcWork.bottom,
            };
        }
    }
    WorkArea { left: 0, top: 0, right: 1920, bottom: 1080 }
}

fn origin_in_area(
    area: WorkArea,
    popup_width: i32,
    popup_height: i32,
    position: NotificationPosition,
) -> (i32, i32) {
    let available_width = (area.right - area.left).max(popup_width);
    let available_height = (area.bottom - area.top).max(popup_height);

    let left = area.left + EDGE_MARGIN;
    let center_x = area.left + (available_width - popup_width) / 2;
    let right = area.right - popup_width - EDGE_MARGIN;
    let top = area.top + EDGE_MARGIN;
    let center_y = area.top + (available_height - popup_height) / 2;
    let bottom = area.bottom - popup_height - EDGE_MARGIN;

    let (x, y) = match position {
        NotificationPosition::TopLeft => (left, top),
        NotificationPosition::TopCenter => (center_x, top),
        NotificationPosition::TopRight => (right, top),
        NotificationPosition::MiddleLeft => (left, center_y),
        NotificationPosition::Center => (center_x, center_y),
        NotificationPosition::MiddleRight => (right, center_y),
        NotificationPosition::BottomLeft => (left, bottom),
        NotificationPosition::BottomCenter => (center_x, bottom),
        NotificationPosition::BottomRight => (right, bottom),
    };

    let max_x = (area.right - popup_width).max(area.left);
    let max_y = (area.bottom - popup_height).max(area.top);
    (x.clamp(area.left, max_x), y.clamp(area.top, max_y))
}

#[cfg(test)]
mod tests {
    use super::{NotificationPosition, WorkArea, origin_in_area};

    const AREA: WorkArea = WorkArea { left: 100, top: 50, right: 1100, bottom: 850 };

    #[test]
    fn places_popup_at_bottom_right_with_margin() {
        assert_eq!(origin_in_area(AREA, 300, 100, NotificationPosition::BottomRight), (784, 734));
    }

    #[test]
    fn places_popup_in_center() {
        assert_eq!(origin_in_area(AREA, 300, 100, NotificationPosition::Center), (450, 400));
    }
}
