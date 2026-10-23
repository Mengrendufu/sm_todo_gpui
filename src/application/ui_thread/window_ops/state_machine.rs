//! Window behavior states. GPUI callbacks and native geometry stay in the adapter.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum VisibleState {
    Free,
    Docked,
    Maximized,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowState {
    Visible(VisibleState),
    Moving(VisibleState),
    Settling(VisibleState),
    Collapsing,
    DockedHidden,
    Expanding,
    TrayHidden(VisibleState),
}

impl WindowState {
    pub fn is_tray_hidden(self) -> bool {
        matches!(self, Self::TrayHidden(_))
    }

    pub fn is_geometry_pending(self) -> bool {
        matches!(self, Self::Moving(_) | Self::Settling(_))
    }

    pub fn visible_context(self) -> VisibleState {
        match self {
            Self::Visible(context)
            | Self::Moving(context)
            | Self::Settling(context)
            | Self::TrayHidden(context) => context,
            Self::Collapsing | Self::DockedHidden | Self::Expanding => VisibleState::Docked,
        }
    }

    /// Animated and hidden bounds are effects, not user geometry changes.
    pub fn note_geometry(&mut self, moving: bool) -> bool {
        match *self {
            Self::Visible(context) | Self::Moving(context) | Self::Settling(context) => {
                *self = if moving {
                    Self::Moving(context)
                } else {
                    Self::Settling(context)
                };
                true
            }
            Self::Collapsing | Self::Expanding if moving => {
                *self = Self::Moving(VisibleState::Docked);
                true
            }
            _ => false,
        }
    }

    pub fn settle(&mut self, maximized: bool) {
        if matches!(self, Self::Visible(_) | Self::Settling(_)) {
            *self = Self::Visible(if maximized {
                VisibleState::Maximized
            } else if self.visible_context() == VisibleState::Docked {
                VisibleState::Docked
            } else {
                VisibleState::Free
            });
        }
    }

    pub fn begin_collapse(&mut self) -> bool {
        if *self != Self::Visible(VisibleState::Docked) {
            return false;
        }
        *self = Self::Collapsing;
        true
    }

    pub fn begin_expand(&mut self) -> bool {
        if !matches!(self, Self::Collapsing | Self::DockedHidden) {
            return false;
        }
        *self = Self::Expanding;
        true
    }

    pub fn finish_slide(&mut self) {
        *self = match *self {
            Self::Collapsing => Self::DockedHidden,
            Self::Expanding => Self::Visible(VisibleState::Docked),
            state => state,
        };
    }

    pub fn hide_to_tray(&mut self) {
        *self = Self::TrayHidden(self.visible_context());
    }

    pub fn reveal_from_tray(&mut self) -> bool {
        if let Self::TrayHidden(context) = *self {
            *self = Self::Visible(context);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn moving_cannot_collapse_and_keeps_its_origin_until_settled() {
        for context in [
            VisibleState::Free,
            VisibleState::Docked,
            VisibleState::Maximized,
        ] {
            let mut state = WindowState::Visible(context);
            assert!(state.note_geometry(true));
            assert!(!state.begin_collapse());
            state.settle(false);
            assert_eq!(state, WindowState::Moving(context));
            assert!(state.note_geometry(false));
            assert!(!state.begin_collapse());
            state.settle(false);
            assert_eq!(
                state,
                WindowState::Visible(if context == VisibleState::Docked {
                    VisibleState::Docked
                } else {
                    VisibleState::Free
                })
            );
        }
    }

    #[test]
    fn animations_reach_stable_states_only_after_completion() {
        let mut state = WindowState::Visible(VisibleState::Docked);
        assert!(state.begin_collapse());
        assert_eq!(state, WindowState::Collapsing);
        assert!(!state.note_geometry(false));
        state.finish_slide();
        assert_eq!(state, WindowState::DockedHidden);
        assert!(state.begin_expand());
        assert_eq!(state, WindowState::Expanding);
        assert!(!state.begin_collapse());
        state.finish_slide();
        assert_eq!(state, WindowState::Visible(VisibleState::Docked));
    }

    #[test]
    fn tray_interrupts_every_phase_and_restores_the_visible_context() {
        for initial in [
            WindowState::Visible(VisibleState::Free),
            WindowState::Visible(VisibleState::Maximized),
            WindowState::Moving(VisibleState::Docked),
            WindowState::Settling(VisibleState::Free),
            WindowState::Collapsing,
            WindowState::DockedHidden,
            WindowState::Expanding,
        ] {
            let mut state = initial;
            let context = state.visible_context();
            state.hide_to_tray();
            assert!(!state.note_geometry(false));
            assert!(!state.begin_expand());
            state.finish_slide();
            state.settle(false);
            assert_eq!(state, WindowState::TrayHidden(context));
            assert!(state.reveal_from_tray());
            assert_eq!(state, WindowState::Visible(context));
        }
    }

    #[test]
    fn entering_maximized_and_restoring_do_not_leave_a_docked_context() {
        let mut state = WindowState::Visible(VisibleState::Docked);
        state.note_geometry(false);
        state.settle(true);
        assert_eq!(state, WindowState::Visible(VisibleState::Maximized));
        assert!(!state.begin_collapse());
        state.note_geometry(false);
        state.settle(false);
        assert_eq!(state, WindowState::Visible(VisibleState::Free));
    }

    #[test]
    fn native_drag_interrupts_an_animation_without_accepting_its_completion() {
        for initial in [WindowState::Collapsing, WindowState::Expanding] {
            let mut state = initial;
            assert!(state.note_geometry(true));
            state.finish_slide();
            assert_eq!(state, WindowState::Moving(VisibleState::Docked));
            assert!(!state.begin_collapse());
        }
    }
}
