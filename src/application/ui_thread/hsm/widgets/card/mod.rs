//! Card appearance, layout and internal editing controls.

pub mod calendar;
pub mod editor;
pub mod meta;
pub mod text_field;
pub(in crate::application::ui_thread::hsm) mod view;

use super::super::theme::Theme;
use super::card_section::COLUMN_PAD_X;
use crate::application::snapshot::{Card, Priority};
use gpui::{div, prelude::*, px, Rgba};

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
const TITLE_EM: f32 = 16.0;
const NOTE_EM: f32 = 14.0;

pub fn displayed_card_width(list_width: f32) -> f32 {
    (list_width - COLUMN_PAD_X * 2.0).max(0.0)
}

pub fn card_inner_width(list_width: f32) -> f32 {
    (displayed_card_width(list_width) - COLUMN_PAD_X * 2.0).max(0.0)
}

pub fn shared_list_width(column_list_widths: &[f32]) -> f32 {
    column_list_widths.iter().copied().fold(0.0_f32, f32::max)
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

pub fn drag_ghost_size(list_width: f32, body: f32) -> (f32, f32) {
    (displayed_card_width(list_width), body)
}

fn slot_chrome() -> gpui::Div {
    div().w_full().rounded_md().border_1().px_3().py_2()
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
    mark_tag(meta::priority_label(priority).to_string(), fg, bg)
}

pub fn render(card: &Card, theme: Theme) -> gpui::Div {
    let now_ms = meta::now_ms();
    let notes = card.notes.trim();
    let overdue = meta::is_overdue_for_card(card.deadline_at, card.overdue_cleared, now_ms);
    let mut marks = face_row(SLOT_MARKS)
        .overflow_hidden()
        .gap(px(6.))
        .child(priority_tag(card.priority, theme));
    if let Some(deadline) = meta::deadline_label(card.deadline_at) {
        let (fg, bg) = theme.deadline_tag();
        marks = marks.child(mark_tag(deadline, fg, bg));
    }
    if let Some(remain) =
        meta::remaining_label_for_card(card.deadline_at, card.overdue_cleared, now_ms)
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

pub fn placeholder(theme: Theme, body: f32) -> gpui::Div {
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
    fn slot_pitch_stays_taller_than_one_line() {
        assert!(SLOT_PITCH > SLOT_TITLE + SLOT_GAP);
        assert_eq!(
            SLOT_BODY,
            SLOT_PAD_Y * 2.0 + SLOT_BORDER_Y * 2.0 + SLOT_FACE
        );
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
