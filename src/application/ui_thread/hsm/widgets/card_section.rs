//! Card sections: column appearance, scrolling and drag targets.

use super::super::theme::Theme;
use crate::application::snapshot::Status;
use crate::application::ui_thread::hsm::{
    status_key, theme, ColumnItem, DragCard, ScrollbarDrag, UI_Controls, SM_UI,
};
use gpui::{div, point, prelude::*, px, Context, DragMoveEvent, MouseButton, MouseDownEvent};

pub const COLUMN_PAD_X: f32 = 12.0;
pub const SCROLLBAR_MIN_THUMB: f32 = 24.0;

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

impl UI_Controls {
    pub(in crate::application::ui_thread::hsm) fn move_scrollbar_drag(
        &mut self,
        pointer_y: f32,
    ) -> bool {
        let Some(drag) = self.scrollbar_drag.as_ref() else {
            return false;
        };
        let thumb_top = pointer_y - drag.track_top - drag.grab;
        let scroll_y =
            scroll_y_from_thumb_top(thumb_top, drag.viewport, drag.max_scroll, drag.track_h);
        let status = drag.status;
        self.column_scroll[status_key(status) as usize].set_offset(point(px(0.), px(-scroll_y)));
        true
    }

    pub(in crate::application::ui_thread::hsm) fn column_scrollbar(
        &self,
        status: Status,
        theme: theme::Theme,
        cx: &mut Context<SM_UI>,
    ) -> impl IntoElement {
        let scroll = &self.column_scroll[status_key(status) as usize];
        let viewport = f32::from(scroll.bounds().size.height);
        let max_scroll = f32::from(scroll.max_offset().height);
        let scroll_y = -f32::from(scroll.offset().y);
        let Some((top, thumb_h)) = scrollbar_thumb(viewport, max_scroll, scroll_y, viewport) else {
            return div().into_any_element();
        };
        let track_top = f32::from(scroll.bounds().origin.y);
        div()
            .id(("col-scroll", status_key(status)))
            .absolute()
            .top_0()
            .right_0()
            .w(px(8.))
            .h_full()
            .occlude()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    let y = f32::from(ev.position.y) - track_top;
                    if y >= top && y <= top + thumb_h {
                        this.scrollbar_drag = Some(ScrollbarDrag {
                            status,
                            track_top,
                            track_h: viewport,
                            grab: (y - top).clamp(0.0, thumb_h),
                            viewport,
                            max_scroll,
                        });
                    } else {
                        let scroll_y = scroll_y_from_thumb_top(
                            y - thumb_h / 2.0,
                            viewport,
                            max_scroll,
                            viewport,
                        );
                        this.column_scroll[status_key(status) as usize]
                            .set_offset(point(px(0.), px(-scroll_y)));
                    }
                    cx.notify();
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

    pub(in crate::application::ui_thread::hsm) fn render_card_section(
        &self,
        title: &'static str,
        status: Status,
        theme: theme::Theme,
        cx: &mut Context<SM_UI>,
    ) -> impl IntoElement {
        let count = self.cards_in(status).len();
        let mut kids = Vec::new();
        for item in self.column_items(status) {
            kids.push(match item {
                ColumnItem::Hole { epoch, phase } => self
                    .hole_element(epoch, phase, theme, cx)
                    .into_any_element(),
                ColumnItem::Card(card) => self
                    .card_element(card, status, theme, cx)
                    .into_any_element(),
            });
        }
        kids.push(self.append_slot(status, cx).into_any_element());
        for overlay in self.squeeze_overlays_in(status, theme) {
            kids.push(overlay);
        }
        let scroll = self.column_scroll[status_key(status) as usize].clone();
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
            .on_drag_move(
                cx.listener(move |this, event: &DragMoveEvent<DragCard>, _, cx| {
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
                }),
            )
            .children(kids);
        let body = div()
            .relative()
            .flex()
            .flex_1()
            .min_h_0()
            .child(list)
            .child(self.column_scrollbar(status, theme, cx));
        render(title, count, body, theme)
            .drag_over::<DragCard>({
                let gold_soft = theme.gold_soft();
                move |style, _, _, _| style.bg(gold_soft)
            })
            .on_drop(cx.listener(move |this, info: &DragCard, window, cx| {
                let pointer = window.mouse_position();
                this.note_pointer(f32::from(pointer.x), f32::from(pointer.y));
                let hovered_here = this
                    .dragging
                    .as_ref()
                    .is_some_and(|d| d.to_status == status);
                if hovered_here {
                    this.commit_drop();
                } else {
                    this.drop_card(&info.card.id, status, None);
                }
                cx.notify();
            }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
