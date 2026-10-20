//! Persistent notification-area icon. Same labels and clicks as Tauri `trayIcon`.
//! No taskbar button changes.

use crate::application::ui_thread::evt::UI_Evt;
use crate::application::ui_thread::inbox::UI_Inbox_post;

pub const TRAY_TOOLTIP: &str = "SunnyMatato";
pub const TRAY_SHOW_LABEL: &str = "显示";
pub const TRAY_QUIT_LABEL: &str = "退出";
pub const TRAY_MENU_SHOW: u16 = 1;
pub const TRAY_MENU_QUIT: u16 = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayAction {
    Show,
    Quit,
}

pub fn tray_mouse_action(msg: u32) -> Option<TrayAction> {
    const WM_LBUTTONUP: u32 = 514;
    if msg == WM_LBUTTONUP {
        Some(TrayAction::Show)
    } else {
        None
    }
}

pub fn tray_menu_action(id: u16) -> Option<TrayAction> {
    match id {
        TRAY_MENU_SHOW => Some(TrayAction::Show),
        TRAY_MENU_QUIT => Some(TrayAction::Quit),
        _ => None,
    }
}

pub fn post_tray_action(action: TrayAction) {
    match action {
        TrayAction::Show => UI_Inbox_post(UI_Evt::TrayShow),
        TrayAction::Quit => UI_Inbox_post(UI_Evt::TrayQuit),
    }
}

