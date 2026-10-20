//! Editor text field: IME, clip to bounds, optional wrap + Enter.

use std::ops::Range;

use gpui::{
    actions, div, fill, point, prelude::*, px, relative, size, App, Bounds,
    ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler, Entity,
    EntityInputHandler, FocusHandle, Focusable, GlobalElementId, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, SharedString, Style,
    TextRun, UTF16Selection, UnderlineStyle, Window,
};

use crate::application::ui_thread::hsm::theme::Theme;

actions!(
    card_field,
    [
        Backspace,
        Delete,
        Left,
        Right,
        SelectLeft,
        SelectRight,
        SelectAll,
        Home,
        End,
        Paste,
        Cut,
        Copy,
        InsertNewline
    ]
);

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, Some("CardField")),
        KeyBinding::new("delete", Delete, Some("CardField")),
        KeyBinding::new("left", Left, Some("CardField")),
        KeyBinding::new("right", Right, Some("CardField")),
        KeyBinding::new("shift-left", SelectLeft, Some("CardField")),
        KeyBinding::new("shift-right", SelectRight, Some("CardField")),
        KeyBinding::new("home", Home, Some("CardField")),
        KeyBinding::new("end", End, Some("CardField")),
        KeyBinding::new("ctrl-a", SelectAll, Some("CardField")),
        KeyBinding::new("ctrl-v", Paste, Some("CardField")),
        KeyBinding::new("ctrl-c", Copy, Some("CardField")),
        KeyBinding::new("ctrl-x", Cut, Some("CardField")),
        KeyBinding::new("cmd-a", SelectAll, Some("CardField")),
        KeyBinding::new("cmd-v", Paste, Some("CardField")),
        KeyBinding::new("cmd-c", Copy, Some("CardField")),
        KeyBinding::new("cmd-x", Cut, Some("CardField")),
        KeyBinding::new("enter", InsertNewline, Some("CardField")),
    ]);
}

pub fn sanitize_paste(text: &str, multiline: bool) -> String {
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    if multiline {
        text
    } else {
        text.replace('\n', " ")
    }
}

pub struct TextField {
    focus_handle: FocusHandle,
    content: SharedString,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    last_bounds: Option<Bounds<Pixels>>,
    is_selecting: bool,
    min_height: f32,
    multiline: bool,
    last_lines: Vec<gpui::WrappedLine>,
    last_starts: Vec<usize>,
    last_line_height: Pixels,
}

impl TextField {
    pub fn new(cx: &mut Context<Self>, placeholder: &str, min_height: f32, multiline: bool) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: "".into(),
            placeholder: placeholder.to_string().into(),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            last_bounds: None,
            is_selecting: false,
            min_height,
            multiline,
            last_lines: Vec::new(),
            last_starts: Vec::new(),
            last_line_height: px(20.),
        }
    }

    pub fn text(&self) -> &str {
        &self.content
    }

    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.content = text.to_string().into();
        let len = self.content.len();
        self.selected_range = len..len;
        self.selection_reversed = false;
        self.marked_range = None;
        cx.notify();
    }

    fn left(&mut self, _: &Left, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), cx);
        } else {
            self.move_to(self.selected_range.start, cx);
        }
    }

    fn right(&mut self, _: &Right, _: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), cx);
        } else {
            self.move_to(self.selected_range.end, cx);
        }
    }

    fn select_left(&mut self, _: &SelectLeft, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), cx);
    }

    fn select_right(&mut self, _: &SelectRight, _: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), cx);
    }

    fn select_all(&mut self, _: &SelectAll, _: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, cx);
        self.select_to(self.content.len(), cx);
    }

    fn home(&mut self, _: &Home, _: &mut Window, cx: &mut Context<Self>) {
        let off = self.cursor_offset();
        let start = self.content[..off]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        self.move_to(start, cx);
    }

    fn end(&mut self, _: &End, _: &mut Window, cx: &mut Context<Self>) {
        let off = self.cursor_offset();
        let end = self.content[off..]
            .find('\n')
            .map(|i| off + i)
            .unwrap_or(self.content.len());
        self.move_to(end, cx);
    }

    fn insert_newline(&mut self, _: &InsertNewline, window: &mut Window, cx: &mut Context<Self>) {
        if self.multiline {
            self.replace_text_in_range(None, "\n", window, cx);
        }
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.previous_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.select_to(self.next_boundary(self.cursor_offset()), cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            self.replace_text_in_range(None, &sanitize_paste(&text, self.multiline), window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.is_selecting = true;
        if event.modifiers.shift {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        } else {
            self.move_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), cx);
        }
    }

    fn move_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.selected_range = offset..offset;
        cx.notify();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        if self.last_lines.is_empty() {
            return 0;
        }
        let local = point(position.x - bounds.left(), position.y - bounds.top());
        let line_h = self.last_line_height;
        let mut y = px(0.);
        for (i, line) in self.last_lines.iter().enumerate() {
            let h = line.size(line_h).height;
            let last = i + 1 == self.last_lines.len();
            if local.y <= y + h || last {
                let pos = point(local.x, (local.y - y).max(px(0.)));
                let idx = line
                    .closest_index_for_position(pos, line_h)
                    .unwrap_or_else(|idx| idx);
                let start = self.last_starts.get(i).copied().unwrap_or(0);
                return (start + idx).min(self.content.len());
            }
            y += h;
        }
        self.content.len()
    }

    fn select_to(&mut self, offset: usize, cx: &mut Context<Self>) {
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        cx.notify();
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        if offset == 0 {
            return 0;
        }
        self.content
            .char_indices()
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .char_indices()
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }
}

