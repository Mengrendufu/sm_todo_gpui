//! Card editing view, field choices and edit commands.

use super::{calendar, meta as card_meta};
use crate::application::snapshot::Priority;
use crate::application::ui_thread::{
    hsm::{theme, UI_Controls, SM_UI},
    thread::{evt::UI_Evt, inbox::UI_Inbox_post},
};
use gpui::{div, prelude::*, px, Context, MouseButton};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorKind {
    Create,
    Edit { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorFields {
    pub title: String,
    pub notes: String,
    pub deadline_at: Option<i64>,
    pub priority: Priority,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistCommand {
    None,
    Create(EditorFields),
    Update { id: String, fields: EditorFields },
    Delete { id: String },
}

pub fn command_on_create_click() -> PersistCommand {
    PersistCommand::None
}

pub fn command_on_editor_save(kind: &EditorKind, fields: EditorFields) -> PersistCommand {
    match kind {
        EditorKind::Create => PersistCommand::Create(fields),
        EditorKind::Edit { id } => PersistCommand::Update {
            id: id.clone(),
            fields,
        },
    }
}

pub fn command_on_editor_cancel() -> PersistCommand {
    PersistCommand::None
}

pub fn command_on_editor_delete(kind: &EditorKind) -> PersistCommand {
    match kind {
        EditorKind::Create => PersistCommand::None,
        EditorKind::Edit { id } => PersistCommand::Delete { id: id.clone() },
    }
}

impl UI_Controls {
    pub(in crate::application::ui_thread::hsm) fn render_card_editor(
        &self,
        theme: theme::Theme,
        cx: &mut Context<SM_UI>,
    ) -> impl IntoElement {
        let Some(editor) = &self.editor else {
            return div().into_any_element();
        };
        let heading = match &editor.kind {
            EditorKind::Create => "新建卡片",
            EditorKind::Edit { .. } => "编辑卡片",
        };
        let can_delete = matches!(editor.kind, EditorKind::Edit { .. });
        let hour = editor.hour;
        let minute = editor.minute;
        let has_date = !editor.deadline.is_empty();
        let priority = editor.priority;
        let error = editor.error.clone();
        let mut form = div()
            .id("card-editor")
            .w(px(440.))
            .max_w(px(440.))
            .min_w_0()
            .overflow_hidden()
            .flex()
            .flex_col()
            .gap_4()
            .rounded(px(20.))
            .border_1()
            .border_color(theme.line())
            .bg(theme.card())
            .p(px(28.))
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_, _, cx| cx.stop_propagation())
            .child(div().text_xl().child(heading))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .min_w_0()
                    .child(div().text_xs().text_color(theme.ink_soft()).child("标题"))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .children(self.title_field.clone()),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .min_w_0()
                    .child(div().text_xs().text_color(theme.ink_soft()).child("备注"))
                    .child(
                        div()
                            .w_full()
                            .min_w_0()
                            .overflow_hidden()
                            .children(self.notes_field.clone()),
                    ),
            )
            .child({
                let mut deadline = div()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .min_w_0()
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.ink_soft())
                            .child("截止日期"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_2()
                            .min_w_0()
                            .child(date_trigger(&editor.deadline, theme, cx))
                            .child(hour_stepper(hour, minute, has_date, theme, cx)),
                    );
                if self.calendar_open {
                    deadline = deadline.child(month_calendar(
                        self.calendar_year,
                        self.calendar_month,
                        calendar::ymd_from_date_string(&editor.deadline),
                        theme,
                        cx,
                    ));
                }
                deadline
            })
            .child(priority_row(priority, theme, cx));
        if let Some(error) = error {
            form = form.child(div().text_xs().text_color(theme.danger()).child(error));
        }
        form = form.child(editor_actions(can_delete, theme, cx));
        div()
            .id("editor-backdrop")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(theme.backdrop())
            .occlude()
            .on_click(cx.listener(|_, _, _, cx| {
                UI_Inbox_post(UI_Evt::CancelEditor);
                cx.notify();
            }))
            .child(form)
            .into_any_element()
    }
}

