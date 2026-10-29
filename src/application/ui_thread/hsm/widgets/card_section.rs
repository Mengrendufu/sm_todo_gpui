//! Card sections: retained column views, visible rows and drag targets.

pub(in crate::application::ui_thread) mod input;
mod state_machine;
mod view;
pub(in crate::application::ui_thread::hsm) use view::{column_element, ColumnView};

use super::super::theme::Theme;
use crate::application::snapshot::Status;
use crate::application::ui_thread::hsm::{
    status_key, theme, ColumnItem, HolePhase, UI_Controls, HOLE_ANIM_MS, SM_UI,
};
use gpui::{div, prelude::*, px, Context};

pub const COLUMN_PAD_X: f32 = 12.0;
pub const SCROLLBAR_MIN_THUMB: f32 = 24.0;
const ROW_OVERDRAW: f32 = 64.0;

#[derive(Debug, Default, PartialEq, Eq)]
struct ColumnMotion {
    holes: Vec<(u64, HolePhase, Option<String>)>,
    slides: Vec<(u64, String)>,
}

impl ColumnMotion {
    fn is_active(&self) -> bool {
        !self.holes.is_empty() || !self.slides.is_empty()
    }
}

pub fn scrollbar_thumb(
    viewport: f32,
    max_scroll: f32,
    scroll_y: f32,
    track: f32,
) -> Option<(f32, f32)> {
    if viewport <= 0.0 || track <= 0.0 || max_scroll <= 0.0 {
        return None;
    }
    let content = viewport + max_scroll;
    let thumb_h = (viewport / content * track)
        .max(SCROLLBAR_MIN_THUMB)
        .min(track);
    let max_top = (track - thumb_h).max(0.0);
    let t = (scroll_y / max_scroll).clamp(0.0, 1.0);
    Some((t * max_top, thumb_h))
}

pub fn scroll_y_from_thumb_top(thumb_top: f32, viewport: f32, max_scroll: f32, track: f32) -> f32 {
    let Some((_, thumb_h)) = scrollbar_thumb(viewport, max_scroll, 0.0, track) else {
        return 0.0;
    };
    let max_top = (track - thumb_h).max(0.0);
    if max_top <= 0.0 {
        return 0.0;
    }
    (thumb_top / max_top).clamp(0.0, 1.0) * max_scroll
}

pub fn render(
    title: &'static str,
    count: usize,
    list: impl IntoElement,
    theme: Theme,
) -> gpui::Div {
    div()
        .flex()
        .flex_col()
        .flex_1()
        .min_h_0()
        .min_w(px(200.))
        .rounded_xl()
        .bg(theme.bg())
        .border_1()
        .border_color(theme.line())
        .child(
            div()
                .flex()
                .flex_none()
                .items_center()
                .justify_between()
                .px_4()
                .py_3()
                .child(div().text_color(theme.ink_soft()).child(title.to_string()))
                .child(
                    div()
                        .px(px(7.))
                        .rounded(px(999.))
                        .bg(theme.fill())
                        .text_color(theme.ink_faint())
                        .child(format!("{count}")),
                ),
        )
        .child(list)
}

fn row_offsets(heights: &[f32]) -> Vec<f32> {
    let mut offsets = Vec::with_capacity(heights.len() + 1);
    offsets.push(0.);
    for height in heights {
        offsets.push(offsets.last().copied().unwrap() + height);
    }
    offsets
}

fn visible_rows(offsets: &[f32], top: f32, viewport: f32) -> std::ops::Range<usize> {
    let count = offsets.len().saturating_sub(1);
    if viewport <= 0. {
        return 0..count.min(1);
    }
    let start = offsets
        .partition_point(|y| *y <= (top - ROW_OVERDRAW).max(0.))
        .saturating_sub(1)
        .min(count);
    let end = offsets
        .partition_point(|y| *y < top + viewport + ROW_OVERDRAW)
        .min(count);
    start..end.max(start)
}

fn spacer(height: f32) -> gpui::AnyElement {
    div().w_full().h(px(height)).flex_none().into_any_element()
}

impl UI_Controls {
    fn column_motion(&self, status: Status) -> ColumnMotion {
        ColumnMotion {
            holes: self
                .holes
                .iter()
                .filter(|hole| hole.status == status)
                .map(|hole| (hole.epoch, hole.phase, hole.before_id.clone()))
                .collect(),
            slides: self
                .slides
                .iter()
                .filter(|slide| slide.status == status)
                .map(|slide| (slide.epoch, slide.id.clone()))
                .collect(),
        }
    }

