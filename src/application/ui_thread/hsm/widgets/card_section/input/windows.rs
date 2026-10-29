//! Native capture loss is deferred with its generation to avoid HSM reentry.

use super::CaptureToken;
use crate::application::ui_thread::thread::{evt::UI_Evt, inbox::UI_Inbox_post};
use ::windows::Win32::{
    Foundation::{HWND, LPARAM, LRESULT, WPARAM},
    UI::{
        Input::KeyboardAndMouse::{GetCapture, ReleaseCapture},
        Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass},
        WindowsAndMessaging::{WM_CANCELMODE, WM_CAPTURECHANGED, WM_NCDESTROY},
    },
};
use gpui::Window;
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::atomic::{AtomicU64, Ordering};

const SUBCLASS: usize = 0x534D_4341;
static ACTIVE_GENERATION: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy, Default)]
pub(super) struct NativeCapture {
    hwnd: HWND,
}
impl NativeCapture {
    pub fn attach(window: &Window) -> Self {
        let RawWindowHandle::Win32(handle) = HasWindowHandle::window_handle(window)
            .expect("window handle")
            .as_raw()
        else {
            unreachable!("Windows window handle");
        };
        let hwnd = HWND(handle.hwnd.get() as *mut _);
        unsafe { SetWindowSubclass(hwnd, Some(capture_proc), SUBCLASS, 0) }
            .ok()
            .expect("install capture-loss observer");
        Self { hwnd }
    }
    pub fn detach(self) {
        ACTIVE_GENERATION.store(0, Ordering::Release);
        unsafe {
            let _ = RemoveWindowSubclass(self.hwnd, Some(capture_proc), SUBCLASS);
        }
    }
    pub fn captured(self) -> bool {
        !self.hwnd.0.is_null() && (unsafe { GetCapture() }) == self.hwnd
    }
    pub fn arm(self, token: CaptureToken) {
        ACTIVE_GENERATION.store(token.0, Ordering::Release);
    }
    pub fn active(self, token: CaptureToken) -> bool {
        ACTIVE_GENERATION.load(Ordering::Acquire) == token.0
    }
    pub fn release(self, token: CaptureToken) {
        if ACTIVE_GENERATION
            .compare_exchange(token.0, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
            && self.captured()
        {
            let _ = unsafe { ReleaseCapture() };
        }
    }
}

unsafe extern "system" fn capture_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _: usize,
    _: usize,
) -> LRESULT {
    if (msg == WM_CAPTURECHANGED && lparam.0 != hwnd.0 as isize) || msg == WM_CANCELMODE {
        let generation = ACTIVE_GENERATION.swap(0, Ordering::AcqRel);
        if generation != 0 {
            UI_Inbox_post(UI_Evt::ColumnCaptureLost { generation });
        }
    }
    if msg == WM_NCDESTROY {
        ACTIVE_GENERATION.store(0, Ordering::Release);
        unsafe {
            let _ = RemoveWindowSubclass(hwnd, Some(capture_proc), SUBCLASS);
        }
    }
    unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) }
}
