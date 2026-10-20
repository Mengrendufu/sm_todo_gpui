//! Coordinates board/editor lifetime. Widget feedback never dispatches here.

use rs_hsm_::{SM_Hsm, SM_HsmState, SM_HsmTrait, SM_RetState, SM_StatePtr};

use super::UI_Controls;
use crate::application::ui_thread::thread::evt::UI_Evt;

impl SM_HsmTrait for UI_Controls {
    type Evt = UI_Evt;

    fn topInitial(_: &mut Self) -> SM_StatePtr<Self> {
        &RUNNING
    }

    fn init(hsm: &mut SM_Hsm<Self>, me: &mut Self) {
        hsm.init(me);
    }

    fn dispatch(hsm: &mut SM_Hsm<Self>, me: &mut Self, e: &UI_Evt) {
        hsm.dispatch(me, e);
    }
}

static RUNNING: SM_HsmState<UI_Controls> = SM_HsmState {
    super_: None,
    init_: Some(|_| &BOARD),
    entry_: None,
    exit_: None,
    // Timeout currently has no control scheduling work. Window requests are
    // consumed by the thread router before HSM dispatch.
    handler_: |me, e| {
        if let UI_Evt::PersistMove {
            id,
            to_status,
            before_id,
        } = e
        {
            if me
                .data
                .persist_move(id, *to_status, before_id.as_deref())
                .is_err()
            {
                me.reload();
            }
        }
        SM_RetState::HANDLED
    },
};

static BOARD: SM_HsmState<UI_Controls> = SM_HsmState {
    super_: Some(&RUNNING),
    init_: None,
    entry_: None,
    exit_: None,
    handler_: |me, e| match e {
        UI_Evt::NewCard if me.dragging.is_none() => {
            me.open_create();
            SM_RetState::TRAN(&EDITOR)
        }
        UI_Evt::EditCard { id } if me.dragging.is_none() => {
            me.open_edit(id);
            if me.editor.is_some() {
                SM_RetState::TRAN(&EDITOR)
            } else {
                SM_RetState::HANDLED
            }
        }
        _ => SM_RetState::SUPER,
    },
};