impl EntityInputHandler for TextField {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, _window: &mut Window, _cx: &mut Context<Self>) {
        self.marked_range = None;
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        self.selected_range = range.start + new_text.len()..range.start + new_text.len();
        self.marked_range.take();
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let range = range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .or(self.marked_range.clone())
            .unwrap_or(self.selected_range.clone());
        self.content =
            (self.content[0..range.start].to_owned() + new_text + &self.content[range.end..])
                .into();
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        self.selected_range = new_selected_range_utf16
            .as_ref()
            .map(|range_utf16| self.range_from_utf16(range_utf16))
            .map(|new_range| new_range.start + range.start..new_range.end + range.end)
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let idx = range.start;
        let line_h = self.last_line_height;
        let mut y = px(0.);
        for (i, line) in self.last_lines.iter().enumerate() {
            let start = self.last_starts.get(i).copied().unwrap_or(0);
            let end = start + line.len();
            let last = i + 1 == self.last_lines.len();
            if idx <= end || last {
                let local = idx.saturating_sub(start).min(line.len());
                let pos = line
                    .position_for_index(local, line_h)
                    .unwrap_or(point(px(0.), px(0.)));
                return Some(Bounds::from_corners(
                    point(bounds.left() + pos.x, bounds.top() + y + pos.y),
                    point(
                        bounds.left() + pos.x + px(2.),
                        bounds.top() + y + pos.y + line_h,
                    ),
                ));
            }
            y += line.size(line_h).height;
        }
        Some(Bounds::from_corners(
            point(bounds.left(), bounds.top()),
            point(bounds.left() + px(2.), bounds.top() + line_h),
        ))
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.offset_to_utf16(self.index_for_mouse_position(point)))
    }
}

fn paragraph_starts(lines: &[gpui::WrappedLine]) -> Vec<usize> {
    let mut starts = Vec::with_capacity(lines.len());
    let mut off = 0;
    for (i, line) in lines.iter().enumerate() {
        starts.push(off);
        off += line.len();
        if i + 1 < lines.len() {
            off += 1;
        }
    }
    starts
}

struct TextElement {
    input: Entity<TextField>,
}

struct PrepaintState {
    lines: Vec<gpui::WrappedLine>,
    starts: Vec<usize>,
    line_height: Pixels,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
}

