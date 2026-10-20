//! Window-event bypass on the GUI thread. Borrows SM_UI; owns no controls.

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

pub struct WindowOps;

impl WindowOps {
    /// Consume window requests; leave other events for HSM dispatch.
    pub fn dispatch(
        sm: &mut SM_UI,
        event: &UI_Evt,
        window: &mut Window,
        cx: &mut Context<SM_UI>,
    ) -> DispatchResult {
        match event {
            UI_Evt::TrayShow => sm.reveal_from_tray(window, cx),
            UI_Evt::HideToTray => {
                sm.cancel_drag();
                sm.post_pending();
                sm.hide_to_tray(window);
            }
            UI_Evt::TogglePin => {
                sm.toggle_pin();
                sm.apply_always_on_top(window);
            }
            UI_Evt::TrayQuit => sm.begin_close(),
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
            sm.reveal_from_tray(window, cx);
        }
        if sm.ready_to_close() {
            ExitProgress::ExitReady
        } else {
            ExitProgress::Continue
        }
    }
}
