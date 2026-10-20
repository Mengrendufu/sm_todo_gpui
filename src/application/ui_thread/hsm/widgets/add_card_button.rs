//! Add-card button: emits the NewCard coordination request.

use crate::application::ui_thread::{
    hsm::{theme::Theme, SM_UI},
    thread::{evt::UI_Evt, inbox::UI_Inbox_post},
};
use gpui::{div, prelude::*, px, Context};

pub fn render(theme: Theme, cx: &mut Context<SM_UI>) -> impl IntoElement {
    div()
        .id("add-card")
        .relative()
        .size(px(32.))
        .rounded(px(16.))
        .bg(theme.gold())
        .cursor_pointer()
        .hover(|this| this.bg(theme.gold_hover()))
        .on_click(cx.listener(|_, _, _, cx| {
            UI_Inbox_post(UI_Evt::NewCard);
            cx.notify();
        }))
        .child(
            div()
                .absolute()
                .left(px(9.5))
                .top(px(14.8))
                .w(px(13.))
                .h(px(2.4))
                .rounded(px(1.2))
                .bg(theme.on_primary()),
        )
        .child(
            div()
                .absolute()
                .left(px(14.8))
                .top(px(9.5))
                .w(px(2.4))
                .h(px(13.))
                .rounded(px(1.2))
                .bg(theme.on_primary()),
        )
}
