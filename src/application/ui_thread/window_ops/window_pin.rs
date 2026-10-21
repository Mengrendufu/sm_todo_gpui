//! OS always-on-top for the pin control. Cards stay out of this.

use gpui::Window;

pub fn set_always_on_top(window: &Window, on: bool) {
    #[cfg(target_os = "windows")]
    windows_impl::apply(window, on);
    #[cfg(not(target_os = "windows"))]
    let _ = (window, on);
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };

    pub(super) fn insert_after(on: bool) -> HWND {
        if on {
            HWND_TOPMOST
        } else {
            HWND_NOTOPMOST
        }
    }

    pub(super) fn apply(window: &Window, on: bool) {
        let Ok(handle) = HasWindowHandle::window_handle(window) else {
            return;
        };
        let RawWindowHandle::Win32(win32) = handle.as_raw() else {
            return;
        };
        let hwnd = HWND(win32.hwnd.get() as *mut _);
        unsafe {
            let _ = SetWindowPos(
                hwnd,
                Some(insert_after(on)),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
}

#[cfg(all(test, target_os = "windows"))]
mod tests {
    use super::windows_impl::insert_after;
    use windows::Win32::UI::WindowsAndMessaging::{HWND_NOTOPMOST, HWND_TOPMOST};

    #[test]
    fn pin_on_inserts_after_topmost() {
        assert_eq!(insert_after(true), HWND_TOPMOST);
        assert_eq!(insert_after(false), HWND_NOTOPMOST);
    }
}
