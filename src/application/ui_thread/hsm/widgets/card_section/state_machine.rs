//! Passive column interaction HSM. GPUI effects are applied after each RTC.

use super::scroll_y_from_thumb_top;
use rs_hsm_::{SM_Hsm, SM_HsmState, SM_HsmTrait, SM_RetState, SM_StatePtr};

pub(super) use crate::application::ui_thread::thread::evt::{
    CaptureToken, ColumnEvent, DragGeometry,
};

#[derive(Default)]
pub(super) struct Effects {
    pub started: bool,
    pub finished: Option<CaptureToken>,
    pub scroll: Option<f32>,
}

#[derive(Default)]
pub(super) struct ColumnInteraction {
    token: CaptureToken,
    geometry: DragGeometry,
    offset: f32,
    effects: Effects,
}

impl SM_HsmTrait for ColumnInteraction {
    type Evt = ColumnEvent;
    fn topInitial(_: &mut Self) -> SM_StatePtr<Self> {
        &IDLE
    }
    fn init(hsm: &mut SM_Hsm<Self>, me: &mut Self) {
        hsm.init(me);
    }
    fn dispatch(hsm: &mut SM_Hsm<Self>, me: &mut Self, event: &ColumnEvent) {
        hsm.dispatch(me, event);
    }
}

impl ColumnInteraction {
    pub fn dispatch_routed(
        &mut self,
        hsm: &mut SM_Hsm<Self>,
        token: CaptureToken,
        event: ColumnEvent,
    ) -> Effects {
        if !matches!(
            event,
            ColumnEvent::ThumbPressed { .. } | ColumnEvent::TrackPressed { .. }
        ) && self.token != token
        {
            return Effects::default();
        }
        self.dispatch(hsm, event)
    }

    pub fn dispatch(&mut self, hsm: &mut SM_Hsm<Self>, event: ColumnEvent) -> Effects {
        hsm.dispatch(self, &event);
        std::mem::take(&mut self.effects)
    }
}

static IDLE: SM_HsmState<ColumnInteraction> = SM_HsmState {
    super_: None,
    init_: None,
    entry_: None,
    exit_: None,
    handler_: |me, event| match *event {
        ColumnEvent::ThumbPressed {
            token,
            geometry,
            offset,
        } => {
            me.token = token;
            me.geometry = geometry;
            me.offset = offset;
            SM_RetState::TRAN(&THUMB_DRAGGING)
        }
        ColumnEvent::TrackPressed {
            top,
            viewport,
            max_scroll,
        } => {
            me.effects.scroll = Some(scroll_y_from_thumb_top(top, viewport, max_scroll, viewport));
            SM_RetState::HANDLED
        }
        _ => SM_RetState::HANDLED,
    },
};

static THUMB_DRAGGING: SM_HsmState<ColumnInteraction> = SM_HsmState {
    super_: None,
    init_: None,
    entry_: Some(|me| me.effects.started = true),
    exit_: Some(|me, _| {
        me.effects.finished = Some(me.token);
        me.token = CaptureToken::default();
        me.geometry = DragGeometry::default();
    }),
    handler_: |me, event| match *event {
        ColumnEvent::PointerMoved { y } => {
            let g = me.geometry;
            let offset = scroll_y_from_thumb_top(
                y - g.track_top - g.grab,
                g.viewport,
                g.max_scroll,
                g.viewport,
            );
            if offset != me.offset {
                me.offset = offset;
                me.effects.scroll = Some(offset);
            }
            SM_RetState::HANDLED
        }
        ColumnEvent::PointerReleased
        | ColumnEvent::CaptureLost
        | ColumnEvent::Cancel
        | ColumnEvent::ViewportChanged => SM_RetState::TRAN(&IDLE),
        // An external self-transition would rerun entry/exit and break idempotence.
        _ => SM_RetState::HANDLED,
    },
};

