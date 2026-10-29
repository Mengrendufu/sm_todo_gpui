//! A retained column viewport; scrollbar capture belongs to this column only.

use super::{
    input::ColumnInput,
    scrollbar_thumb,
    state_machine::{CaptureToken, ColumnEvent, ColumnInteraction, DragGeometry},
};
use crate::application::snapshot::Status;
use crate::application::ui_thread::hsm::widgets::card::view::{card_element, CardView};
use crate::application::ui_thread::hsm::{status_key, theme::Theme, DragCard, SM_UI};
use crate::application::ui_thread::thread::{evt::UI_Evt, inbox::UI_Inbox_post};
use gpui::DragMoveEvent;
use gpui::{
    canvas, div, point, prelude::*, px, AnyView, Context, Entity, EntityId, MouseButton,
    MouseDownEvent, Render, ScrollHandle, StyleRefinement, WeakEntity, Window,
};
use rs_hsm_::SM_Hsm;

pub(in crate::application::ui_thread::hsm) struct ColumnView {
    owner: WeakEntity<SM_UI>,
    status: Status,
    scroll: ScrollHandle,
    input: ColumnInput,
    hsm: SM_Hsm<ColumnInteraction>,
    interaction: ColumnInteraction,
    pub(super) rows: Vec<(Entity<CardView>, f32)>,
    pub(super) offsets: Vec<f32>,
    pub(super) content_key: Vec<(EntityId, u64)>,
    pub(super) motion_key: (u64, u64, usize, usize),
    pub(super) appearance: Option<Theme>,
}

pub(in crate::application::ui_thread::hsm) fn column_element(view: &Entity<ColumnView>) -> AnyView {
    AnyView::from(view.clone()).cached(
        StyleRefinement::default()
            .flex_1()
            .min_h_0()
            .min_w(px(200.)),
    )
}

impl ColumnView {
    pub(super) fn new(
        owner: WeakEntity<SM_UI>,
        status: Status,
        scroll: ScrollHandle,
        input: ColumnInput,
    ) -> Self {
        let mut hsm = SM_Hsm::new();
        let mut interaction = ColumnInteraction::default();
        hsm.init(&mut interaction);
        Self {
            owner,
            status,
            scroll,
            input,
            hsm,
            interaction,
            rows: Vec::new(),
            offsets: vec![0.],
            content_key: Vec::new(),
            motion_key: (0, 0, 0, 0),
            appearance: None,
        }
    }

    pub(super) fn dispatch_routed(
        &mut self,
        token: CaptureToken,
        event: ColumnEvent,
        cx: &mut Context<Self>,
    ) {
        let effects = self
            .interaction
            .dispatch_routed(&mut self.hsm, token, event);
        if let Some(y) = effects.scroll {
            self.set_scroll(y, cx);
        }
        if let Some(token) = effects.finished {
            self.input.release(token);
        }
        if effects.started || effects.finished.is_some() {
            gpui::App::notify(cx, self.owner.entity_id());
            cx.notify();
        }
    }

    fn set_scroll(&mut self, y: f32, cx: &mut Context<Self>) {
        let offset = point(px(0.), px(-y));
        if self.scroll.offset() != offset {
            self.scroll.set_offset(offset);
            cx.notify();
        }
    }

    fn append_slot(&self) -> impl IntoElement {
        let owner = self.owner.clone();
        let status = self.status;
        div()
            .id(("append", status_key(status)))
            .flex_1()
            .w_full()
            .min_h(px(32.))
            .on_drop(move |info: &DragCard, window, cx| {
                owner
                    .update(cx, |owner, cx| {
                        let pointer = window.mouse_position();
                        owner.note_pointer(f32::from(pointer.x), f32::from(pointer.y));
                        UI_Inbox_post(UI_Evt::DropCard {
                            id: info.card.id.clone(),
                            status,
                            before: None,
                            pointer: Some((f32::from(pointer.x), f32::from(pointer.y))),
                        });
                        cx.notify();
                    })
                    .ok();
            })
    }