pub fn install(window: &gpui::Window) {
    #[cfg(target_os = "windows")]
    windows_impl::install(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

pub fn uninstall(window: &gpui::Window) {
    #[cfg(target_os = "windows")]
    windows_impl::uninstall(window);
    #[cfg(not(target_os = "windows"))]
    let _ = window;
}

#[cfg(target_os = "windows")]
mod windows_impl {
    use std::sync::atomic::{AtomicBool, Ordering};

    use gpui::Window;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::UI::Shell::{
        DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass, Shell_NotifyIconW, NIF_ICON,
        NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, NOTIFYICONDATAW_0,
    };
    use windows::Win32::UI::WindowsAndMessaging::{
        AppendMenuW, CreatePopupMenu, DestroyMenu, GetCursorPos, LoadIconW, PostMessageW,
        SetForegroundWindow, TrackPopupMenu, IDI_APPLICATION, MF_STRING, TPM_RETURNCMD,
        TPM_RIGHTBUTTON, WM_APP, WM_NCDESTROY, WM_RBUTTONUP,
    };

    use super::{
        post_tray_action, tray_menu_action, tray_mouse_action, TRAY_MENU_QUIT, TRAY_MENU_SHOW,
        TRAY_QUIT_LABEL, TRAY_SHOW_LABEL, TRAY_TOOLTIP,
    };

    const TRAY_CALLBACK: u32 = WM_APP + 0x20;
    const TRAY_SUBCLASS: usize = 0x534D_5452;
    const TRAY_ID: u32 = 1;

    static INSTALLED: AtomicBool = AtomicBool::new(false);

    fn hwnd_of(window: &Window) -> Option<HWND> {
        let handle = HasWindowHandle::window_handle(window).ok()?;
        match handle.as_raw() {
            RawWindowHandle::Win32(win32) => Some(HWND(win32.hwnd.get() as *mut _)),
            _ => None,
        }
    }

    fn wide(text: &str) -> Vec<u16> {
        text.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn nid(hwnd: HWND) -> NOTIFYICONDATAW {
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            uFlags: NIF_MESSAGE | NIF_ICON | NIF_TIP,
            uCallbackMessage: TRAY_CALLBACK,
            Anonymous: NOTIFYICONDATAW_0 { uVersion: 0 },
            ..Default::default()
        };
        if let Ok(icon) = unsafe { LoadIconW(None, IDI_APPLICATION) } {
            data.hIcon = icon;
        }
        let tip: Vec<u16> = TRAY_TOOLTIP.encode_utf16().chain(std::iter::once(0)).collect();
        for (dst, src) in data.szTip.iter_mut().zip(tip.iter().copied()) {
            *dst = src;
        }
        data
    }

    pub(super) fn install(window: &Window) {
        if INSTALLED.load(Ordering::SeqCst) {
            return;
        }
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        let data = nid(hwnd);
        if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
            return;
        }
        if !unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), TRAY_SUBCLASS, 0) }.as_bool() {
            let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
            return;
        }
        INSTALLED.store(true, Ordering::SeqCst);
    }

    pub(super) fn uninstall(window: &Window) {
        let Some(hwnd) = hwnd_of(window) else {
            return;
        };
        remove_icon(hwnd);
        let _ = unsafe { RemoveWindowSubclass(hwnd, Some(subclass_proc), TRAY_SUBCLASS) };
        INSTALLED.store(false, Ordering::SeqCst);
    }

    fn remove_icon(hwnd: HWND) {
        let data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: TRAY_ID,
            ..Default::default()
        };
        let _ = unsafe { Shell_NotifyIconW(NIM_DELETE, &data) };
    }

    fn popup_menu(hwnd: HWND) {
        let Ok(menu) = (unsafe { CreatePopupMenu() }) else {
            return;
        };
        let show = wide(TRAY_SHOW_LABEL);
        let quit = wide(TRAY_QUIT_LABEL);
        let _ = unsafe {
            AppendMenuW(
                menu,
                MF_STRING,
                TRAY_MENU_SHOW as usize,
                windows::core::PCWSTR(show.as_ptr()),
            )
        };
        let _ = unsafe {
            AppendMenuW(
                menu,
                MF_STRING,
                TRAY_MENU_QUIT as usize,
                windows::core::PCWSTR(quit.as_ptr()),
            )
        };
        let mut point = windows::Win32::Foundation::POINT::default();
        if unsafe { GetCursorPos(&mut point) }.is_err() {
            let _ = unsafe { DestroyMenu(menu) };
            return;
        }
        let _ = unsafe { SetForegroundWindow(hwnd) };
        let cmd = unsafe {
            TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON | TPM_RETURNCMD,
                point.x,
                point.y,
                None,
                hwnd,
                None,
            )
        };
        let _ = unsafe { PostMessageW(Some(hwnd), 0, WPARAM(0), LPARAM(0)) };
        let _ = unsafe { DestroyMenu(menu) };
        if let Some(action) = tray_menu_action(cmd.0 as u16) {
            post_tray_action(action);
        }
    }

    unsafe extern "system" fn subclass_proc(
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _uid: usize,
        _data: usize,
    ) -> LRESULT {
        if msg == TRAY_CALLBACK {
            let mouse = lparam.0 as u32;
            if mouse == WM_RBUTTONUP {
                popup_menu(hwnd);
            } else if let Some(action) = tray_mouse_action(mouse) {
                post_tray_action(action);
            }
            return LRESULT(0);
        }
        if msg == WM_NCDESTROY {
            remove_icon(hwnd);
            let _ = RemoveWindowSubclass(hwnd, Some(subclass_proc), TRAY_SUBCLASS);
            INSTALLED.store(false, Ordering::SeqCst);
        }
        unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tray_copy_matches_tauri() {
        assert_eq!(TRAY_TOOLTIP, "SunnyMatato");
        assert_eq!(TRAY_SHOW_LABEL, "显示");
        assert_eq!(TRAY_QUIT_LABEL, "退出");
    }

    #[test]
    fn left_click_shows_the_window() {
        assert_eq!(tray_mouse_action(514), Some(TrayAction::Show));
        assert_eq!(tray_mouse_action(517), None);
    }

    #[test]
    fn menu_items_show_or_quit() {
        assert_eq!(tray_menu_action(TRAY_MENU_SHOW), Some(TrayAction::Show));
        assert_eq!(tray_menu_action(TRAY_MENU_QUIT), Some(TrayAction::Quit));
        assert_eq!(tray_menu_action(99), None);
    }
}
