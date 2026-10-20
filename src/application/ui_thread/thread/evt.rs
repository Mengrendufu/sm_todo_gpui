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
    PersistPending,
    RetryPersistence,
    Persisted(crate::application::data_manager::PersistResult),
}