impl IntoElement for TextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let min_height = self.input.read(cx).min_height;
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = px(min_height).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let content = input.content.clone();
        let selected_range = input.selected_range.clone();
        let cursor = input.cursor_offset();
        let style = window.text_style();
        let theme = Theme::for_window(window);
        let (display_text, text_color) = if content.is_empty() {
            (input.placeholder.clone(), theme.ink_faint().into())
        } else {
            (content, style.color)
        };
        let run = TextRun {
            len: display_text.len(),
            font: style.font(),
            color: text_color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = if let Some(marked_range) = input.marked_range.as_ref() {
            vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end - marked_range.start,
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len().saturating_sub(marked_range.end),
                    ..run
                },
            ]
            .into_iter()
            .filter(|run| run.len > 0)
            .collect()
        } else {
            vec![run]
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let wrap = if input.multiline {
            Some(bounds.size.width)
        } else {
            None
        };
        let lines: Vec<gpui::WrappedLine> = window
            .text_system()
            .shape_text(display_text, font_size, &runs, wrap, None)
            .unwrap_or_default()
            .into_iter()
            .collect();
        let starts = paragraph_starts(&lines);
        let line_h = window.line_height();
        let pos_for = |idx: usize| {
            let mut y = px(0.);
            for (i, line) in lines.iter().enumerate() {
                let start = starts.get(i).copied().unwrap_or(0);
                let end = start + line.len();
                let last = i + 1 == lines.len();
                if idx <= end || last {
                    let local = idx.saturating_sub(start).min(line.len());
                    let p = line
                        .position_for_index(local, line_h)
                        .unwrap_or(point(px(0.), px(0.)));
                    return point(p.x, y + p.y);
                }
                y += line.size(line_h).height;
            }
            point(px(0.), y)
        };
        let (selection, cursor_quad) = if selected_range.is_empty() {
            let p = pos_for(cursor);
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + p.x, bounds.top() + p.y),
                        size(px(2.), line_h),
                    ),
                    theme.gold(),
                )),
            )
        } else {
            let a = pos_for(selected_range.start);
            let b = pos_for(selected_range.end);
            let top_left = point(
                bounds.left() + a.x.min(b.x),
                bounds.top() + a.y.min(b.y),
            );
            let bottom_right = point(
                bounds.left() + a.x.max(b.x).max(a.x + px(2.)),
                bounds.top() + a.y.max(b.y) + line_h,
            );
            (
                Some(fill(
                    Bounds::from_corners(top_left, bottom_right),
                    theme.gold_soft(),
                )),
                None,
            )
        };
        PrepaintState {
            lines,
            starts,
            line_height: line_h,
            cursor: cursor_quad,
            selection,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&gpui::InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let line_h = prepaint.line_height;
        window.paint_layer(bounds, |window| {
            if let Some(selection) = prepaint.selection.take() {
                window.paint_quad(selection);
            }
            let mut origin = bounds.origin;
            for line in &prepaint.lines {
                let _ = line.paint(
                    origin,
                    line_h,
                    gpui::TextAlign::Left,
                    Some(bounds),
                    window,
                    cx,
                );
                origin.y += line.size(line_h).height;
            }
            if focus_handle.is_focused(window) {
                if let Some(cursor) = prepaint.cursor.take() {
                    window.paint_quad(cursor);
                }
            }
        });
        let lines = prepaint.lines.clone();
        let starts = prepaint.starts.clone();
        self.input.update(cx, |input, _cx| {
            input.last_lines = lines;
            input.last_starts = starts;
            input.last_line_height = line_h;
            input.last_bounds = Some(bounds);
        });
    }
}

impl Render for TextField {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = Theme::for_window(window);
        let focused = self.focus_handle.is_focused(window);
        let border = if focused {
            theme.gold()
        } else {
            theme.line_strong()
        };
        div()
            .flex()
            .key_context("CardField")
            .track_focus(&self.focus_handle(cx))
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::backspace))
            .on_action(cx.listener(Self::delete))
            .on_action(cx.listener(Self::left))
            .on_action(cx.listener(Self::right))
            .on_action(cx.listener(Self::select_left))
            .on_action(cx.listener(Self::select_right))
            .on_action(cx.listener(Self::select_all))
            .on_action(cx.listener(Self::home))
            .on_action(cx.listener(Self::end))
            .on_action(cx.listener(Self::insert_newline))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::cut))
            .on_action(cx.listener(Self::copy))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .rounded_lg()
            .border_1()
            .border_color(border)
            .bg(theme.card())
            .px_3()
            .py_2()
            .text_sm()
            .text_color(theme.ink())
            .child(TextElement {
                input: cx.entity(),
            })
    }
}

impl Focusable for TextField {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_paste_strips_newlines() {
        assert_eq!(sanitize_paste("a\nb\r\nc", false), "a b c");
    }

    #[test]
    fn multiline_paste_keeps_newlines() {
        assert_eq!(sanitize_paste("a\nb\r\nc", true), "a\nb\nc");
    }
}