fn date_trigger(deadline: &str, theme: theme::Theme, cx: &mut Context<SM_UI>) -> impl IntoElement {
    let label = if deadline.is_empty() {
        "选择日期".to_string()
    } else {
        deadline.to_string()
    };
    let muted = deadline.is_empty();
    div()
        .id("deadline-date")
        .flex_1()
        .min_w_0()
        .h(px(36.))
        .px_3()
        .rounded_lg()
        .border_1()
        .border_color(theme.line_strong())
        .flex()
        .items_center()
        .justify_between()
        .cursor_pointer()
        .on_click(cx.listener(|this, _, _, cx| {
            this.calendar_open = !this.calendar_open;
            if this.calendar_open {
                this.sync_calendar_month();
            }
            cx.notify();
        }))
        .child(
            div()
                .text_sm()
                .text_color(if muted {
                    theme.ink_faint()
                } else {
                    theme.ink()
                })
                .child(label),
        )
        .child(
            div()
                .id("deadline-clear")
                .text_xs()
                .text_color(theme.ink_faint())
                .cursor_pointer()
                .on_click(cx.listener(|this, _, _, cx| {
                    cx.stop_propagation();
                    if let Some(editor) = &mut this.editor {
                        editor.deadline.clear();
                    }
                    this.calendar_open = false;
                    cx.notify();
                }))
                .child(if muted { "" } else { "清除" }),
        )
}

fn month_calendar(
    year: i32,
    month: u32,
    selected: Option<calendar::Ymd>,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let cells = calendar::month_cells(year, month);
    let heading = format!("{year}年{month}月");
    let weekdays = ["一", "二", "三", "四", "五", "六", "日"];
    let mut grid = div().flex().flex_col().gap_1();
    let mut header = div().flex();
    for day in weekdays {
        header = header.child(
            div()
                .flex_1()
                .text_center()
                .text_xs()
                .text_color(theme.ink_faint())
                .child(day),
        );
    }
    grid = grid.child(header);
    for week in 0..6 {
        let mut row = div().flex();
        for col in 0..7 {
            let cell = cells[week * 7 + col];
            row = row.child(day_cell(cell, selected, theme, cx));
        }
        grid = grid.child(row);
    }
    div()
        .mt_1()
        .p_2()
        .rounded_lg()
        .border_1()
        .border_color(theme.line())
        .bg(theme.card())
        .child(
            div()
                .flex()
                .items_center()
                .justify_between()
                .pb_2()
                .child(step_btn("cal-prev", "‹", true, theme, cx, |this| {
                    let (y, m) = calendar::shift_month(this.calendar_year, this.calendar_month, -1);
                    this.calendar_year = y;
                    this.calendar_month = m;
                }))
                .child(div().text_sm().child(heading))
                .child(step_btn("cal-next", "›", true, theme, cx, |this| {
                    let (y, m) = calendar::shift_month(this.calendar_year, this.calendar_month, 1);
                    this.calendar_year = y;
                    this.calendar_month = m;
                })),
        )
        .child(grid)
}

fn day_cell(
    cell: calendar::MonthCell,
    selected: Option<calendar::Ymd>,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let on = selected == Some(cell.ymd);
    let fg = if !cell.in_month {
        theme.ink_faint()
    } else if on {
        theme.on_primary()
    } else {
        theme.ink()
    };
    let bg = if on { theme.gold() } else { theme.card() };
    let ymd = cell.ymd;
    div()
        .id((
            "cal-day",
            (ymd.year as u64) << 16 | (ymd.month as u64) << 8 | ymd.day as u64,
        ))
        .flex_1()
        .h(px(28.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .text_xs()
        .text_color(fg)
        .bg(bg)
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            if let Some(editor) = &mut this.editor {
                editor.deadline = calendar::date_string_from_ymd(ymd);
            }
            this.calendar_year = ymd.year;
            this.calendar_month = ymd.month;
            this.calendar_open = false;
            cx.notify();
        }))
        .child(format!("{}", ymd.day))
}

fn hour_stepper(
    hour: u32,
    minute: u32,
    enabled: bool,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let label = format!("{hour:02}:{minute:02}");
    div()
        .flex()
        .items_center()
        .gap_1()
        .opacity(if enabled { 1. } else { 0.45 })
        .child(step_btn("hour-dec", "−", enabled, theme, cx, |this| {
            if let Some(editor) = &mut this.editor {
                let (hour, minute) = card_meta::step_half_hour(editor.hour, editor.minute, -1);
                editor.hour = hour;
                editor.minute = minute;
            }
        }))
        .child(div().w(px(56.)).text_center().text_sm().child(label))
        .child(step_btn("hour-inc", "+", enabled, theme, cx, |this| {
            if let Some(editor) = &mut this.editor {
                let (hour, minute) = card_meta::step_half_hour(editor.hour, editor.minute, 1);
                editor.hour = hour;
                editor.minute = minute;
            }
        }))
}