    pub(in crate::application::ui_thread::hsm) fn sync_column_views(
        &mut self,
        theme: theme::Theme,
        input: &input::ColumnInput,
        cx: &mut Context<SM_UI>,
    ) {
        for status in [Status::Todo, Status::Doing, Status::Done] {
            let index = status_key(status) as usize;
            let content = self
                .snapshot
                .cards
                .iter()
                .filter(|card| card.status == status)
                .map(|card| {
                    let entry = &self.card_views[&card.id];
                    (entry.view.entity_id(), entry.revision)
                })
                .collect::<Vec<_>>();
            let motion = self.column_motion(status);
            if self.column_views[index].is_none() {
                let owner = cx.entity().downgrade();
                self.column_views[index] = Some(cx.new(|_| {
                    ColumnView::new(
                        owner,
                        status,
                        self.column_scroll[index].clone(),
                        input.clone(),
                    )
                }));
            }
            input.register(
                status,
                self.column_views[index].as_ref().unwrap().downgrade(),
            );
            self.column_views[index]
                .as_ref()
                .unwrap()
                .update(cx, |column, cx| {
                    let content_changed = column.content_key != content;
                    let motion_changed = column.motion_key != motion;
                    let appearance_changed = column.appearance != Some(theme);
                    if content_changed {
                        column.rows = self
                            .snapshot
                            .cards
                            .iter()
                            .filter(|card| card.status == status)
                            .map(|card| {
                                let entry = &self.card_views[&card.id];
                                (entry.view.clone(), entry.body)
                            })
                            .collect();
                        column.offsets = row_offsets(
                            &column
                                .rows
                                .iter()
                                .map(|(_, body)| body + super::card::SLOT_GAP)
                                .collect::<Vec<_>>(),
                        );
                        column.content_key = content;
                    }
                    if motion_changed {
                        column.motion_key = motion;
                    }
                    if appearance_changed {
                        column.appearance = Some(theme);
                    }
                    if content_changed || motion_changed || appearance_changed {
                        ColumnView::notify_column(cx);
                    }
                });
        }
    }

