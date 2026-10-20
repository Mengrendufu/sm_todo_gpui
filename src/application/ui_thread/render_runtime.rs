//! RenderRuntime: gpui Application::run on the UI thread.
//! Drains Inbox after a wake from a cross-thread post.

use async_channel::Receiver;
use gpui::{
    px, size, App, Application, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*,
};

use crate::application::data::DataManager;
use crate::application::ui_thread::evt::UI_Evt;
use crate::application::ui_thread::inbox::{
    UI_Inbox_bind, UI_Inbox_take_all,
};
use crate::application::ui_thread::sm_ui::{SM_UI, SM_UI_HostOps};

pub fn RenderRuntime_setup() -> Receiver<()> {
    let (wake_tx, wake_rx) = async_channel::bounded::<()>(1);
    UI_Inbox_bind(wake_tx);
    wake_rx
}

pub fn RenderRuntime_drain() {}

pub fn RenderRuntime_request_frame() {}

pub fn RenderRuntime_run(wake_rx: Receiver<()>) {
    Application::new().run(|cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1200.), px(800.)), cx);
        crate::application::ui_thread::sm_ui::bind_editor_keys(cx);
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
                |window, cx| {
                    let ui = cx.new(|cx| {
                        cx.observe_window_appearance(window, |_, _, cx| {
                            cx.notify();
                        })
                        .detach();
                        let data =
                            DataManager::open(&DataManager::default_db_path()).expect("open db");
                        SM_UI::setup(
                            SM_UI_HostOps {
                                request_quit: || {},
                                request_frame: || {},
                            },
                            data,
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
                            UI_Evt::TrayQuit => {
                                SM_UI::uninstall_tray(window);
                                window.remove_window();
                                cx.quit();
                            }
                            other => sm.dispatch_evt(&other),
                        }
                    }
                    cx.notify();
                });
            }
        })
        .detach();
        cx.activate(true);
    });
}