fn step_btn(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
    on_press: impl Fn(&mut SM_UI) + 'static,
) -> impl IntoElement {
    div()
        .id(id)
        .w(px(28.))
        .h(px(28.))
        .rounded_md()
        .flex()
        .items_center()
        .justify_center()
        .border_1()
        .border_color(theme.line_strong())
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            if enabled {
                on_press(this);
                cx.notify();
            }
        }))
        .child(label)
}

fn priority_row(
    selected: Priority,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(div().text_xs().text_color(theme.ink_soft()).child("优先级"))
        .child(
            div()
                .flex()
                .gap_2()
                .child(priority_choice(
                    "prio-low",
                    "低",
                    Priority::Low,
                    selected,
                    theme,
                    cx,
                ))
                .child(priority_choice(
                    "prio-med",
                    "中",
                    Priority::Medium,
                    selected,
                    theme,
                    cx,
                ))
                .child(priority_choice(
                    "prio-high",
                    "高",
                    Priority::High,
                    selected,
                    theme,
                    cx,
                )),
        )
}

fn priority_choice(
    id: &'static str,
    label: &'static str,
    value: Priority,
    selected: Priority,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let on = selected == value;
    let (fg, bg) = if on {
        theme.priority_tag(value)
    } else {
        (theme.ink(), theme.clear())
    };
    let border = if on {
        theme.clear()
    } else {
        theme.line_strong()
    };
    div()
        .id(id)
        .px_3()
        .py_1()
        .rounded_md()
        .border_1()
        .border_color(border)
        .bg(bg)
        .text_color(fg)
        .text_xs()
        .cursor_pointer()
        .on_click(cx.listener(move |this, _, _, cx| {
            this.set_editor_priority(value);
            cx.notify();
        }))
        .child(label)
}

fn editor_actions(
    can_delete: bool,
    theme: theme::Theme,
    cx: &mut Context<SM_UI>,
) -> impl IntoElement {
    let mut row = div().flex().items_center().gap_2().pt_2();
    if can_delete {
        row = row.child(
            div()
                .id("editor-delete")
                .flex_1()
                .text_xs()
                .text_color(theme.danger())
                .cursor_pointer()
                .on_click(cx.listener(|_, _, _, cx| {
                    UI_Inbox_post(UI_Evt::DeleteEditor);
                    cx.notify();
                }))
                .child("删除"),
        );
    } else {
        row = row.child(div().flex_1());
    }
    row.child(
        div()
            .id("editor-cancel")
            .px_3()
            .py_1()
            .rounded_md()
            .border_1()
            .border_color(theme.line_strong())
            .text_xs()
            .cursor_pointer()
            .on_click(cx.listener(|_, _, _, cx| {
                UI_Inbox_post(UI_Evt::CancelEditor);
                cx.notify();
            }))
            .child("取消"),
    )
    .child(
        div()
            .id("editor-save")
            .px_3()
            .py_1()
            .rounded_md()
            .bg(theme.gold())
            .text_color(theme.on_primary())
            .text_xs()
            .cursor_pointer()
            .on_click(cx.listener(|_, _, _, cx| {
                UI_Inbox_post(UI_Evt::SaveEditor);
                cx.notify();
            }))
            .child("保存"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::snapshot::Priority;

    fn fields() -> EditorFields {
        EditorFields {
            title: "买菜".into(),
            notes: "今晚".into(),
            deadline_at: Some(1_777_000_000),
            priority: Priority::High,
        }
    }

    #[test]
    fn opening_create_does_not_persist() {
        assert_eq!(command_on_create_click(), PersistCommand::None);
    }

    #[test]
    fn cancel_never_persists() {
        assert_eq!(command_on_editor_cancel(), PersistCommand::None);
    }

    #[test]
    fn saving_create_persists_a_new_card() {
        assert_eq!(
            command_on_editor_save(&EditorKind::Create, fields()),
            PersistCommand::Create(fields())
        );
    }

    #[test]
    fn saving_edit_updates_the_existing_card() {
        let mut fields = fields();
        fields.notes = String::new();
        assert_eq!(
            command_on_editor_save(&EditorKind::Edit { id: "c1".into() }, fields.clone()),
            PersistCommand::Update {
                id: "c1".into(),
                fields
            }
        );
    }

    #[test]
    fn delete_only_applies_to_edit() {
        assert_eq!(
            command_on_editor_delete(&EditorKind::Create),
            PersistCommand::None
        );
        assert_eq!(
            command_on_editor_delete(&EditorKind::Edit { id: "c1".into() }),
            PersistCommand::Delete { id: "c1".into() }
        );
    }
}
