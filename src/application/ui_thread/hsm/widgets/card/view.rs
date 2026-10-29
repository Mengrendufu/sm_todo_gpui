//! Retained card widgets. Scrolling changes visibility, not card identity.
//! Rendering reads local display state; only interaction callbacks access the HSM
//! owner, so an unrelated column update cannot invalidate this widget's cache.

use gpui::{
    div, prelude::*, px, AnyView, Context, Entity, Render, StyleRefinement, WeakEntity, Window,
};
use std::collections::HashSet;

use super::{meta, Card};
use crate::application::ui_thread::hsm::{
    hash_id, status_key, theme::Theme, DragCard, DragPreview, UI_Controls, SM_UI,
};
use crate::application::ui_thread::thread::{evt::UI_Evt, inbox::UI_Inbox_post};

pub(in crate::application::ui_thread::hsm) struct CardView {
    card: Card,
    owner: WeakEntity<SM_UI>,
    hidden: bool,
    body: f32,
}

impl Render for CardView {
    fn render(&mut self, window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let card = &self.card;
        let status = card.status;
        let id = card.id.clone();
        let payload = DragCard { card: card.clone() };
        let view = self.owner.clone();
        let drop_owner = self.owner.clone();
        let click_owner = self.owner.clone();
        let hidden = self.hidden;
        let body = self.body;
        let pitch = body + super::SLOT_GAP;
        let tile = div()
            .w_full()
            .flex_none()
            .h(px(body))
            .min_h(px(body))
            .opacity(if hidden { 0. } else { 1. })
            .child(super::render(card, theme));
        div()
            .id(("card", hash_id(&id)))
            .w_full()
            .flex_none()
            .h(px(pitch))
            .min_h(px(pitch))
            .overflow_hidden()
            .cursor_move()
            // GPUI's native active_drag still forces a global refresh per move.
            // Column-scoped notifications cannot override that framework path.
            .on_drag(payload, {
                let view = view.clone();
                move |info, offset, window, cx| {
                    let width = view
                        .update(cx, |this, cx| {
                            this.start_drag(&info.card.id);
                            this.set_grab_offset(f32::from(offset.x), f32::from(offset.y));
                            this.note_pointer(
                                f32::from(window.mouse_position().x),
                                f32::from(window.mouse_position().y),
                            );
                            let this = &mut this.controls;
                            this.note_column_frame(
                                status,
                                f32::from(
                                    this.column_scroll[status_key(status) as usize]
                                        .bounds()
                                        .origin
                                        .x,
                                ),
                                f32::from(
                                    this.column_scroll[status_key(status) as usize]
                                        .bounds()
                                        .origin
                                        .y,
                                ),
                                f32::from(
                                    this.column_scroll[status_key(status) as usize].offset().y,
                                ),
                            );
                            SM_UI::notify_global(cx);
                            this.ghost_size_for(status).0
                        })
                        .unwrap_or(0.0);
                    cx.new(|_| DragPreview {
                        card: info.card.clone(),
                        width,
                    })
                }
            })
            .on_drop(move |_: &DragCard, window, cx| {
                drop_owner
                    .update(cx, |this, cx| {
                        let pointer = window.mouse_position();
                        this.note_pointer(f32::from(pointer.x), f32::from(pointer.y));
                        this.post_drop();
                        SM_UI::notify_global(cx);
                    })
                    .ok();
            })
            .on_click({
                let id = id.clone();
                move |_, _, cx| {
                    click_owner
                        .update(cx, |this, cx| {
                            if this.dragging.is_some() {
                                return;
                            }
                            UI_Inbox_post(UI_Evt::EditCard { id: id.clone() });
                            SM_UI::notify_global(cx);
                        })
                        .ok();
                }
            })
            .child(tile)
    }
}

pub(in crate::application::ui_thread::hsm) struct CardEntry {
    pub view: Entity<CardView>,
    pub body: f32,
    pub revision: u64,
    width: f32,
    hidden: bool,
    theme: Theme,
    remaining: Option<String>,
    overdue: bool,
}

impl CardEntry {
    pub fn element(&self) -> AnyView {
        card_element(&self.view, self.body)
    }
}

pub(in crate::application::ui_thread::hsm) fn card_element(
    view: &Entity<CardView>,
    body: f32,
) -> AnyView {
    AnyView::from(view.clone()).cached(
        StyleRefinement::default()
            .w_full()
            .h(px(body + super::SLOT_GAP))
            .flex_none(),
    )
}

impl UI_Controls {
    pub(in crate::application::ui_thread::hsm) fn sync_card_views(
        &mut self,
        theme: Theme,
        cx: &mut Context<SM_UI>,
    ) {
        let width = self.card_inner_width(crate::application::snapshot::Status::Todo);
        let now = meta::now_ms();
        let owner = cx.entity().downgrade();
        for card in &self.snapshot.cards {
            let hidden = self.card_is_hidden(&card.id);
            let remaining =
                meta::remaining_label_for_card(card.deadline_at, card.overdue_cleared, now);
            let overdue = meta::is_overdue_for_card(card.deadline_at, card.overdue_cleared, now);
            if let Some(entry) = self.card_views.get_mut(&card.id) {
                let data_changed = self.cards_dirty && entry.view.read(cx).card != *card;
                let resized = entry.width != width;
                if data_changed
                    || resized
                    || entry.hidden != hidden
                    || entry.theme != theme
                    || entry.remaining != remaining
                    || entry.overdue != overdue
                {
                    if data_changed || resized {
                        entry.body = super::card_body_height(&card.title, &card.notes, width);
                    }
                    if data_changed {
                        entry.view.update(cx, |view, _| view.card = card.clone());
                    }
                    entry.width = width;
                    entry.hidden = hidden;
                    entry.theme = theme;
                    entry.remaining = remaining;
                    entry.overdue = overdue;
                    entry.revision = entry.revision.wrapping_add(1);
                    entry.view.update(cx, |view, cx| {
                        view.hidden = hidden;
                        view.body = entry.body;
                        cx.notify();
                    });
                }
            } else {
                let body = super::card_body_height(&card.title, &card.notes, width);
                self.card_views.insert(
                    card.id.clone(),
                    CardEntry {
                        view: cx.new(|_| CardView {
                            card: card.clone(),
                            owner: owner.clone(),
                            hidden,
                            body,
                        }),
                        body,
                        revision: 0,
                        width,
                        hidden,
                        theme,
                        remaining,
                        overdue,
                    },
                );
            }
        }
        if self.cards_dirty {
            let live_ids = self
                .snapshot
                .cards
                .iter()
                .map(|card| card.id.as_str())
                .collect::<HashSet<_>>();
            self.card_views
                .retain(|id, _| live_ids.contains(id.as_str()));
            self.cards_dirty = false;
        }
    }
}
