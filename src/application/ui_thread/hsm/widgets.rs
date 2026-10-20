//! Passive board projections. No events, no authority.

use gpui::{div, prelude::*, px, rgb, Rgba, WindowControlArea};
use crate::application::data::{Card, Priority};

use super::{card_meta, theme::Theme};

/// Title line: rem 16 × φ, rounded. Same as gpui default line-height.
pub const SLOT_TITLE: f32 = 26.0;
/// Notes preview: ~text_sm line box, one clamped line (fixed pitch).
pub const SLOT_NOTE: f32 = 22.0;
/// Tauri `.card p` margin-top.
pub const SLOT_NOTE_GAP: f32 = 6.0;
/// Priority / deadline / remaining pills.
pub const SLOT_MARKS: f32 = 20.0;
/// Tauri `.card-marks` margin-top.
pub const SLOT_MARKS_GAP: f32 = 10.0;
/// Title + marks. Notes are extra and may wrap.
pub const SLOT_FACE: f32 = SLOT_TITLE + SLOT_MARKS_GAP + SLOT_MARKS;
/// `py_2` / `gap_2` = 0.5rem at 16px rem.
pub const SLOT_GAP: f32 = 8.0;
pub const SLOT_PAD_Y: f32 = 8.0;
pub const SLOT_BORDER_Y: f32 = 1.0;
/// Card chrome: pad + border + face. Must match `CardTile` / `Hole` box.
pub const SLOT_BODY: f32 = SLOT_PAD_Y * 2.0 + SLOT_BORDER_Y * 2.0 + SLOT_FACE;
/// Flex item size: body plus the old column `gap_2`, so a 0-height hole leaves no gap.
pub const SLOT_PITCH: f32 = SLOT_BODY + SLOT_GAP;
/// Dead zone around a dest midpoint before the hole steps.
pub const INSERT_HYSTERESIS: f32 = 3.0;
/// Column list `px_3` (0.75rem at 16px).
pub const COLUMN_PAD_X: f32 = 12.0;
pub const SCROLLBAR_MIN_THUMB: f32 = 24.0;
const TITLE_EM: f32 = 16.0;
const NOTE_EM: f32 = 14.0;

/// Visible card width inside a column list (list inner width minus `px_3`).
pub fn displayed_card_width(list_width: f32) -> f32 {
    (list_width - COLUMN_PAD_X * 2.0).max(0.0)
}

pub fn card_inner_width(list_width: f32) -> f32 {
    (displayed_card_width(list_width) - COLUMN_PAD_X * 2.0).max(0.0)
}

/// All columns share the widest measured list so wrap/height stay aligned.
pub fn shared_list_width(column_list_widths: &[f32]) -> f32 {
    column_list_widths
        .iter()
        .copied()
        .fold(0.0_f32, f32::max)
}

pub fn shared_inner_width(column_list_widths: &[f32]) -> f32 {
    card_inner_width(shared_list_width(column_list_widths))
}

pub fn wrapped_line_count(text: &str, max_width: f32, em: f32) -> usize {
    if text.is_empty() {
        return 0;
    }
    let unbounded = max_width <= 0.0;
    let mut lines = 0usize;
    for para in text.split('\n') {
        if para.is_empty() {
            lines += 1;
            continue;
        }
        if unbounded {
            lines += 1;
            continue;
        }
        let mut x = 0.0;
        let mut n = 1usize;
        for ch in para.chars() {
            let w = glyph_width(ch, em);
            if x > 0.0 && x + w > max_width {
                n += 1;
                x = w;
            } else {
                x += w;
            }
        }
        lines += n;
    }
    lines
}

fn glyph_width(ch: char, em: f32) -> f32 {
    if ch.is_ascii() {
        em * 0.55
    } else {
        em
    }
}