    fn content(
        &self,
        kids: Vec<gpui::AnyElement>,
        scrollbar: gpui::AnyElement,
        theme: Theme,
    ) -> impl IntoElement {
        let status = self.status;
        let scroll = self.scroll.clone();
        let title = match status {
            Status::Todo => "待办",
            Status::Doing => "进行中",
            Status::Done => "完成",
        };
        let count = self.rows.len();
        let drag_owner = self.owner.clone();
        let drop_owner = self.owner.clone();
        let list = div()
            .id(title)
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .px_3()
            .pb_3()
            .overflow_y_scroll()
            .track_scroll(&scroll)
            .on_drag_move(move |event: &DragMoveEvent<DragCard>, _, cx| {
                drag_owner
                    .update(cx, |this, cx| {
                        if this.dragging.is_none() {
                            return;
                        }
                        this.note_pointer(
                            f32::from(event.event.position.x),
                            f32::from(event.event.position.y),
                        );
                        if !event.bounds.contains(&event.event.position) {
                            return;
                        }
                        let local = event.event.position.y - event.bounds.origin.y;
                        let scrolled = this.column_scroll[status_key(status) as usize].offset().y;
                        let y = f32::from(local - scrolled);
                        this.note_drag_geometry(
                            f32::from(event.event.position.x),
                            f32::from(event.event.position.y),
                            f32::from(event.bounds.origin.x),
                            f32::from(event.bounds.origin.y),
                            f32::from(scrolled),
                        );
                        this.note_column_frame(
                            status,
                            f32::from(event.bounds.origin.x),
                            f32::from(event.bounds.origin.y),
                            f32::from(scrolled),
                        );
                        if this.hover_pointer(status, y) {
                            cx.notify();
                        }
                    })
                    .ok();
            })
            .children(kids)
            .child(self.append_slot());
        let body = div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .child(list)
            .child(scrollbar);
        super::render(title, count, body, theme)
            .drag_over::<DragCard>({
                let gold_soft = theme.gold_soft();
                move |style, _, _, _| style.bg(gold_soft)
            })
            .on_drop(move |info: &DragCard, window, cx| {
                drop_owner
                    .update(cx, |this, cx| {
                        let pointer = window.mouse_position();
                        this.note_pointer(f32::from(pointer.x), f32::from(pointer.y));
                        let hovered_here = this
                            .dragging
                            .as_ref()
                            .is_some_and(|d| d.to_status == status);
                        if hovered_here {
                            this.post_drop();
                        } else {
                            UI_Inbox_post(UI_Evt::DropCard {
                                id: info.card.id.clone(),
                                status,
                                before: None,
                                pointer: Some((f32::from(pointer.x), f32::from(pointer.y))),
                            });
                        }
                        cx.notify();
                    })
                    .ok();
            })
    }

    fn scrollbar(&self, theme: Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let viewport = f32::from(self.scroll.bounds().size.height);
        let max_scroll = f32::from(self.scroll.max_offset().height);
        let scroll_y = -f32::from(self.scroll.offset().y);
        let Some((top, thumb_h)) = scrollbar_thumb(viewport, max_scroll, scroll_y, viewport) else {
            return div().into_any_element();
        };
        let track_top = f32::from(self.scroll.bounds().origin.y);
        div()
            .absolute()
            .top_0()
            .right_0()
            .w(px(8.))
            .h_full()
            .occlude()
            .cursor_default()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |column, event: &MouseDownEvent, _, cx| {
                    let allowed = column
                        .owner
                        .read_with(cx, |owner, _| !owner.closing && owner.dragging.is_none())
                        .unwrap_or(false);
                    if !allowed {
                        return;
                    }
                    if let Some((_, token)) = column.input.pending_loss() {
                        column.input.post_capture_lost(token.0);
                    }
                    if !column.input.accepts(column.status) {
                        return;
                    }
                    let y = f32::from(event.position.y) - track_top;
                    if y >= top && y <= top + thumb_h {
                        if let Some(token) = column.input.claim(column.status) {
                            column.input.post(
                                column.status,
                                token,
                                ColumnEvent::ThumbPressed {
                                    token,
                                    geometry: DragGeometry {
                                        track_top,
                                        grab: y - top,
                                        viewport,
                                        max_scroll,
                                    },
                                    offset: -f32::from(column.scroll.offset().y),
                                },
                            );
                        }
                    } else {
                        column.input.post(
                            column.status,
                            CaptureToken::default(),
                            ColumnEvent::TrackPressed {
                                top: y - thumb_h / 2.,
                                viewport,
                                max_scroll,
                            },
                        );
                    }
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .absolute()
                    .top(px(top))
                    .right(px(1.))
                    .w(px(6.))
                    .h(px(thumb_h))
                    .rounded(px(3.))
                    .bg(theme.line_strong()),
            )
            .into_any_element()
    }
}

impl Render for ColumnView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let scrollbar = self.scrollbar(theme, cx);
        let status = self.status;
        let kids = if self.motion_key.2 > 0 || self.motion_key.3 > 0 {
            self.owner
                .update(cx, |owner, cx| {
                    owner.column_animation_children(status, theme, cx)
                })
                .unwrap_or_default()
        } else {
            let visible = super::visible_rows(
                &self.offsets,
                -f32::from(self.scroll.offset().y),
                f32::from(self.scroll.bounds().size.height),
            );
            let mut kids = Vec::new();
            if self.offsets[visible.start] > 0. {
                kids.push(super::spacer(self.offsets[visible.start]));
            }
            for (view, body) in &self.rows[visible.clone()] {
                kids.push(card_element(view, *body).into_any_element());
            }
            let tail = self.offsets.last().copied().unwrap_or(0.) - self.offsets[visible.end];
            if tail > 0. {
                kids.push(super::spacer(tail));
            }
            kids
        };
        let content = self.content(kids, scrollbar, theme);
        let geometry = (self.scroll.bounds(), self.scroll.max_offset());
        let scroll = self.scroll.clone();
        let target = cx.entity().downgrade();

        let owner = self.owner.clone();
        div()
            .flex()
            .flex_col()
            .size_full()
            .flex_1()
            .min_h_0()
            .min_w(px(200.))
            .child(content)
            .child(
                canvas(
                    move |_, _, cx| {
                        let next = (scroll.bounds(), scroll.max_offset());
                        if next != geometry {
                            target
                                .update(cx, |column, _cx| {
                                    column.input.viewport_changed(column.status)
                                })
                                .ok();
                            cx.notify(target.entity_id());
                            if next.0.size.width != geometry.0.size.width {
                                cx.notify(owner.entity_id());
                            }
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute(),
            )
    }
}
