//! Top-edge auto-hide. Same rules as Tauri `useTopEdgeDock`, top edge only.

use std::time::{Duration, Instant};

use gpui::{Context, Timer, Window};

use super::edge_dock::{
    apply_intent, collapse_should_wait_for_settle, intent_after_pointer_enter,
    intent_after_pointer_leave, intent_after_settle, is_cursor_on_top_hot_edge, is_point_in_rect,
    remember_expanded_size, should_arm_collapse_on_hold_release, should_collapse_after_tray_reveal,
    should_skip_settle, slide_down_y, slide_up_y, tray_reveal_plan, DockAction, DockIntent,
    DockMode, DockSession, Rect, Size, TrayRevealPlan, APPLY_GUARD_MS, COLLAPSE_DELAY_MS,
    DOCK_PREPAINT_MS, DOCK_SLIDE_MS, HOT_EDGE_POLL_MS, SETTLE_DELAY_MS, TRAY_REVEAL_HOLD_MS,
};
use super::window_chrome::{self, SlideFrame};
use super::WindowOps;
use crate::application::ui_thread::hsm::SM_UI;

pub struct TopEdgeDock {
    session: DockSession,
    applying: bool,
    apply_gen: u64,
    pointer_inside: bool,
    last_moved_at: Instant,
    settle_gen: u64,
    collapse_gen: u64,
    poll_gen: u64,
    slide_gen: u64,
    slide_frame: Option<SlideFrame>,
    last_hold_open: bool,
    wired: bool,
    hidden_to_tray: bool,
}

impl TopEdgeDock {
    pub fn new() -> Self {
        Self {
            session: DockSession::new(),
            applying: false,
            apply_gen: 0,
            pointer_inside: true,
            last_moved_at: Instant::now(),
            settle_gen: 0,
            collapse_gen: 0,
            poll_gen: 0,
            slide_gen: 0,
            slide_frame: None,
            last_hold_open: false,
            wired: false,
            hidden_to_tray: false,
        }
    }

    fn suspend_for_tray(&mut self) {
        self.hidden_to_tray = true;
        self.settle_gen = self.settle_gen.wrapping_add(1);
        self.collapse_gen = self.collapse_gen.wrapping_add(1);
        self.poll_gen = self.poll_gen.wrapping_add(1);
        self.slide_gen = self.slide_gen.wrapping_add(1);
        self.apply_gen = self.apply_gen.wrapping_add(1);
        self.applying = false;
    }
}

impl WindowOps {
    pub(crate) fn hide_to_tray(&mut self, window: &Window) {
        self.top_dock.suspend_for_tray();
        window_chrome::hide(window);
    }

    pub(super) fn wire_top_edge_dock(&mut self, window: &mut Window, cx: &mut Context<SM_UI>) {
        if self.top_dock.wired {
            return;
        }
        self.top_dock.wired = true;
        cx.observe_window_bounds(window, |this, window, cx| {
            this.window_ops.hold_open = this.dock_hold_open();
            this.window_ops.note_window_moved(window, cx);
        })
        .detach();
    }

    pub(super) fn sync_window_hover(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        if self.top_dock.hidden_to_tray {
            return;
        }
        let hovered = window.is_window_hovered();
        if hovered != self.top_dock.pointer_inside {
            self.note_window_hover(hovered, window, cx);
        }
    }

    pub(super) fn sync_dock_hold_open(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        if self.top_dock.hidden_to_tray {
            return;
        }
        let hold_open = self.hold_open;
        let was = self.top_dock.last_hold_open;
        self.top_dock.last_hold_open = hold_open;
        if hold_open {
            if !was {
                self.top_dock.collapse_gen = self.top_dock.collapse_gen.wrapping_add(1);
            }
            return;
        }
        if was
            && should_arm_collapse_on_hold_release(
                self.top_dock.session.mode,
                self.top_dock.pointer_inside,
            )
        {
            self.schedule_collapse(window, cx, COLLAPSE_DELAY_MS);
        }
    }

