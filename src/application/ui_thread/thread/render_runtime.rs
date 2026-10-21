//! RenderRuntime: gpui Application::run on the UI thread.
//! Drains Inbox after a wake from a cross-thread signal or GUI coordination request.

use async_channel::Receiver;
use gpui::{
    px, size, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*,
};

use crate::application::data_manager::DataClient;
use crate::application::snapshot::Snapshot;
use crate::application::ui_thread::thread::evt::UI_Evt;
use crate::application::ui_thread::thread::inbox::{
    UI_Inbox_bind, UI_Inbox_take_all,
};
use crate::application::ui_thread::hsm::{SM_UI, SM_UI_HostOps};
use super::super::window_ops::{DispatchResult, ExitProgress, WindowOps};

pub fn RenderRuntime_setup() -> Receiver<()> {
    let (wake_tx, wake_rx) = async_channel::bounded::<()>(1);
    UI_Inbox_bind(wake_tx);
    wake_rx
}

pub fn RenderRuntime_drain() {}

pub fn RenderRuntime_request_frame() {}

pub fn RenderRuntime_run(wake_rx: Receiver<()>, snapshot: Snapshot, client: DataClient) {
    Application::new().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        crate::application::ui_thread::hsm::bind_editor_keys(cx);
        let handle = cx
            .open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    titlebar: Some(TitlebarOptions {
                        title: Some("SunnyMatato".into()),
                        appears_transparent: true,
                        ..Default::default()
                    }),
                    ..Default::default()
                },
                move |window, cx| {
                    let ui = cx.new(|cx| {
                        cx.observe_window_appearance(window, |_, _, cx| {
                            cx.notify();
                        })
                        .detach();
                        SM_UI::setup(
                            SM_UI_HostOps {
                                request_quit: || {},
                                request_frame: || {},
                            },
                            snapshot,
                            client,
                        )
                    });
                    ui.update(cx, |sm, cx| {
                        sm.window_ops.attach(window, cx);
                        cx.notify();
                    });
                    ui
                },
            )
            .expect("open window");
        cx.spawn(async move |cx| {
            while wake_rx.recv().await.is_ok() {
                let _ = handle.update(cx, |sm, window, cx| {
                    for e in UI_Inbox_take_all() {
                        if WindowOps::dispatch(sm, &e, window, cx) == DispatchResult::Unhandled {
                            if matches!(e, UI_Evt::SaveEditor) {
                                sm.pull_editor_fields(cx);
                            }
                            sm.dispatch_evt(&e);
                        }
                    }
                    if WindowOps::advance_exit(sm, window, cx) == ExitProgress::ExitReady {
                        WindowOps::detach(window);
                        window.remove_window();
                        cx.quit();
                    } else {
                        cx.notify();
                    }
                });
            }
        })
        .detach();
        cx.activate(true);
    });
}