#[cfg(test)]
mod tests {
    use super::*;
    fn machine() -> (SM_Hsm<ColumnInteraction>, ColumnInteraction) {
        let mut hsm = SM_Hsm::new();
        let mut me = ColumnInteraction::default();
        hsm.init(&mut me);
        (hsm, me)
    }
    fn begin(token: u64) -> ColumnEvent {
        ColumnEvent::ThumbPressed {
            token: CaptureToken(token),
            geometry: DragGeometry {
                track_top: 10.,
                grab: 5.,
                viewport: 100.,
                max_scroll: 100.,
            },
            offset: 0.,
        }
    }
    #[test]
    fn queued_sessions_wait_for_drain_and_ignore_stale_input() {
        use crate::application::{
            snapshot::Status,
            ui_thread::thread::{evt::UI_Evt, inbox::UI_Inbox},
        };
        let (mut hsm, mut me) = machine();
        let mut inbox = UI_Inbox::new();
        for (token, event) in [
            (1, begin(1)),
            (1, ColumnEvent::PointerReleased),
            (2, begin(2)),
            (1, ColumnEvent::CaptureLost),
            (1, ColumnEvent::PointerMoved { y: 90. }),
            (2, ColumnEvent::PointerMoved { y: 40. }),
            (2, ColumnEvent::PointerReleased),
            (2, ColumnEvent::PointerReleased),
        ] {
            inbox.post(UI_Evt::Column {
                status: Status::Todo,
                token: CaptureToken(token),
                event,
            });
        }
        assert_eq!(me.token, CaptureToken::default());
        let mut starts = 0;
        let mut ends = Vec::new();
        let mut offsets = Vec::new();
        for e in inbox.take_all() {
            let UI_Evt::Column { token, event, .. } = e else {
                unreachable!()
            };
            let effects = me.dispatch_routed(&mut hsm, token, event);
            starts += usize::from(effects.started);
            ends.extend(effects.finished);
            offsets.extend(effects.scroll);
        }
        assert_eq!(starts, 2);
        assert_eq!(ends, vec![CaptureToken(1), CaptureToken(2)]);
        assert_eq!(offsets, vec![50.]);
        assert_eq!(me.token, CaptureToken::default());
    }

    #[test]
    fn duplicate_down_keeps_the_original_session_and_grab() {
        let (mut hsm, mut me) = machine();
        assert!(me.dispatch(&mut hsm, begin(1)).started);
        assert!(!me.dispatch(&mut hsm, begin(2)).started);
        let effect = me.dispatch(&mut hsm, ColumnEvent::PointerMoved { y: 40. });
        assert_eq!(effect.scroll, Some(50.));
        assert_eq!(
            me.dispatch(&mut hsm, ColumnEvent::PointerReleased).finished,
            Some(CaptureToken(1))
        );
    }
    #[test]
    fn idle_move_and_up_and_repeated_move_are_noops() {
        let (mut hsm, mut me) = machine();
        assert!(me
            .dispatch(&mut hsm, ColumnEvent::PointerMoved { y: 40. })
            .scroll
            .is_none());
        assert!(me
            .dispatch(&mut hsm, ColumnEvent::PointerReleased)
            .finished
            .is_none());
        me.dispatch(&mut hsm, begin(1));
        assert_eq!(
            me.dispatch(&mut hsm, ColumnEvent::PointerMoved { y: 40. })
                .scroll,
            Some(50.)
        );
        assert!(me
            .dispatch(&mut hsm, ColumnEvent::PointerMoved { y: 40. })
            .scroll
            .is_none());
    }
    #[test]
    fn every_terminal_event_exits_once_and_accepts_a_fresh_session() {
        for event in [
            ColumnEvent::PointerReleased,
            ColumnEvent::CaptureLost,
            ColumnEvent::Cancel,
            ColumnEvent::ViewportChanged,
        ] {
            let (mut hsm, mut me) = machine();
            me.dispatch(&mut hsm, begin(1));
            assert_eq!(me.dispatch(&mut hsm, event).finished, Some(CaptureToken(1)));
            assert!(me.dispatch(&mut hsm, event).finished.is_none());
            assert!(me
                .dispatch(&mut hsm, ColumnEvent::PointerMoved { y: 40. })
                .scroll
                .is_none());
            assert!(me.dispatch(&mut hsm, begin(2)).started);
        }
    }
    #[test]
    fn track_click_cannot_reposition_an_active_drag() {
        let (mut hsm, mut me) = machine();
        me.dispatch(&mut hsm, begin(1));
        assert!(me
            .dispatch(
                &mut hsm,
                ColumnEvent::TrackPressed {
                    top: 20.,
                    viewport: 100.,
                    max_scroll: 100.
                }
            )
            .scroll
            .is_none());
    }
}
