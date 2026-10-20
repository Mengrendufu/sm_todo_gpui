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
    UI_Inbox_bind, UI_Inbox_take_all, UI_Inbox_post,
};
use crate::application::ui_thread::hsm::{SM_UI, SM_UI_HostOps};

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
                    window.on_window_should_close(cx, |_, _| {
                        UI_Inbox_post(UI_Evt::TrayQuit);
                        false
                    });
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
                        sm.apply_always_on_top(window);
                        sm.apply_skip_taskbar(window);
                        cx.observe_window_activation(window, |this, window, _cx| {
                            if window.is_window_active() {
                                this.apply_skip_taskbar(window);
                            }
                        })
                        .detach();
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
                        match e {
                            UI_Evt::TrayShow => sm.reveal_from_tray(window, cx),
                            UI_Evt::HideToTray => {
                                sm.cancel_drag();
                                sm.post_pending();
                                sm.hide_to_tray(window);
                            }
                            UI_Evt::TogglePin => {
                                sm.toggle_pin();
                                sm.apply_always_on_top(window);
                            }
                            UI_Evt::TrayQuit => sm.begin_close(),
                            other => {
                                if matches!(other, UI_Evt::SaveEditor) {
                                    sm.pull_editor_fields(cx);
                                }
                                sm.dispatch_evt(&other);
                            }
                        }
                    }
                    if sm.close_failed() {
                        sm.reveal_from_tray(window, cx);
                    }
                    if sm.ready_to_close() {
                        SM_UI::uninstall_tray(window);
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