    pub(crate) fn reveal_from_tray(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        let was_hidden = self.top_dock.hidden_to_tray;
        self.top_dock.hidden_to_tray = false;
        self.top_dock.collapse_gen = self.top_dock.collapse_gen.wrapping_add(1);
        // A hide can interrupt the slide that was revealing a docked window.
        if was_hidden && self.top_dock.session.mode == DockMode::Expanded {
            if let Some(work) = window_chrome::work_area(window, cx) {
                let box_ = self.reveal_box(work);
                window_chrome::set_position(window, box_.x, box_.y);
            }
        }
        if tray_reveal_plan(self.top_dock.session.mode) == TrayRevealPlan::ExpandDock {
            if window_chrome::work_area(window, cx).is_none()
                || self.top_dock.session.remembered.is_none()
            {
                self.restore_window(window);
                self.arm_collapse_if_cursor_left(window, cx);
                return;
            }
            self.expand_now(window, cx);
        }
        self.restore_window(window);
        self.arm_collapse_if_cursor_left(window, cx);
    }

    fn restore_window(&self, window: &Window) {
        window_chrome::restore(window);
        window.activate_window();
    }

    fn arm_collapse_if_cursor_left(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        let box_ = window_chrome::window_rect(window);
        let over = window_chrome::cursor_point(window)
            .is_some_and(|cursor| is_point_in_rect(cursor, box_));
        self.top_dock.pointer_inside = over;
        if should_collapse_after_tray_reveal(over, self.hold_open) {
            self.schedule_collapse(window, cx, TRAY_REVEAL_HOLD_MS);
        }
    }

    fn note_window_moved(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        if self.top_dock.hidden_to_tray {
            return;
        }
        self.top_dock.last_moved_at = Instant::now();
        if should_skip_settle(self.top_dock.applying, self.top_dock.session.mode) {
            return;
        }
        self.schedule_settle(window, cx);
    }

    fn note_window_hover(&mut self, inside: bool, window: &Window, cx: &mut Context<SM_UI>) {
        self.top_dock.pointer_inside = inside;
        if inside {
            self.top_dock.collapse_gen = self.top_dock.collapse_gen.wrapping_add(1);
            if self.top_dock.session.mode != DockMode::Collapsed {
                return;
            }
            self.expand_now(window, cx);
            return;
        }
        if self.hold_open {
            return;
        }
        self.schedule_collapse(window, cx, COLLAPSE_DELAY_MS);
    }

