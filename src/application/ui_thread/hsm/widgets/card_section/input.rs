//! One logical pointer-capture owner per GUI window. No per-column broadcast.

use super::{
    state_machine::{CaptureToken, ColumnEvent},
    view::ColumnView,
};
use crate::application::snapshot::Status;
use crate::application::ui_thread::hsm::status_key;
use crate::application::ui_thread::thread::{evt::UI_Evt, inbox::UI_Inbox_post};
use gpui::{
    canvas, prelude::*, App, DispatchPhase, MouseButton, MouseMoveEvent, MouseUpEvent, WeakEntity,
    Window,
};
use std::{cell::RefCell, rc::Rc};

#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
use windows::NativeCapture;
#[cfg(not(target_os = "windows"))]
#[derive(Clone, Copy, Default)]
struct NativeCapture;
#[cfg(not(target_os = "windows"))]
impl NativeCapture {
    fn attach(_: &Window) -> Self {
        Self
    }
    fn detach(self) {}
    fn captured(self) -> bool {
        true
    }
    fn arm(self, _: CaptureToken) {}
    fn active(self, _: CaptureToken) -> bool {
        true
    }
    fn release(self, _: CaptureToken) {}
}

#[derive(Default)]
struct CaptureLease {
    owner: Option<Status>,
    generation: u64,
}
impl CaptureLease {
    fn claim(&mut self, owner: Status) -> Option<CaptureToken> {
        match self.owner {
            Some(current) if current != owner => None,
            Some(_) => Some(CaptureToken(self.generation)),
            None => {
                self.generation = self.generation.wrapping_add(1).max(1);
                self.owner = Some(owner);
                Some(CaptureToken(self.generation))
            }
        }
    }
    fn release(&mut self, token: CaptureToken) -> bool {
        if self.owner.is_some() && token.0 == self.generation {
            self.owner = None;
            true
        } else {
            false
        }
    }
}

#[derive(Default)]
struct InputState {
    lease: CaptureLease,
    targets: [Option<WeakEntity<ColumnView>>; 3],
    native: NativeCapture,
}

#[derive(Clone, Default)]
pub(in crate::application::ui_thread) struct ColumnInput(Rc<RefCell<InputState>>);

impl ColumnInput {
    pub(super) fn register(&self, status: Status, target: WeakEntity<ColumnView>) {
        self.0.borrow_mut().targets[status_key(status) as usize] = Some(target);
    }
    pub fn attach(&self, window: &Window) {
        self.0.borrow_mut().native = NativeCapture::attach(window);
    }
    pub fn detach(&self) {
        let native = std::mem::take(&mut self.0.borrow_mut().native);
        native.detach();
    }
    pub fn active(&self) -> bool {
        self.0.borrow().lease.owner.is_some()
    }
    pub(super) fn accepts(&self, status: Status) -> bool {
        self.0
            .borrow()
            .lease
            .owner
            .is_none_or(|owner| owner == status)
    }
    pub(super) fn claim(&self, status: Status) -> Option<CaptureToken> {
        let mut state = self.0.borrow_mut();
        if !state.native.captured() {
            return None;
        }
        let token = state.lease.claim(status)?;
        state.native.arm(token);
        Some(token)
    }
    pub(super) fn release(&self, token: CaptureToken) -> bool {
        let (native, released) = {
            let mut state = self.0.borrow_mut();
            let released = state.lease.release(token);
            (state.native, released)
        };
        native.release(token);
        released
    }
    /// Reserve a destination at input time; HSM processing belongs to the router.
    pub(super) fn post(&self, status: Status, token: CaptureToken, event: ColumnEvent) {
        UI_Inbox_post(UI_Evt::Column {
            status,
            token,
            event,
        });
    }
    pub(super) fn post_current(&self, event: ColumnEvent) -> bool {
        let mut state = self.0.borrow_mut();
        let Some(status) = state.lease.owner else {
            return false;
        };
        let token = CaptureToken(state.lease.generation);
        self.post(status, token, event);
        if matches!(
            event,
            ColumnEvent::PointerReleased
                | ColumnEvent::CaptureLost
                | ColumnEvent::Cancel
                | ColumnEvent::ViewportChanged
        ) {
            state.lease.release(token);
        }
        true
    }
    pub(super) fn viewport_changed(&self, status: Status) {
        if self.0.borrow().lease.owner == Some(status) {
            self.post_current(ColumnEvent::ViewportChanged);
        }
    }
    pub(super) fn pending_loss(&self) -> Option<(Status, CaptureToken)> {
        let state = self.0.borrow();
        let owner = state.lease.owner?;
        let token = CaptureToken(state.lease.generation);
        (!state.native.active(token)).then_some((owner, token))
    }
    /// Called only by RenderRuntime after taking an event from the inbox.
    pub fn dispatch(&self, status: Status, token: CaptureToken, event: ColumnEvent, cx: &mut App) {
        let target = self.0.borrow().targets[status_key(status) as usize].clone();
        if let Some(target) = target {
            if target
                .update(cx, |column, cx| column.dispatch_routed(token, event, cx))
                .is_err()
            {
                self.release(token);
            }
        }
    }
    pub fn post_cancel(&self) {
        self.post_current(ColumnEvent::Cancel);
    }
    pub fn post_capture_lost(&self, generation: u64) {
        let matches = {
            let state = self.0.borrow();
            state.lease.owner.is_some() && state.lease.generation == generation
        };
        if matches {
            self.post_current(ColumnEvent::CaptureLost);
        }
    }
    pub fn element(&self) -> impl IntoElement {
        let moved = self.clone();
        let released = self.clone();
        canvas(
            |_, _, _| (),
            move |_, _, window, _| {
                let moved = moved.clone();
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Capture {
                        return;
                    }
                    let input = if moved.pending_loss().is_some() {
                        ColumnEvent::CaptureLost
                    } else if event.pressed_button == Some(MouseButton::Left) {
                        ColumnEvent::PointerMoved {
                            y: f32::from(event.position.y),
                        }
                    } else {
                        ColumnEvent::Cancel
                    };
                    if moved.post_current(input) {
                        cx.stop_propagation();
                    }
                });
                let released = released.clone();
                window.on_mouse_event(move |event: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture
                        && event.button == MouseButton::Left
                        && released.post_current(ColumnEvent::PointerReleased)
                    {
                        cx.stop_propagation();
                    }
                });
            },
        )
        .absolute()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(target_os = "windows")]
    #[test]
    fn an_unattached_window_cannot_authorize_a_capture() {
        let input = ColumnInput::default();
        assert_eq!(input.claim(Status::Todo), None);
        assert!(!input.active());
    }
    #[test]
    fn a_duplicate_claim_is_idempotent_and_another_column_is_rejected() {
        let mut lease = CaptureLease::default();
        let token = lease.claim(Status::Todo).unwrap();
        assert_eq!(lease.claim(Status::Todo), Some(token));
        assert_eq!(lease.claim(Status::Doing), None);
        assert_eq!(lease.owner, Some(Status::Todo));
    }
    #[test]
    fn old_loss_or_release_cannot_end_the_next_capture() {
        let mut lease = CaptureLease::default();
        let old = lease.claim(Status::Todo).unwrap();
        assert!(lease.release(old));
        assert!(!lease.release(old));
        let current = lease.claim(Status::Doing).unwrap();
        assert_ne!(old, current);
        assert!(!lease.release(old));
        assert_eq!(lease.owner, Some(Status::Doing));
        assert!(lease.release(current));
    }
}
