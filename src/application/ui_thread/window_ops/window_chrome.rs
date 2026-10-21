//! HWND geometry for pin and top-edge dock.

use gpui::{App, Window};

use super::edge_dock::{Point, Rect};

pub fn window_rect(window: &Window) -> Rect {
    #[cfg(target_os = "windows")]
    if let Some(rect) = windows_impl::window_rect(window) {
        return rect;
    }
    let bounds = window.bounds();
    Rect {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        width: f32::from(bounds.size.width),
        height: f32::from(bounds.size.height),
    }
}

pub fn work_area(window: &Window, cx: &App) -> Option<Rect> {
    #[cfg(target_os = "windows")]
    {
        let _ = cx;
        return windows_impl::work_area(window);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let bounds = window.display(cx)?.bounds();
        Some(Rect {
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
        })
    }
}

pub fn cursor_point(window: &Window) -> Option<Point> {
    #[cfg(target_os = "windows")]
    {
        return windows_impl::cursor_point(window);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        None
    }
}

pub fn set_position(window: &Window, x: f32, y: f32) {
    #[cfg(target_os = "windows")]
    windows_impl::set_position(window, x, y);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, x, y);
}

pub fn is_minimized(window: &Window) -> bool {
    #[cfg(target_os = "windows")]
    {
        return windows_impl::is_minimized(window);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        false
    }
}

pub fn show(window: &Window) {
    #[cfg(target_os = "windows")]
    windows_impl::show(window, false);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn restore(window: &Window) {
    #[cfg(target_os = "windows")]
    windows_impl::restore(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn hide(window: &Window) {
    #[cfg(target_os = "windows")]
    windows_impl::hide(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

#[derive(Clone, Copy)]
pub struct SlideFrame {
    pub x: i32,
    pub rest_y: i32,
    pub height: i32,
}

pub fn begin_slide_hide(window: &Window) -> Option<SlideFrame> {
    #[cfg(target_os = "windows")]
    {
        return windows_impl::begin_slide_hide(window);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = window;
        None
    }
}

pub fn set_slide_y(window: &Window, x: i32, y: i32) {
    #[cfg(target_os = "windows")]
    windows_impl::set_slide_y(window, x, y);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, x, y);
}

pub fn finish_slide_hide(window: &Window, frame: &SlideFrame) {
    #[cfg(target_os = "windows")]
    windows_impl::finish_slide_hide(window, frame);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, frame);
}

pub fn begin_slide_show(window: &Window, frame: &SlideFrame) -> bool {
    #[cfg(target_os = "windows")]
    {
        return windows_impl::begin_slide_show(window, frame);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = (window, frame);
        false
    }
}

pub fn finish_slide_show(window: &Window) {
    #[cfg(target_os = "windows")]
    windows_impl::show(window, true);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, POINT, RECT};
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        GetCursorPos, GetWindowRect, IsIconic, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
        SWP_NOSIZE, SWP_NOZORDER, SW_HIDE, SW_RESTORE, SW_SHOW, SW_SHOWNOACTIVATE,
    };

    use super::{Point, Rect, SlideFrame};

    pub(super) fn hwnd_of(window: &Window) -> Option<HWND> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as *mut _)),
            _ => None,
        }
    }

    fn scale(window: &Window) -> f32 {
        window.scale_factor().max(0.1)
    }

    fn rect_from_physical(rect: RECT, scale: f32) -> Rect {
        Rect {
            x: rect.left as f32 / scale,
            y: rect.top as f32 / scale,
            width: (rect.right - rect.left) as f32 / scale,
            height: (rect.bottom - rect.top) as f32 / scale,
        }
    }

    pub(super) fn window_rect(window: &Window) -> Option<Rect> {
        let hwnd = hwnd_of(window)?;
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
        Some(rect_from_physical(rect, scale(window)))
    }

    pub(super) fn work_area(window: &Window) -> Option<Rect> {
        let hwnd = hwnd_of(window)?;
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            rcMonitor: RECT::default(),
            rcWork: RECT::default(),
            dwFlags: 0,
        };
        if !unsafe { GetMonitorInfoW(monitor, &mut info) }.as_bool() {
            return None;
        }
        Some(rect_from_physical(info.rcWork, scale(window)))
    }

    pub(super) fn cursor_point(window: &Window) -> Option<Point> {
        let mut point = POINT::default();
        unsafe { GetCursorPos(&mut point) }.ok()?;
        let scale = scale(window);
        Some(Point {
            x: point.x as f32 / scale,
            y: point.y as f32 / scale,
        })
    }

    pub(super) fn set_position(window: &Window, x: f32, y: f32) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        let scale = scale(window);
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                (x * scale).round() as i32,
                (y * scale).round() as i32,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    pub(super) fn is_minimized(window: &Window) -> bool {
        hwnd_of(window).is_some_and(|hwnd| unsafe { IsIconic(hwnd) }.as_bool())
    }

    pub(super) fn show(window: &Window, activate: bool) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        let cmd = if activate { SW_SHOW } else { SW_SHOWNOACTIVATE };
        let _ = unsafe { ShowWindow(hwnd, cmd) };
    }

    pub(super) fn restore(window: &Window) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        let _ = unsafe { ShowWindow(hwnd, SW_RESTORE) };
        let _ = unsafe { ShowWindow(hwnd, SW_SHOW) };
    }

    pub(super) fn hide(window: &Window) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    }

    pub(super) fn begin_slide_hide(window: &Window) -> Option<SlideFrame> {
        let hwnd = hwnd_of(window)?;
        let mut rect = RECT::default();
        unsafe { GetWindowRect(hwnd, &mut rect) }.ok()?;
        Some(SlideFrame {
            x: rect.left,
            rest_y: rect.top,
            height: (rect.bottom - rect.top).max(1),
        })
    }

    pub(super) fn set_slide_y(window: &Window, x: i32, y: i32) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        let _ = unsafe {
            SetWindowPos(
                hwnd,
                None,
                x,
                y,
                0,
                0,
                SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
            )
        };
    }

    pub(super) fn finish_slide_hide(window: &Window, frame: &SlideFrame) {
        set_slide_y(
            window,
            frame.x,
            super::super::edge_dock::hide_finish_y(frame.rest_y, frame.height, false),
        );
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        let _ = unsafe { ShowWindow(hwnd, SW_HIDE) };
    }

    pub(super) fn begin_slide_show(window: &Window, frame: &SlideFrame) -> bool {
        let start_y = super::super::edge_dock::hide_finish_y(frame.rest_y, frame.height, false);
        set_slide_y(window, frame.x, start_y);
        let Some(hwnd) = hwnd_of(window) else {
            return false;
        };
        let _ = unsafe { ShowWindow(hwnd, SW_SHOWNOACTIVATE) };
        true
    }
}
