//! Application titles and titlebar controls.

use super::super::theme::Theme;
use crate::application::ui_thread::{
    hsm::{theme, UI_Controls, SM_UI},
    thread::{evt::UI_Evt, inbox::UI_Inbox_post},
};
use gpui::{div, prelude::*, px, rgb, Context, Window, WindowControlArea};

pub fn window_button(
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
        .child(div().size(px(12.)).rounded(px(6.)).bg(rgb(color)))
}

pub fn drag_area(id: &'static str) -> impl gpui::IntoElement {
    div()
        .id(id)
        .flex_1()
        .h_full()
        .min_w(px(8.))
        .window_control_area(WindowControlArea::Drag)
}

pub fn window_title(name: &'static str, theme: Theme) -> impl gpui::IntoElement {
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

pub fn board_title(theme: Theme) -> gpui::Div {
    div().text_xl().text_color(theme.gold()).child("TODO")
}

impl UI_Controls {
    pub(in crate::application::ui_thread::hsm) fn render_titlebar(
        &self,
        window: &Window,
        theme: theme::Theme,
        pinned: bool,
        cx: &mut Context<SM_UI>,
    ) -> impl IntoElement {
        let active = window.is_window_active();
        let close = if active { 0xff5f57 } else { theme.traffic_dim };
        let min = if active { 0xfebc2e } else { theme.traffic_dim };
        let max = if active { 0x28c840 } else { theme.traffic_dim };
        let pin_bg = theme.pin_bg(pinned);
        let pin_fg = theme.pin_fg(pinned);
        let pin_hover = theme.pin_hover(pinned);
        div()
            .relative()
            .flex()
            .w_full()
            .h(px(42.))
            .flex_none()
            .items_center()
            .px_3()
            .border_b_1()
            .border_color(theme.line())
            .child(window_title("SunnyMatato", theme))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .pr_2()
                    .child(window_button(
                        "close",
                        Some(WindowControlArea::Close),
                        close,
                    ))
                    .child(
                        window_button("min", None, min).on_click(cx.listener(|_, _, _, cx| {
                            UI_Inbox_post(UI_Evt::HideToTray);
                            cx.notify();
                        })),
                    )
                    .child(window_button("max", Some(WindowControlArea::Max), max)),
            )
            .child(drag_area("titlebar-drag"))
            .child(
                div()
                    .id("pin-window")
                    .w(px(32.))
                    .h(px(32.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .bg(pin_bg)
                    .text_color(pin_fg)
                    .text_xs()
                    .hover(move |this| this.bg(pin_hover))
                    .on_click(cx.listener(|_, _, _, cx| {
                        UI_Inbox_post(UI_Evt::TogglePin);
                        cx.notify();
                    }))
                    .child(if pinned { "●" } else { "○" }),
            )
    }
}
