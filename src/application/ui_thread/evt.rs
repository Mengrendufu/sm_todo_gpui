//! UI_Evt: SM_UI's event domain on the UI thread.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UI_Evt {
    Null,
    Timeout,
    TrayShow,
    TrayQuit,
}