static EDITOR: SM_HsmState<UI_Controls> = SM_HsmState {
    super_: Some(&RUNNING),
    init_: None,
    entry_: None,
    exit_: Some(|me, _| me.close_editor()),
    handler_: |me, e| match e {
        UI_Evt::CancelEditor => SM_RetState::TRAN(&BOARD),
        UI_Evt::SaveEditor => {
            if me.save_editor() {
                SM_RetState::TRAN(&BOARD)
            } else {
                SM_RetState::HANDLED
            }
        }
        UI_Evt::DeleteEditor => {
            if me.delete_editor() {
                SM_RetState::TRAN(&BOARD)
            } else {
                SM_RetState::HANDLED
            }
        }
        _ => SM_RetState::SUPER,
    },
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::data::DataManager;
    use crate::application::ui_thread::thread::inbox::UI_Inbox;
    use crate::application::ui_thread::hsm::{SM_UI_HostOps, SM_UI};
    use crate::application::data::Status;

    fn ui() -> SM_UI {
        SM_UI::setup(
            SM_UI_HostOps {
                request_quit: || {},
                request_frame: || {},
            },
            DataManager::open_in_memory().unwrap(),
        )
    }

    fn drain(inbox: &mut UI_Inbox, ui: &mut SM_UI) {
        for e in inbox.take_all() {
            ui.dispatch_evt(&e);
        }
    }

    #[test]
    fn queued_open_requests_cannot_replace_an_active_draft() {
        let mut ui = ui();
        ui.add_todo_card("existing");
        let id = ui.cards[0].id.clone();
        let mut inbox = UI_Inbox::new();
        inbox.post(UI_Evt::NewCard);
        assert!(!ui.editor_is_open());
        drain(&mut inbox, &mut ui);
        ui.set_editor_title("unsaved");
        inbox.post(UI_Evt::NewCard);
        inbox.post(UI_Evt::EditCard { id: id.clone() });
        inbox.post(UI_Evt::Timeout);
        drain(&mut inbox, &mut ui);
        assert_eq!(ui.editor_title(), Some("unsaved"));
        inbox.post(UI_Evt::CancelEditor);
        inbox.post(UI_Evt::EditCard { id });
        drain(&mut inbox, &mut ui);
        assert_eq!(ui.editor_title(), Some("existing"));
    }

    #[test]
    fn failed_save_keeps_editor_active_for_retry_then_returns_to_board() {
        let mut ui = ui();
        ui.dispatch_evt(&UI_Evt::NewCard);
        ui.set_editor_title(" ");
        ui.dispatch_evt(&UI_Evt::SaveEditor);
        assert!(ui.editor_is_open());
        assert_eq!(ui.editor_error(), Some("标题不能为空"));
        ui.dispatch_evt(&UI_Evt::NewCard);
        assert_eq!(ui.editor_title(), Some(" "));
        ui.set_editor_title("saved");
        ui.dispatch_evt(&UI_Evt::SaveEditor);
        assert!(!ui.editor_is_open());
        assert_eq!(ui.cards[0].title, "saved");
        ui.dispatch_evt(&UI_Evt::NewCard);
        assert_eq!(ui.editor_title(), Some(""));
    }

    #[test]
    fn nonexistent_edit_and_delete_on_create_preserve_valid_modes() {
        let mut ui = ui();
        ui.dispatch_evt(&UI_Evt::EditCard {
            id: "missing".into(),
        });
        assert!(!ui.editor_is_open());
        ui.dispatch_evt(&UI_Evt::NewCard);
        ui.set_editor_title("draft");
        ui.dispatch_evt(&UI_Evt::DeleteEditor);
        assert_eq!(ui.editor_title(), Some("draft"));
        ui.dispatch_evt(&UI_Evt::CancelEditor);
        ui.dispatch_evt(&UI_Evt::SaveEditor);
        assert!(ui.cards.is_empty());
        ui.dispatch_evt(&UI_Evt::NewCard);
        assert!(ui.editor_is_open());
    }

    #[test]
    fn local_drag_blocks_editor_activation_until_drag_finishes() {
        let mut ui = ui();
        ui.add_todo_card("card");
        let id = ui.cards[0].id.clone();
        ui.start_drag(&id);
        ui.dispatch_evt(&UI_Evt::NewCard);
        ui.dispatch_evt(&UI_Evt::EditCard { id: id.clone() });
        assert!(!ui.editor_is_open());
        assert!(ui.dragging.is_some());
        ui.cancel_drag();
        ui.dispatch_evt(&UI_Evt::EditCard { id });
        assert_eq!(ui.editor_title(), Some("card"));
    }

    #[test]
    fn landing_starts_locally_then_owned_move_is_persisted_after_first_paint() {
        let mut ui = ui();
        ui.add_todo_card("card");
        let id = ui.cards[0].id.clone();
        ui.start_drag(&id);
        ui.note_drag_geometry(200.0, 90.0, 40.0, 50.0, 0.0);
        ui.drop_card(&id, Status::Doing, None);
        assert_eq!(ui.cards[0].status, Status::Doing);
        assert!(ui.settling.is_some());
        assert_eq!(ui.data.getCards().unwrap()[0].status, Status::Todo);
        assert!(ui.controls.flush().is_none());
        ui.settling
            .as_mut()
            .unwrap()
            .progress(std::time::Instant::now());
        let e = ui.controls.flush().expect("move after first paint");
        assert!(ui.controls.flush().is_none(), "move is emitted once");
        let mut inbox = UI_Inbox::new();
        inbox.post(e);
        // Opening another control before persistence must preserve its draft.
        ui.dispatch_evt(&UI_Evt::NewCard);
        ui.set_editor_title("another draft");
        drain(&mut inbox, &mut ui);
        assert_eq!(ui.data.getCards().unwrap()[0].status, Status::Doing);
        assert_eq!(ui.editor_title(), Some("another draft"));
        assert!(ui.settling.is_some());
    }
}
