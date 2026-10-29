//! Window behavior and state on the GUI thread.
//! SM_UI is the GPUI callback host; its controls only supply interaction/save state.

mod edge_dock;
mod state_machine;
mod top_edge_dock;
mod tray_icon;
mod window_chrome;
mod window_pin;
mod window_taskbar;

use gpui::{Context, Window};

use super::thread::evt::UI_Evt;
use crate::application::ui_thread::hsm::SM_UI;

#[derive(Debug, PartialEq, Eq)]
pub enum DispatchResult {
    Handled,
    Unhandled,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ExitProgress {
    Continue,
    ExitReady,
}

pub struct WindowOps {
    pinned: bool,
    top_dock: top_edge_dock::TopEdgeDock,
    hold_open: bool,
}

impl WindowOps {
    pub fn new() -> Self {
        Self {
            pinned: true,
            top_dock: top_edge_dock::TopEdgeDock::new(),
            hold_open: false,
        }
    }

    pub fn is_pinned(&self) -> bool {
        self.pinned
    }

    fn toggle_pin(&mut self) -> bool {
        self.pinned = !self.pinned;
        self.pinned
    }

    fn apply_always_on_top(&self, window: &Window) {
        window_pin::set_always_on_top(window, self.pinned);
    }

    fn apply_skip_taskbar(&self, window: &Window) {
        window_taskbar::set_skip_taskbar(window);
    }

    pub fn attach(&mut self, window: &mut Window, cx: &mut Context<SM_UI>) {
        self.apply_always_on_top(window);
        self.apply_skip_taskbar(window);
        window.on_window_should_close(cx, |_, _| {
            super::thread::inbox::UI_Inbox_post(UI_Evt::TrayQuit);
            false
        });
        cx.observe_window_activation(window, |this, window, _cx| {
            if window.is_window_active() {
                this.window_ops.apply_skip_taskbar(window);
            }
        })
        .detach();
    }

    /// Synchronize interaction conditions before rendering; dock/pin state stays here.
    pub fn sync(&mut self, hold_open: bool, window: &mut Window, cx: &mut Context<SM_UI>) {
        self.hold_open = hold_open;
        self.wire_top_edge_dock(window, cx);
        self.sync_window_hover(window, cx);
        self.sync_dock_hold_open(window, cx);
        tray_icon::install(window);
    }

    pub fn detach(window: &Window) {
        tray_icon::uninstall(window);
    }

    /// Consume window requests; leave other events for HSM dispatch.
    pub fn dispatch(
        sm: &mut SM_UI,
        event: &UI_Evt,
        window: &mut Window,
        cx: &mut Context<SM_UI>,
    ) -> DispatchResult {
        match event {
            UI_Evt::Window(event) => {
                sm.window_ops.hold_open = sm.dock_hold_open();
                sm.window_ops.dispatch_event(*event, window, cx);
            }
            UI_Evt::TrayShow => sm.window_ops.reveal_from_tray(window, cx),
            UI_Evt::HideToTray => {
                sm.cancel_scrollbar_drag(cx);
                sm.cancel_drag();
                sm.post_pending();
                sm.window_ops.hide_to_tray(window);
            }
            UI_Evt::TogglePin => {
                sm.window_ops.toggle_pin();
                sm.window_ops.apply_always_on_top(window);
            }
            UI_Evt::TrayQuit => {
                sm.cancel_scrollbar_drag(cx);
                sm.begin_close();
            }
            _ => return DispatchResult::Unhandled,
        }
        DispatchResult::Handled
    }

    /// Revisit exit after the event batch, including any persistence receipts.
    /// GUI teardown remains the runtime's responsibility.
    pub fn advance_exit(
        sm: &mut SM_UI,
        window: &mut Window,
        cx: &mut Context<SM_UI>,
    ) -> ExitProgress {
        if sm.close_failed() {
            sm.window_ops.reveal_from_tray(window, cx);
        }
        if sm.ready_to_close() {
            ExitProgress::ExitReady
        } else {
            ExitProgress::Continue
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_starts_pinned() {
        assert!(WindowOps::new().is_pinned());
    }

    #[test]
    fn toggle_pin_flips_always_on_top_flag() {
        let mut ops = WindowOps::new();
        assert!(!ops.toggle_pin());
        assert!(!ops.is_pinned());
        assert!(ops.toggle_pin());
        assert!(ops.is_pinned());
    }
}
