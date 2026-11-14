//! Opening the picker next to the pointer. Wayland can't place windows, so Linux leaves
//! placement to the window manager.

use rune::AppHandle;

const CURSOR_OFFSET: f32 = 8.0;
const SCREEN_MARGIN: f32 = 8.0;

/// The window's top-left, in physical desktop pixels, for a window created at the pointer.
pub(super) fn initial_position(size: (f32, f32)) -> Option<(i32, i32)> {
    #[cfg(target_os = "windows")]
    {
        windows::position_near_cursor(size)
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = size;
        None
    }
}

/// Moves an existing (possibly hidden) window next to the pointer.
pub(super) fn place_at_cursor<S: 'static>(handle: &AppHandle<S>, size: (f32, f32)) {
    #[cfg(target_os = "windows")]
    if let Some(position) = windows::position_near_cursor(size) {
        handle.set_position(position);
    }

    #[cfg(target_os = "macos")]
    if let Some((x, y)) = macos::origin_near_cursor(size) {
        handle.place(
            rune::DisplayTarget::Primary,
            rune::OverlayPlacement::Rect {
                x,
                y,
                width: size.0,
                height: size.1,
            },
        );
    }

    #[cfg(target_os = "linux")]
    let _ = (handle, size);
}

/// Puts a `size` box beside `cursor` inside `area`, flipping to the other side of the pointer
/// when it would cross the area's right or bottom edge.
fn origin_near(cursor: (f32, f32), area: (f32, f32, f32, f32), size: (f32, f32), scale: f32) -> (f32, f32) {
    let (left, top, right, bottom) = area;
    let offset = CURSOR_OFFSET * scale;
    let margin = SCREEN_MARGIN * scale;

    let mut x = cursor.0 + offset;
    if x + size.0 > right - margin {
        x = cursor.0 - size.0 - offset;
    }
    let mut y = cursor.1 + offset;
    if y + size.1 > bottom - margin {
        y = cursor.1 - size.1 - offset;
    }

    (
        x.clamp(left + margin, (right - size.0 - margin).max(left + margin)),
        y.clamp(top + margin, (bottom - size.1 - margin).max(top + margin)),
    )
}

#[cfg(target_os = "windows")]
mod windows {
    use std::mem;

    use winapi::shared::windef::POINT;
    use winapi::um::shellscalingapi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
    use winapi::um::winuser::{
        GetCursorPos, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
    };

    pub(super) fn position_near_cursor(size: (f32, f32)) -> Option<(i32, i32)> {
        let mut cursor = POINT { x: 0, y: 0 };
        if unsafe { GetCursorPos(&mut cursor) } == 0 {
            return None;
        }

        let monitor = unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) };
        let mut info: MONITORINFO = unsafe { mem::zeroed() };
        info.cbSize = mem::size_of::<MONITORINFO>() as u32;
        if unsafe { GetMonitorInfoW(monitor, &mut info) } == 0 {
            return None;
        }

        let (mut dpi_x, mut dpi_y) = (96, 96);
        unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };
        let scale = dpi_x as f32 / 96.0;

        let work = info.rcWork;
        let (x, y) = super::origin_near(
            (cursor.x as f32, cursor.y as f32),
            (work.left as f32, work.top as f32, work.right as f32, work.bottom as f32),
            (size.0 * scale, size.1 * scale),
            scale,
        );
        Some((x.round() as i32, y.round() as i32))
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSEvent, NSScreen};

    /// Top-left in points from the primary display's top-left, as `OverlayPlacement::Rect`
    /// takes it. AppKit's global coordinates grow upwards from the primary display's bottom.
    pub(super) fn origin_near_cursor(size: (f32, f32)) -> Option<(f32, f32)> {
        let main_thread = MainThreadMarker::new()?;
        let screens = NSScreen::screens(main_thread);
        let primary_height = screens.firstObject()?.frame().size.height;
        let cursor = NSEvent::mouseLocation();

        let screen = screens.iter().find(|screen| {
            let frame = screen.frame();
            cursor.x >= frame.origin.x
                && cursor.x < frame.origin.x + frame.size.width
                && cursor.y >= frame.origin.y
                && cursor.y < frame.origin.y + frame.size.height
        })?;
        let visible = screen.visibleFrame();

        let flip = |y: f64| (primary_height - y) as f32;
        Some(super::origin_near(
            (cursor.x as f32, flip(cursor.y)),
            (
                visible.origin.x as f32,
                flip(visible.origin.y + visible.size.height),
                (visible.origin.x + visible.size.width) as f32,
                flip(visible.origin.y),
            ),
            size,
            1.0,
        ))
    }
}