    fn schedule_settle(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        self.top_dock.settle_gen = self.top_dock.settle_gen.wrapping_add(1);
        let gen = self.top_dock.settle_gen;
        cx.spawn_in(window, async move |this, cx| {
            Timer::after(Duration::from_millis(SETTLE_DELAY_MS)).await;
            this.update_in(cx, |this, window, cx| {
                this.window_ops.hold_open = this.dock_hold_open();
                if this.window_ops.top_dock.settle_gen != gen {
                    return;
                }
                this.window_ops.run_settle(window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn schedule_collapse(&mut self, window: &Window, cx: &mut Context<SM_UI>, delay_ms: u64) {
        self.top_dock.collapse_gen = self.top_dock.collapse_gen.wrapping_add(1);
        let gen = self.top_dock.collapse_gen;
        cx.spawn_in(window, async move |this, cx| {
            Timer::after(Duration::from_millis(delay_ms)).await;
            this.update_in(cx, |this, window, cx| {
                this.window_ops.hold_open = this.dock_hold_open();
                if this.window_ops.top_dock.collapse_gen != gen {
                    return;
                }
                this.window_ops.run_collapse(window, cx);
            })
            .ok();
        })
        .detach();
    }

    fn mark_applying(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        self.top_dock.applying = true;
        self.top_dock.apply_gen = self.top_dock.apply_gen.wrapping_add(1);
        let gen = self.top_dock.apply_gen;
        cx.spawn_in(window, async move |this, cx| {
            Timer::after(Duration::from_millis(APPLY_GUARD_MS)).await;
            this.update(cx, |this, _cx| {
                this.window_ops.hold_open = this.dock_hold_open();
                if this.window_ops.top_dock.apply_gen == gen {
                    this.window_ops.top_dock.applying = false;
                }
            })
            .ok();
        })
        .detach();
    }

    fn run_settle(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        if self.top_dock.hidden_to_tray
            || should_skip_settle(self.top_dock.applying, self.top_dock.session.mode)
        {
            return;
        }
        let Some(work) = window_chrome::work_area(window, cx) else {
            return;
        };
        let box_ = window_chrome::window_rect(window);
        let maximized = window.is_maximized();
        let minimized = window_chrome::is_minimized(window);
        remember_expanded_size(&mut self.top_dock.session, box_, maximized, minimized);
        let intent = intent_after_settle(
            self.top_dock.session.mode,
            box_,
            work,
            self.top_dock.session.remembered,
            maximized,
            minimized,
        );
        self.apply_now(intent, window, cx);
    }

    fn run_collapse(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        if self.top_dock.hidden_to_tray || self.hold_open || self.top_dock.pointer_inside {
            return;
        }
        let elapsed_ms = self
            .top_dock
            .last_moved_at
            .elapsed()
            .as_millis()
            .min(u128::from(u64::MAX)) as u64;
        if collapse_should_wait_for_settle(elapsed_ms) {
            self.schedule_collapse(window, cx, COLLAPSE_DELAY_MS);
            return;
        }
        let Some(work) = window_chrome::work_area(window, cx) else {
            return;
        };
        let box_ = window_chrome::window_rect(window);
        let intent = intent_after_pointer_leave(
            self.top_dock.session.mode,
            box_,
            work,
            self.hold_open,
            window_chrome::is_minimized(window),
        );
        self.apply_now(intent, window, cx);
    }

    fn expand_now(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        let Some(work) = window_chrome::work_area(window, cx) else {
            return;
        };
        let box_ = self.reveal_box(work);
        let intent = intent_after_pointer_enter(
            self.top_dock.session.mode,
            box_,
            work,
            self.top_dock.session.remembered,
        );
        self.apply_now(intent, window, cx);
    }

    fn reveal_box(&self, work: Rect) -> Rect {
        let remembered = self.top_dock.session.remembered.unwrap_or(Size {
            width: self.top_dock.session.docked_width,
            height: 800.0,
        });
        Rect {
            x: self.top_dock.session.docked_x,
            y: work.y,
            width: remembered.width,
            height: remembered.height,
        }
    }

    fn apply_now(&mut self, intent: DockIntent, window: &Window, cx: &mut Context<SM_UI>) {
        if self.top_dock.hidden_to_tray {
            return;
        }
        let action = apply_intent(&mut self.top_dock.session, intent);
        if action == DockAction::None {
            return;
        }
        self.mark_applying(window, cx);
        match action {
            DockAction::None => {}
            DockAction::ShowAt { x, y } => {
                self.stop_hot_edge_poll();
                window_chrome::show(window);
                window_chrome::set_position(window, x, y);
            }
            DockAction::Hide => {
                self.start_slide(window, cx, true);
            }
            DockAction::Show => {
                self.stop_hot_edge_poll();
                self.start_slide(window, cx, false);
            }
        }
    }

    fn stop_hot_edge_poll(&mut self) {
        self.top_dock.poll_gen = self.top_dock.poll_gen.wrapping_add(1);
    }

    fn start_hot_edge_poll(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        self.top_dock.poll_gen = self.top_dock.poll_gen.wrapping_add(1);
        let gen = self.top_dock.poll_gen;
        cx.spawn_in(window, async move |this, cx| loop {
            Timer::after(Duration::from_millis(HOT_EDGE_POLL_MS)).await;
            let keep = this
                .update_in(cx, |this, window, cx| {
                    this.window_ops.hold_open = this.dock_hold_open();
                    if this.window_ops.top_dock.poll_gen != gen
                        || this.window_ops.top_dock.session.mode != DockMode::Collapsed
                    {
                        return false;
                    }
                    this.window_ops.poll_hot_edge_once(window, cx);
                    true
                })
                .unwrap_or(false);
            if !keep {
                break;
            }
        })
        .detach();
    }

    fn poll_hot_edge_once(&mut self, window: &Window, cx: &mut Context<SM_UI>) {
        if self.top_dock.hidden_to_tray {
            return;
        }
        let Some(work) = window_chrome::work_area(window, cx) else {
            return;
        };
        let Some(cursor) = window_chrome::cursor_point(window) else {
            return;
        };
        if !is_cursor_on_top_hot_edge(
            cursor,
            self.top_dock.session.docked_x,
            self.top_dock.session.docked_width,
            work,
        ) {
            return;
        }
        self.top_dock.pointer_inside = true;
        self.top_dock.collapse_gen = self.top_dock.collapse_gen.wrapping_add(1);
        self.expand_now(window, cx);
    }

    fn start_slide(&mut self, window: &Window, cx: &mut Context<SM_UI>, hide: bool) {
        self.top_dock.slide_gen = self.top_dock.slide_gen.wrapping_add(1);
        let gen = self.top_dock.slide_gen;
        cx.spawn_in(window, async move |this, cx| {
            if hide {
                let Some(frame) = this
                    .update_in(cx, |this, window, _cx| {
                        this.window_ops.hold_open = this.dock_hold_open();
                        if this.window_ops.top_dock.slide_gen != gen {
                            return None;
                        }
                        let frame = window_chrome::begin_slide_hide(window)?;
                        this.window_ops.top_dock.slide_frame = Some(frame);
                        Some(frame)
                    })
                    .ok()
                    .flatten()
                else {
                    this.update_in(cx, |this, window, cx| {
                        this.window_ops.hold_open = this.dock_hold_open();
                        if this.window_ops.top_dock.slide_gen != gen {
                            return;
                        }
                        window_chrome::hide(window);
                        this.window_ops.start_hot_edge_poll(window, cx);
                    })
                    .ok();
                    return;
                };
                animate_slide(this.clone(), cx, gen, frame, true).await;
                this.update_in(cx, |this, window, cx| {
                    this.window_ops.hold_open = this.dock_hold_open();
                    if this.window_ops.top_dock.slide_gen != gen {
                        return;
                    }
                    window_chrome::finish_slide_hide(window, &frame);
                    this.window_ops.start_hot_edge_poll(window, cx);
                })
                .ok();
                return;
            }

            let Some(frame) = this
                .update_in(cx, |this, window, _cx| {
                    this.window_ops.hold_open = this.dock_hold_open();
                    if this.window_ops.top_dock.slide_gen != gen {
                        return None;
                    }
                    let frame = this.window_ops.top_dock.slide_frame?;
                    if !window_chrome::begin_slide_show(window, &frame) {
                        window_chrome::show(window);
                        return None;
                    }
                    Some(frame)
                })
                .ok()
                .flatten()
            else {
                return;
            };
            Timer::after(Duration::from_millis(DOCK_PREPAINT_MS)).await;
            animate_slide(this.clone(), cx, gen, frame, false).await;
            this.update_in(cx, |this, window, _cx| {
                this.window_ops.hold_open = this.dock_hold_open();
                if this.window_ops.top_dock.slide_gen != gen {
                    return;
                }
                window_chrome::set_slide_y(window, frame.x, frame.rest_y);
                window_chrome::finish_slide_show(window);
            })
            .ok();
        })
        .detach();
    }
}

async fn animate_slide(
    this: gpui::WeakEntity<SM_UI>,
    cx: &mut gpui::AsyncWindowContext,
    gen: u64,
    frame: SlideFrame,
    hide: bool,
) {
    let start = Instant::now();
    let duration = Duration::from_millis(DOCK_SLIDE_MS);
    loop {
        let t = (start.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.0);
        let y = if hide {
            slide_up_y(frame.rest_y, frame.height, t)
        } else {
            slide_down_y(frame.rest_y, frame.height, t)
        };
        let cancelled = this
            .update_in(cx, |this, window, _cx| {
                this.window_ops.hold_open = this.dock_hold_open();
                if this.window_ops.top_dock.slide_gen != gen {
                    return true;
                }
                window_chrome::set_slide_y(window, frame.x, y);
                false
            })
            .unwrap_or(true);
        if cancelled || t >= 1.0 {
            break;
        }
        Timer::after(Duration::from_millis(8)).await;
    }
}