pub fn card_body_height(title: &str, notes: &str, inner_width: f32) -> f32 {
    let title_h = wrapped_line_count(title, inner_width, TITLE_EM).max(1) as f32 * SLOT_TITLE;
    let notes = notes.trim();
    let notes_h = match wrapped_line_count(notes, inner_width, NOTE_EM) {
        0 => 0.0,
        n => SLOT_NOTE_GAP + n as f32 * SLOT_NOTE,
    };
    SLOT_PAD_Y * 2.0 + SLOT_BORDER_Y * 2.0 + title_h + notes_h + SLOT_MARKS_GAP + SLOT_MARKS
}

pub fn card_pitch(title: &str, notes: &str, inner_width: f32) -> f32 {
    card_body_height(title, notes, inner_width) + SLOT_GAP
}

/// Drag / settle ghost matches the on-screen card face, not intrinsic content size.
pub fn drag_ghost_size(list_width: f32, body: f32) -> (f32, f32) {
    (displayed_card_width(list_width), body)
}

/// `(thumb_top, thumb_height)` in the same units as `track`.
/// `scroll_y` is the positive distance from the top (`-offset.y` in gpui).
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

pub fn scroll_y_from_thumb_top(
    thumb_top: f32,
    viewport: f32,
    max_scroll: f32,
    track: f32,
) -> f32 {
    let Some((_, thumb_h)) = scrollbar_thumb(viewport, max_scroll, 0.0, track) else {
        return 0.0;
    };
    let max_top = (track - thumb_h).max(0.0);
    if max_top <= 0.0 {
        return 0.0;
    }
    (thumb_top / max_top).clamp(0.0, 1.0) * max_scroll
}

