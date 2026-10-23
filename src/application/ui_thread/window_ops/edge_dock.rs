//! Window geometry policies and effects for the WindowOps state machine.

use super::state_machine::{VisibleState, WindowState};

pub const TOP_SNAP_PX: f32 = 28.0;
pub const TOP_HOT_EDGE_PX: f32 = 0.0;
pub const COLLAPSE_DELAY_MS: u64 = 200;
pub const TRAY_REVEAL_HOLD_MS: u64 = 1600;
pub const SETTLE_DELAY_MS: u64 = 180;
pub const HOT_EDGE_POLL_MS: u64 = 50;
pub const DOCK_SLIDE_MS: u64 = 100;
pub const DOCK_PREPAINT_MS: u64 = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DockMode {
    Free,
    Expanded,
    Collapsed,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DockIntent {
    None,
    Dock { rect: Rect, remember: Size },
    Collapse { rect: Rect },
    Expand { rect: Rect },
    Undock { rect: Rect },
}

pub fn collapsed_top_rect(box_: Rect, _work: Rect) -> Rect {
    box_
}

pub fn expanded_top_rect(box_: Rect, work: Rect, remembered: Size) -> Rect {
    Rect {
        x: box_.x,
        y: work.y,
        width: remembered.width,
        height: remembered.height,
    }
}

pub fn is_near_top(box_: Rect, work: Rect) -> bool {
    box_.y - work.y <= TOP_SNAP_PX
}

pub fn is_cursor_on_top_hot_edge(cursor: Point, docked_x: f32, docked_width: f32, work: Rect) -> bool {
    if cursor.y < work.y - TOP_HOT_EDGE_PX || cursor.y > work.y + TOP_HOT_EDGE_PX {
        return false;
    }
    cursor.x >= docked_x && cursor.x <= docked_x + docked_width
}

fn visible_undock_rect(box_: Rect, work: Rect, remembered: Option<Size>) -> Rect {
    let size = remembered.unwrap_or(Size {
        width: box_.width,
        height: box_.height.max(800.0),
    });
    Rect {
        x: box_.x,
        y: box_.y.max(work.y),
        width: size.width,
        height: size.height,
    }
}

pub fn intent_after_settle(
    mode: DockMode,
    box_: Rect,
    work: Rect,
    remembered: Option<Size>,
    maximized: bool,
    minimized: bool,
) -> DockIntent {
    if minimized {
        return DockIntent::None;
    }
    if maximized {
        // Native maximization already owns the geometry. Updating the window
        // context must not reposition it and undo that native state.
        return DockIntent::None;
    }
    if mode == DockMode::Free {
        if !is_near_top(box_, work) {
            return DockIntent::None;
        }
        return DockIntent::Dock {
            rect: Rect { y: work.y, ..box_ },
            remember: Size {
                width: box_.width,
                height: box_.height,
            },
        };
    }
    if !is_near_top(box_, work) {
        return DockIntent::Undock {
            rect: visible_undock_rect(box_, work, remembered),
        };
    }
    if mode == DockMode::Expanded && box_.y != work.y {
        return DockIntent::Dock {
            rect: Rect { y: work.y, ..box_ },
            remember: Size {
                width: box_.width,
                height: box_.height,
            },
        };
    }
    DockIntent::None
}

pub fn intent_after_pointer_leave(
    mode: DockMode,
    box_: Rect,
    work: Rect,
    hold_open: bool,
    minimized: bool,
) -> DockIntent {
    if minimized || hold_open || mode != DockMode::Expanded {
        return DockIntent::None;
    }
    DockIntent::Collapse {
        rect: collapsed_top_rect(box_, work),
    }
}

pub fn intent_after_pointer_enter(
    mode: DockMode,
    box_: Rect,
    work: Rect,
    remembered: Option<Size>,
) -> DockIntent {
    let Some(remembered) = remembered else {
        return DockIntent::None;
    };
    if mode != DockMode::Collapsed {
        return DockIntent::None;
    }
    DockIntent::Expand {
        rect: expanded_top_rect(box_, work, remembered),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayRevealPlan {
    ExpandDock,
    RestoreWindow,
}

pub fn tray_reveal_plan(mode: DockMode) -> TrayRevealPlan {
    if mode == DockMode::Collapsed {
        TrayRevealPlan::ExpandDock
    } else {
        TrayRevealPlan::RestoreWindow
    }
}

pub fn is_point_in_rect(point: Point, box_: Rect) -> bool {
    point.x >= box_.x
        && point.x <= box_.x + box_.width
        && point.y >= box_.y
        && point.y <= box_.y + box_.height
}

pub fn should_collapse_after_tray_reveal(pointer_over_window: bool, hold_open: bool) -> bool {
    !pointer_over_window && !hold_open
}

pub fn slide_up_y(start_y: i32, height: i32, t: f32) -> i32 {
    let t = t.clamp(0.0, 1.0);
    let eased = t * t * (3.0 - 2.0 * t);
    start_y - (height as f32 * eased).round() as i32
}

pub fn hide_finish_y(start_y: i32, height: i32, restore_immediately: bool) -> i32 {
    if restore_immediately {
        start_y
    } else {
        start_y - height
    }
}

pub fn slide_down_y(rest_y: i32, height: i32, t: f32) -> i32 {
    slide_up_y(rest_y, height, 1.0 - t.clamp(0.0, 1.0))
}

#[derive(Debug, Clone, PartialEq)]
pub struct DockSession {
    pub state: WindowState,
    pub remembered: Option<Size>,
    pub docked_x: f32,
    pub docked_width: f32,
}

impl DockSession {
    pub fn new() -> Self {
        Self {
            state: WindowState::Visible(VisibleState::Free),
            remembered: None,
            docked_x: 0.0,
            docked_width: 0.0,
        }
    }

    /// Geometry policy view, derived from the single window behavior state.
    pub fn mode(&self) -> DockMode {
        match self.state {
            WindowState::Collapsing | WindowState::DockedHidden => DockMode::Collapsed,
            _ if self.state.visible_context() == VisibleState::Docked => DockMode::Expanded,
            _ => DockMode::Free,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DockAction {
    None,
    ShowAt { x: f32, y: f32 },
    Hide,
    Show,
}

pub fn collapse_should_wait_for_settle(elapsed_ms: u64) -> bool {
    elapsed_ms < SETTLE_DELAY_MS + 80
}

pub fn should_arm_collapse_on_hold_release(mode: DockMode, pointer_inside: bool) -> bool {
    mode == DockMode::Expanded && !pointer_inside
}

pub fn remember_expanded_size(session: &mut DockSession, box_: Rect, maximized: bool, minimized: bool) {
    if session.mode() == DockMode::Expanded && !maximized && !minimized {
        session.remembered = Some(Size {
            width: box_.width,
            height: box_.height,
        });
        session.docked_x = box_.x;
        session.docked_width = box_.width;
    }
}

pub fn apply_intent(session: &mut DockSession, intent: DockIntent) -> DockAction {
    if session.state.is_tray_hidden() {
        return DockAction::None;
    }
    if matches!(intent, DockIntent::Dock { .. } | DockIntent::Undock { .. })
        && !matches!(
            session.state,
            WindowState::Visible(_) | WindowState::Settling(_)
        )
    {
        return DockAction::None;
    }
    match intent {
        DockIntent::None => DockAction::None,
        DockIntent::Dock { rect, remember } => {
            session.remembered = Some(remember);
            session.docked_x = rect.x;
            session.docked_width = rect.width;
            session.state = WindowState::Visible(VisibleState::Docked);
            DockAction::ShowAt {
                x: rect.x,
                y: rect.y,
            }
        }
        DockIntent::Collapse { rect } => {
            if !session.state.begin_collapse() {
                return DockAction::None;
            }
            session.remembered = Some(Size {
                width: rect.width,
                height: rect.height,
            });
            session.docked_x = rect.x;
            session.docked_width = rect.width;
            DockAction::Hide
        }
        DockIntent::Expand { .. } => {
            if !session.state.begin_expand() {
                return DockAction::None;
            }
            DockAction::Show
        }
        DockIntent::Undock { rect } => {
            session.state = WindowState::Visible(VisibleState::Free);
            session.remembered = None;
            DockAction::ShowAt {
                x: rect.x,
                y: rect.y,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORK: Rect = Rect {
        x: 0.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
    };
    const BOARD: Rect = Rect {
        x: 200.0,
        y: 12.0,
        width: 1200.0,
        height: 800.0,
    };

    #[test]
    fn settling_near_the_top_docks_and_remembers_the_full_size() {
        assert_eq!(
            intent_after_settle(DockMode::Free, BOARD, WORK, None, false, false),
            DockIntent::Dock {
                rect: Rect { y: 0.0, ..BOARD },
                remember: Size {
                    width: 1200.0,
                    height: 800.0,
                },
            }
        );
    }

    #[test]
    fn moving_an_expanded_dock_within_snap_range_realigns_to_the_work_top() {
        let work = Rect { y: 100.0, ..WORK };
        for offset in [-12.0, TOP_SNAP_PX] {
            let moved = Rect {
                y: work.y + offset,
                ..BOARD
            };
            let mut session = DockSession::new();
            session.state = WindowState::Visible(VisibleState::Docked);
            let intent = intent_after_settle(session.mode(), moved, work, None, false, false);
            assert_eq!(
                apply_intent(&mut session, intent),
                DockAction::ShowAt {
                    x: moved.x,
                    y: work.y
                }
            );
            assert_eq!(session.mode(), DockMode::Expanded);
            assert_eq!(
                intent_after_settle(
                    session.mode(),
                    Rect { y: work.y, ..moved },
                    work,
                    session.remembered,
                    false,
                    false,
                ),
                DockIntent::None
            );
        }
    }

    #[test]
    fn a_pending_dock_cannot_move_the_window_before_release_or_after_tray_hide() {
        for state in [
            WindowState::Moving(VisibleState::Free),
            WindowState::TrayHidden(VisibleState::Free),
            WindowState::Collapsing,
            WindowState::Expanding,
        ] {
            let mut session = DockSession::new();
            session.state = state;
            let before = session.clone();
            let intent = intent_after_settle(DockMode::Free, BOARD, WORK, None, false, false);
            assert_eq!(apply_intent(&mut session, intent), DockAction::None);
            assert_eq!(session, before);
        }
    }

    #[test]
    fn settling_away_from_the_top_stays_free() {
        assert_eq!(
            intent_after_settle(
                DockMode::Free,
                Rect { y: 120.0, ..BOARD },
                WORK,
                None,
                false,
                false,
            ),
            DockIntent::None
        );
    }

    #[test]
    fn a_maximized_window_never_docks() {
        assert_eq!(
            intent_after_settle(DockMode::Free, BOARD, WORK, None, true, false),
            DockIntent::None
        );
    }

    #[test]
    fn maximizing_a_docked_window_never_emits_a_position_change() {
        assert_eq!(
            intent_after_settle(DockMode::Expanded, BOARD, WORK, None, true, false),
            DockIntent::None
        );
    }

    #[test]
    fn dragging_an_expanded_dock_off_the_top_undocks() {
        assert_eq!(
            intent_after_settle(
                DockMode::Expanded,
                Rect { y: 90.0, ..BOARD },
                WORK,
                Some(Size {
                    width: 1200.0,
                    height: 800.0,
                }),
                false,
                false,
            ),
            DockIntent::Undock {
                rect: Rect {
                    x: 200.0,
                    y: 90.0,
                    width: 1200.0,
                    height: 800.0,
                },
            }
        );
    }

    #[test]
    fn pointer_leave_hides_in_place_without_changing_the_window_rect() {
        assert!(COLLAPSE_DELAY_MS > 0);
        let expanded = Rect { y: 0.0, ..BOARD };
        assert_eq!(
            intent_after_pointer_leave(DockMode::Expanded, expanded, WORK, false, false),
            DockIntent::Collapse { rect: expanded }
        );
        assert_eq!(collapsed_top_rect(expanded, WORK), expanded);
    }

    #[test]
    fn pointer_leave_does_not_collapse_while_held_open() {
        assert_eq!(
            intent_after_pointer_leave(
                DockMode::Expanded,
                Rect { y: 0.0, ..BOARD },
                WORK,
                true,
                false,
            ),
            DockIntent::None
        );
    }

    #[test]
    fn pointer_leave_does_not_collapse_a_minimized_window() {
        assert_eq!(
            intent_after_pointer_leave(
                DockMode::Expanded,
                Rect { y: 0.0, ..BOARD },
                WORK,
                false,
                true,
            ),
            DockIntent::None
        );
    }

    #[test]
    fn settling_while_minimized_does_not_move_or_hide() {
        assert_eq!(
            intent_after_settle(
                DockMode::Expanded,
                Rect { y: 90.0, ..BOARD },
                WORK,
                Some(Size {
                    width: 1200.0,
                    height: 800.0,
                }),
                false,
                true,
            ),
            DockIntent::None
        );
    }

    #[test]
    fn tray_click_expands_a_collapsed_dock_and_otherwise_restores_the_window() {
        assert_eq!(tray_reveal_plan(DockMode::Collapsed), TrayRevealPlan::ExpandDock);
        assert_eq!(
            tray_reveal_plan(DockMode::Expanded),
            TrayRevealPlan::RestoreWindow
        );
        assert_eq!(tray_reveal_plan(DockMode::Free), TrayRevealPlan::RestoreWindow);
    }

    #[test]
    fn tray_reveal_collapses_again_if_the_cursor_is_not_on_the_window() {
        assert!(should_collapse_after_tray_reveal(false, false));
        assert!(!should_collapse_after_tray_reveal(true, false));
        assert!(!should_collapse_after_tray_reveal(false, true));
        assert!(TRAY_REVEAL_HOLD_MS > COLLAPSE_DELAY_MS);
        assert!(!is_point_in_rect(
            Point { x: 10.0, y: 10.0 },
            Rect {
                x: 200.0,
                y: 0.0,
                width: 1200.0,
                height: 800.0,
            }
        ));
        assert!(is_point_in_rect(
            Point { x: 400.0, y: 40.0 },
            Rect {
                x: 200.0,
                y: 0.0,
                width: 1200.0,
                height: 800.0,
            }
        ));
    }

    #[test]
    fn pointer_enter_shows_a_collapsed_dock_without_changing_size() {
        let hidden = collapsed_top_rect(Rect { y: 0.0, ..BOARD }, WORK);
        assert_eq!(
            intent_after_pointer_enter(
                DockMode::Collapsed,
                hidden,
                WORK,
                Some(Size {
                    width: 1200.0,
                    height: 800.0,
                }),
            ),
            DockIntent::Expand {
                rect: Rect {
                    x: hidden.x,
                    y: 0.0,
                    width: 1200.0,
                    height: 800.0,
                },
            }
        );
    }

    #[test]
    fn only_a_cursor_on_the_docked_top_edge_can_reveal_the_hidden_window() {
        assert!(is_cursor_on_top_hot_edge(
            Point { x: 400.0, y: 0.0 },
            200.0,
            1200.0,
            WORK
        ));
        assert!(!is_cursor_on_top_hot_edge(
            Point { x: 50.0, y: 0.0 },
            200.0,
            1200.0,
            WORK
        ));
        for y in [-1.0, 1.0, 8.0, 40.0] {
            assert!(!is_cursor_on_top_hot_edge(
                Point { x: 400.0, y },
                200.0,
                1200.0,
                WORK
            ));
        }
    }

    #[test]
    fn hide_keeps_the_window_painted_while_sliding_off_screen() {
        assert_eq!(slide_up_y(100, 800, 0.0), 100);
        assert_eq!(slide_up_y(100, 800, 1.0), 100 - 800);
        let mid = slide_up_y(100, 800, 0.5);
        assert!(mid < 100 && mid > 100 - 800);
    }

    #[test]
    fn after_hide_the_window_stays_off_screen() {
        assert_eq!(hide_finish_y(100, 800, false), -700);
        assert_eq!(hide_finish_y(100, 800, true), 100);
    }

    #[test]
    fn show_slides_down_from_off_screen_as_the_reverse_of_hide() {
        assert_eq!(slide_down_y(100, 800, 0.0), hide_finish_y(100, 800, false));
        assert_eq!(slide_down_y(100, 800, 1.0), 100);
        assert_eq!(slide_down_y(100, 800, 0.5), slide_up_y(100, 800, 0.5));
    }

    #[test]
    fn applying_a_dock_snaps_to_the_work_top_and_stays_expanded() {
        let mut session = DockSession::new();
        let intent = intent_after_settle(DockMode::Free, BOARD, WORK, None, false, false);
        assert_eq!(
            apply_intent(&mut session, intent),
            DockAction::ShowAt {
                x: 200.0,
                y: 0.0,
            }
        );
        assert_eq!(session.mode(), DockMode::Expanded);
        assert_eq!(
            session.remembered,
            Some(Size {
                width: 1200.0,
                height: 800.0,
            })
        );
        assert_eq!(session.docked_x, 200.0);
        assert_eq!(session.docked_width, 1200.0);
    }

    #[test]
    fn applying_a_collapse_hides_in_place_and_keeps_the_span() {
        let mut session = DockSession {
            state: WindowState::Visible(VisibleState::Docked),
            remembered: Some(Size {
                width: 1200.0,
                height: 800.0,
            }),
            docked_x: 200.0,
            docked_width: 1200.0,
        };
        let expanded = Rect { y: 0.0, ..BOARD };
        let intent = intent_after_pointer_leave(DockMode::Expanded, expanded, WORK, false, false);
        assert_eq!(apply_intent(&mut session, intent), DockAction::Hide);
        assert_eq!(session.state, WindowState::Collapsing);
        session.state.finish_slide();
        assert_eq!(session.state, WindowState::DockedHidden);
        assert_eq!(session.docked_x, 200.0);
        assert_eq!(session.docked_width, 1200.0);
    }

    #[test]
    fn applying_an_expand_shows_without_changing_size() {
        let mut session = DockSession {
            state: WindowState::DockedHidden,
            remembered: Some(Size {
                width: 1200.0,
                height: 800.0,
            }),
            docked_x: 200.0,
            docked_width: 1200.0,
        };
        let hidden = collapsed_top_rect(Rect { y: 0.0, ..BOARD }, WORK);
        let intent = intent_after_pointer_enter(
            DockMode::Collapsed,
            hidden,
            WORK,
            session.remembered,
        );
        assert_eq!(apply_intent(&mut session, intent), DockAction::Show);
        assert_eq!(session.state, WindowState::Expanding);
        session.state.finish_slide();
        assert_eq!(session.state, WindowState::Visible(VisibleState::Docked));
        assert_eq!(
            session.remembered,
            Some(Size {
                width: 1200.0,
                height: 800.0,
            })
        );
    }

    #[test]
    fn applying_an_undock_clears_the_session_and_shows_at_the_new_origin() {
        let mut session = DockSession {
            state: WindowState::Visible(VisibleState::Docked),
            remembered: Some(Size {
                width: 1200.0,
                height: 800.0,
            }),
            docked_x: 200.0,
            docked_width: 1200.0,
        };
        let intent = intent_after_settle(
            DockMode::Expanded,
            Rect { y: 90.0, ..BOARD },
            WORK,
            session.remembered,
            false,
            false,
        );
        assert_eq!(
            apply_intent(&mut session, intent),
            DockAction::ShowAt {
                x: 200.0,
                y: 90.0,
            }
        );
        assert_eq!(session.mode(), DockMode::Free);
        assert_eq!(session.remembered, None);
    }

    #[test]
    fn an_expanded_resize_updates_the_remembered_size() {
        let mut session = DockSession {
            state: WindowState::Visible(VisibleState::Docked),
            remembered: Some(Size {
                width: 1200.0,
                height: 800.0,
            }),
            docked_x: 200.0,
            docked_width: 1200.0,
        };
        remember_expanded_size(
            &mut session,
            Rect {
                x: 180.0,
                y: 0.0,
                width: 1100.0,
                height: 900.0,
            },
            false,
            false,
        );
        assert_eq!(
            session.remembered,
            Some(Size {
                width: 1100.0,
                height: 900.0,
            })
        );
        assert_eq!(session.docked_x, 180.0);
        assert_eq!(session.docked_width, 1100.0);
    }

    #[test]
    fn collapse_waits_until_the_window_has_settled() {
        assert!(collapse_should_wait_for_settle(SETTLE_DELAY_MS + 79));
        assert!(!collapse_should_wait_for_settle(SETTLE_DELAY_MS + 80));
    }

    #[test]
    fn releasing_hold_open_collapses_only_when_the_pointer_already_left() {
        assert!(should_arm_collapse_on_hold_release(
            DockMode::Expanded,
            false
        ));
        assert!(!should_arm_collapse_on_hold_release(
            DockMode::Expanded,
            true
        ));
        assert!(!should_arm_collapse_on_hold_release(DockMode::Free, false));
    }
}
