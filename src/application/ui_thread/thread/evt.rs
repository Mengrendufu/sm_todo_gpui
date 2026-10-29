//! Owned requests routed by the GUI thread; local widget feedback stays in callbacks.

#[derive(Clone, Debug, PartialEq)]
pub enum UI_Evt {
    Null,
    Timeout,
    TrayShow,
    TrayQuit,
    HideToTray,
    TogglePin,
    ColumnCaptureLost {
        generation: u64,
    },
    Column {
        status: crate::application::snapshot::Status,
        token: CaptureToken,
        event: ColumnEvent,
    },
    DropCard {
        id: String,
        status: crate::application::snapshot::Status,
        before: Option<String>,
        pointer: Option<(f32, f32)>,
    },
    CancelCardDrag,
    Window(WindowEvent),
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CaptureToken(pub u64);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DragGeometry {
    pub track_top: f32,
    pub grab: f32,
    pub viewport: f32,
    pub max_scroll: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColumnEvent {
    ThumbPressed {
        token: CaptureToken,
        geometry: DragGeometry,
        offset: f32,
    },
    TrackPressed {
        top: f32,
        viewport: f32,
        max_scroll: f32,
    },
    PointerMoved {
        y: f32,
    },
    PointerReleased,
    CaptureLost,
    Cancel,
    ViewportChanged,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowEvent {
    GeometryChanged,
    HoverChanged(bool),
    HoldReleased,
    SettleDue(u64),
    CollapseDue(u64),
    HotEdgeDue(u64),
    SlideFinished { generation: u64, hide: bool },
}