pub fn traffic_light(
    id: &'static str,
    area: Option<WindowControlArea>,
    color: u32,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .w(px(22.))
        .h(px(22.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .when_some(area, |el, area| el.window_control_area(area))
        .child(
            div()
                .size(px(12.))
                .rounded(px(6.))
                .bg(rgb(color)),
        )
}

pub fn titlebar_drag(id: &'static str) -> impl gpui::IntoElement {
    div()
        .id(id)
        .flex_1()
        .h_full()
        .min_w(px(8.))
        .window_control_area(WindowControlArea::Drag)
}

/// Full-width overlay so the whole title string sits on the window midline,
/// not in leftover space between left/right chrome.
pub fn titlebar_window_title(name: &'static str, theme: Theme) -> impl gpui::IntoElement {
    div()
        .id("titlebar-name")
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(theme.gold())
        .child(name)
}

pub fn Column(
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
                .child(
                    div()
                        .text_color(theme.ink_soft())
                        .child(title.to_string()),
                )
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

fn slot_chrome() -> gpui::Div {
    div()
        .w_full()
        .rounded_md()
        .border_1()
        .px_3()
        .py_2()
}

fn face_row(height: f32) -> gpui::Div {
    div()
        .h(px(height))
        .min_h(px(height))
        .flex_none()
        .flex()
        .items_center()
}

fn mark_tag(label: String, fg: Rgba, bg: Rgba) -> gpui::Div {
    div()
        .h(px(SLOT_MARKS))
        .flex()
        .items_center()
        .px(px(8.))
        .rounded(px(999.))
        .text_xs()
        .text_color(fg)
        .bg(bg)
        .child(label)
}

fn priority_tag(priority: Priority, theme: Theme) -> gpui::Div {
    let (fg, bg) = theme.priority_tag(priority);
    mark_tag(card_meta::priority_label(priority).to_string(), fg, bg)
}

pub fn CardTile(card: &Card, theme: Theme) -> gpui::Div {
    let now_ms = card_meta::now_ms();
    let notes = card.notes.trim();
    let overdue = card_meta::is_overdue_for_card(card.deadline_at, card.overdue_cleared, now_ms);
    let mut marks = face_row(SLOT_MARKS)
        .overflow_hidden()
        .gap(px(6.))
        .child(priority_tag(card.priority, theme));
    if let Some(deadline) = card_meta::deadline_label(card.deadline_at) {
        let (fg, bg) = theme.deadline_tag();
        marks = marks.child(mark_tag(deadline, fg, bg));
    }
    if let Some(remain) =
        card_meta::remaining_label_for_card(card.deadline_at, card.overdue_cleared, now_ms)
    {
        let (fg, bg) = theme.remain_tag(overdue);
        marks = marks.child(mark_tag(remain, fg, bg));
    }
    slot_chrome()
        .bg(theme.card())
        .border_color(theme.line())
        .flex()
        .flex_col()
        .flex_none()
        .child(
            div()
                .w_full()
                .text_size(px(TITLE_EM))
                .line_height(px(SLOT_TITLE))
                .whitespace_normal()
                .child(card.title.clone()),
        )
        .children(if notes.is_empty() {
            None
        } else {
            Some(
                div()
                    .mt(px(SLOT_NOTE_GAP))
                    .w_full()
                    .text_sm()
                    .line_height(px(SLOT_NOTE))
                    .text_color(theme.ink_soft())
                    .whitespace_normal()
                    .child(notes.to_string()),
            )
        })
        .child(face_row(SLOT_MARKS_GAP))
        .child(marks)
}

pub fn Hole(theme: Theme, body: f32) -> gpui::Div {
    slot_chrome()
        .bg(theme.fill())
        .border_color(theme.line_strong())
        .flex_none()
        .min_h_0()
        .h(px(body))
        .child(
            div()
                .h(px((body - SLOT_PAD_Y * 2.0 - SLOT_BORDER_Y * 2.0).max(0.0)))
                .flex_none(),
        )
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

    #[test]
    fn slot_pitch_stays_taller_than_one_line() {
        assert!(SLOT_PITCH > SLOT_TITLE + SLOT_GAP);
        assert_eq!(SLOT_BODY, SLOT_PAD_Y * 2.0 + SLOT_BORDER_Y * 2.0 + SLOT_FACE);
    }

    #[test]
    fn drag_ghost_matches_displayed_card_face_not_content() {
        let list_width = 360.0;
        let body = card_body_height("A", "", 400.0);
        let (w, h) = drag_ghost_size(list_width, body);
        assert_eq!(w, displayed_card_width(list_width));
        assert_eq!(w, list_width - 2.0 * COLUMN_PAD_X);
        assert_eq!(h, body);
        assert_ne!(h, body + SLOT_GAP);
    }

    #[test]
    fn displayed_card_width_strips_column_padding() {
        assert_eq!(displayed_card_width(24.0), 0.0);
        assert_eq!(displayed_card_width(12.0), 0.0);
    }

    #[test]
    fn empty_notes_do_not_reserve_a_notes_row() {
        let empty = card_body_height("Title", "", 400.0);
        let with_note = card_body_height("Title", "note", 400.0);
        assert_eq!(empty, SLOT_BODY);
        assert!(empty < with_note);
        assert_eq!(with_note - empty, SLOT_NOTE_GAP + SLOT_NOTE);
    }

    #[test]
    fn explicit_newlines_add_note_lines() {
        assert_eq!(wrapped_line_count("a\nb", 400.0, NOTE_EM), 2);
        let one = card_body_height("T", "a", 400.0);
        let two = card_body_height("T", "a\nb", 400.0);
        assert_eq!(two - one, SLOT_NOTE);
    }

    #[test]
    fn narrow_width_wraps_notes_taller() {
        let notes = "这段备注需要折行才能完全显示出来";
        let wide = card_body_height("T", notes, 400.0);
        let narrow = card_body_height("T", notes, 64.0);
        assert!(narrow > wide);
    }

    #[test]
    fn unmeasured_column_shares_the_widest_list() {
        let notes = "这段备注需要折行才能完全显示出来";
        let shared = shared_inner_width(&[88.0, 88.0, 0.0]);
        assert_eq!(shared, card_inner_width(88.0));
        assert_eq!(
            card_body_height("T", notes, shared),
            card_body_height("T", notes, card_inner_width(88.0))
        );
        assert!(card_body_height("T", notes, shared) > card_body_height("T", notes, 0.0));
    }
}
