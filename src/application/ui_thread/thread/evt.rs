//! Owned requests routed by the GUI thread; local widget feedback stays in callbacks.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UI_Evt {
    Null,
    Timeout,
    TrayShow,
    TrayQuit,
    HideToTray,
    TogglePin,
    NewCard,
    EditCard {
        id: String,
    },
    CancelEditor,
    SaveEditor,
    DeleteEditor,
    PersistMove {
        id: String,
        to_status: crate::application::data::Status,
        before_id: Option<String>,
    },
}