    pub(in crate::application::ui_thread::hsm) fn column_animation_children(
        &self,
        status: Status,
        theme: theme::Theme,
        cx: &mut Context<SM_UI>,
    ) -> Vec<gpui::AnyElement> {
        let items = self.column_items(status);
        let scroll = &self.column_scroll[status_key(status) as usize];
        let heights = items
            .iter()
            .map(|item| match item {
                ColumnItem::Card(card) => self.card_views[&card.id].body + super::card::SLOT_GAP,
                ColumnItem::Hole { epoch, phase } => {
                    let elapsed = self
                        .holes
                        .iter()
                        .find(|hole| hole.epoch == *epoch && hole.status == status)
                        .map(|hole| hole.started.elapsed().as_secs_f32())
                        .unwrap_or(0.);
                    let progress =
                        gpui::ease_in_out((elapsed / (HOLE_ANIM_MS as f32 / 1000.)).min(1.));
                    crate::application::ui_thread::hsm::hole_box(
                        *phase,
                        progress,
                        self.drag_pitch(),
                    )
                    .height
                }
            })
            .collect::<Vec<_>>();
        let prefix = row_offsets(&heights);
        let visible = visible_rows(
            &prefix,
            -f32::from(scroll.offset().y),
            f32::from(scroll.bounds().size.height),
        );
        let mut kids = Vec::new();
        let mut skipped = 0.;
        for (index, item) in items.into_iter().enumerate() {
            // Keep animated holes alive; only card rows are culled.
            if matches!(item, ColumnItem::Card(_)) && !visible.contains(&index) {
                skipped += heights[index];
                continue;
            }
            if skipped > 0. {
                kids.push(spacer(skipped));
                skipped = 0.;
            }
            kids.push(match item {
                ColumnItem::Hole { epoch, phase } => self
                    .hole_element(epoch, phase, theme, cx)
                    .into_any_element(),
                ColumnItem::Card(card) => self.card_views[&card.id].element().into_any_element(),
            });
        }
        if skipped > 0. {
            kids.push(spacer(skipped));
        }
        for overlay in self.squeeze_overlays_in(status, theme) {
            kids.push(overlay);
        }
        kids
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_motion_stays_in_affected_columns_and_duplicate_targets_are_noops() {
        use crate::application::{data_manager::DataManager, snapshot::Snapshot};
        let (database, cards) = DataManager::open_in_memory(|_| {}).unwrap();
        let mut controls = UI_Controls::new(Snapshot::new(cards), database.client());
        controls.add_todo_card("A");
        controls.add_todo_card("B");
        let id = controls.snapshot.cards[0].id.clone();
        let untouched = controls.column_motion(Status::Done);

        controls.start_drag(&id);
        let source = controls.column_motion(Status::Todo);
        assert!(source.is_active());
        assert!(!controls.column_motion(Status::Doing).is_active());
        assert_eq!(controls.column_motion(Status::Done), untouched);

        assert!(controls.hover_insert(Status::Todo, None));
        assert_ne!(controls.column_motion(Status::Todo), source);
        assert_eq!(controls.column_motion(Status::Done), untouched);

        assert!(controls.hover_insert(Status::Doing, None));
        assert!(controls.column_motion(Status::Todo).is_active());
        let target = controls.column_motion(Status::Doing);
        assert!(target.is_active());
        assert_eq!(controls.column_motion(Status::Done), untouched);
        assert!(!controls.hover_insert(Status::Doing, None));
        assert_eq!(controls.column_motion(Status::Doing), target);

        controls.cancel_drag();
        assert!(
            controls.column_motion(Status::Doing).is_active(),
            "closing animation must remain active after leaving"
        );
        assert_eq!(controls.column_motion(Status::Done), untouched);
        for hole in &mut controls.holes {
            hole.started =
                std::time::Instant::now() - std::time::Duration::from_millis(HOLE_ANIM_MS + 1);
        }
        controls.flush();
        assert!(!controls.column_motion(Status::Doing).is_active());
        assert_eq!(controls.column_motion(Status::Done), untouched);
    }

    #[test]
    fn viewport_selects_only_intersecting_rows_with_a_small_overdraw() {
        let offsets = row_offsets(&[100.; 10]);
        assert_eq!(visible_rows(&offsets, 500., 100.), 4..7);
        assert_eq!(visible_rows(&offsets, 0., 100.), 0..2);
        assert_eq!(visible_rows(&offsets, 900., 100.), 8..10);
    }

    #[test]
    fn a_tall_card_stays_visible_when_the_viewport_is_inside_it() {
        let offsets = row_offsets(&[70., 900., 40., 110.]);
        assert_eq!(visible_rows(&offsets, 500., 200.), 1..2);
        assert_eq!(*offsets.last().unwrap(), 1120.);
    }

    #[test]
    fn an_empty_column_has_no_virtual_rows() {
        assert_eq!(visible_rows(&row_offsets(&[]), 0., 500.), 0..0);
    }

    #[test]
    fn an_unmeasured_viewport_bootstraps_without_building_the_entire_list() {
        assert_eq!(visible_rows(&row_offsets(&[90.; 1000]), 0., 0.), 0..1);
    }

    #[test]
    fn no_thumb_when_content_fits() {
        assert_eq!(scrollbar_thumb(400.0, 0.0, 0.0, 400.0), None);
        assert_eq!(scrollbar_thumb(400.0, -10.0, 0.0, 400.0), None);
    }

    #[test]
    fn thumb_scales_to_viewport_share_of_content() {
        let (top, h) = scrollbar_thumb(100.0, 100.0, 0.0, 100.0).unwrap();
        assert_eq!(top, 0.0);
        assert_eq!(h, 50.0);
    }

    #[test]
    fn thumb_moves_with_scroll() {
        let (_, h) = scrollbar_thumb(100.0, 100.0, 0.0, 100.0).unwrap();
        let (top, h2) = scrollbar_thumb(100.0, 100.0, 100.0, 100.0).unwrap();
        assert_eq!(h, h2);
        assert_eq!(top, 100.0 - h);
    }

    #[test]
    fn thumb_click_maps_back_to_scroll() {
        let max_scroll = 200.0;
        let viewport = 100.0;
        let track = 100.0;
        let (top, _) = scrollbar_thumb(viewport, max_scroll, 80.0, track).unwrap();
        let y = scroll_y_from_thumb_top(top, viewport, max_scroll, track);
        assert!((y - 80.0).abs() < 0.01);
    }
}
