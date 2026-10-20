//! Hide the window from the taskbar. Matches Tauri `skipTaskbar`.

use gpui::Window;

#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    WINDOW_EX_STYLE, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW,
};

#[cfg(target_os = "windows")]
pub fn skip_taskbar_ex_style(ex: WINDOW_EX_STYLE) -> WINDOW_EX_STYLE {
    (ex & !WS_EX_APPWINDOW) | WS_EX_TOOLWINDOW
}

pub fn set_skip_taskbar(window: &Window) {
    #[cfg(target_os = "windows")]
    windows_impl::apply(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_SERVER};
    use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};
    use windows::Win32::UI::WindowsAndMessaging::{
        GetWindowLongPtrW, SetWindowLongPtrW, SetWindowPos, GWL_EXSTYLE, SWP_FRAMECHANGED,
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER, WINDOW_EX_STYLE,
    };

    use super::skip_taskbar_ex_style;

    fn hwnd_of(window: &Window) -> Option<HWND> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as *mut _)),
            _ => None,
        }
    }

    pub(super) fn apply(window: &Window) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        let current = WINDOW_EX_STYLE(unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32);
        let next = skip_taskbar_ex_style(current);
        unsafe {
            SetWindowLongPtrW(hwnd, GWL_EXSTYLE, next.0 as isize);
            let _ = SetWindowPos(
                hwnd,
                None,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
            );
        }
        if let Ok(taskbar) =
            unsafe { CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_SERVER) }
        {
            let _ = unsafe { taskbar.HrInit() };
            let _ = unsafe { taskbar.DeleteTab(hwnd) };
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::skip_taskbar_ex_style;
    use windows::Win32::UI::WindowsAndMessaging::{
        WS_EX_APPWINDOW, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    };

    #[test]
    fn skipping_the_taskbar_drops_appwindow_and_uses_toolwindow() {
        let next = skip_taskbar_ex_style(WS_EX_APPWINDOW | WS_EX_NOREDIRECTIONBITMAP | WS_EX_TOPMOST);
        assert!(!next.contains(WS_EX_APPWINDOW));
        assert!(next.contains(WS_EX_TOOLWINDOW));
        assert!(next.contains(WS_EX_TOPMOST));
        assert!(next.contains(WS_EX_NOREDIRECTIONBITMAP));
    }
}
