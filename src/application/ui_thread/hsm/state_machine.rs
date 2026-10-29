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
        if me.closing
            && matches!(
                e,
                UI_Evt::NewCard
                    | UI_Evt::EditCard { .. }
                    | UI_Evt::SaveEditor
                    | UI_Evt::DeleteEditor
                    | UI_Evt::DropCard { .. }
            )
        {
            return;
        }
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
        match e {
            UI_Evt::PersistPending => me.post_pending(),
            UI_Evt::Persisted(result) => me.persistence.complete(result),
            UI_Evt::RetryPersistence => me.persistence.retry(&me.snapshot),
            _ => {}
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
        UI_Evt::DropCard { id, status, before, pointer } => {
            if let Some((x, y)) = pointer { me.note_pointer(*x, *y); }
            me.drop_card(id, *status, before.as_deref());
            SM_RetState::HANDLED
        }
        UI_Evt::CancelCardDrag => {
            me.cancel_drag();
            SM_RetState::HANDLED
        }
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
    use crate::application::data_manager::DataManager;
    use crate::application::snapshot::Status;
    use crate::application::ui_thread::hsm::{SM_UI_HostOps, SM_UI};
    use crate::application::ui_thread::thread::inbox::UI_Inbox;

    fn ui() -> (DataManager, SM_UI) {
        let (database, cards) = DataManager::open_in_memory(|_| {}).unwrap();
        let client = database.client();
        let ui = SM_UI::setup(
            SM_UI_HostOps {
                request_quit: || {},
                request_frame: || {},
            },
            super::super::Snapshot::new(cards),
            client,
        );
        (database, ui)
    }

    fn drain(inbox: &mut UI_Inbox, ui: &mut SM_UI) {
        for e in inbox.take_all() {
            ui.dispatch_evt(&e);
        }
    }

    #[test]
    fn card_drop_changes_snapshot_only_after_queued_dispatch() {
        let (_database, mut ui) = ui();
        ui.add_todo_card("move me");
        let id = ui.snapshot.cards[0].id.clone();
        let mut inbox = UI_Inbox::new();
        inbox.post(UI_Evt::DropCard { id: id.clone(), status: Status::Doing, before: None, pointer: None });
        assert_eq!(ui.snapshot.cards[0].status, Status::Todo);
        drain(&mut inbox, &mut ui);
        assert_eq!(ui.snapshot.cards[0].status, Status::Doing);
        // An active editor keeps a late board drop from mutating its snapshot.
        ui.dispatch_evt(&UI_Evt::EditCard { id: id.clone() });
        inbox.post(UI_Evt::DropCard { id, status: Status::Done, before: None, pointer: None });
        drain(&mut inbox, &mut ui);
        assert_eq!(ui.snapshot.cards[0].status, Status::Doing);
    }

    #[test]
    fn queued_open_requests_cannot_replace_an_active_draft() {
        let (_database, mut ui) = ui();
        ui.add_todo_card("existing");
        let id = ui.snapshot.cards[0].id.clone();
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
        let (_database, mut ui) = ui();
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
        assert_eq!(ui.snapshot.cards[0].title, "saved");
        ui.dispatch_evt(&UI_Evt::NewCard);
        assert_eq!(ui.editor_title(), Some(""));
    }

    #[test]
    fn nonexistent_edit_and_delete_on_create_preserve_valid_modes() {
        let (_database, mut ui) = ui();
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
        assert!(ui.snapshot.cards.is_empty());
        ui.dispatch_evt(&UI_Evt::NewCard);
        assert!(ui.editor_is_open());
    }

    #[test]
    fn local_drag_blocks_editor_activation_until_drag_finishes() {
        let (_database, mut ui) = ui();
        ui.add_todo_card("card");
        let id = ui.snapshot.cards[0].id.clone();
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
        let (_database, mut ui) = ui();
        ui.add_todo_card("card");
        let id = ui.snapshot.cards[0].id.clone();
        ui.start_drag(&id);
        ui.note_drag_geometry(200.0, 90.0, 40.0, 50.0, 0.0);
        ui.drop_card(&id, Status::Doing, None);
        assert_eq!(ui.snapshot.cards[0].status, Status::Doing);
        assert!(ui.settling.is_some());
        assert!(ui.persistence.has_outbox());
        assert!(ui.controls.flush().is_none());
        ui.settling
            .as_mut()
            .unwrap()
            .progress(std::time::Instant::now());
        let e = ui.controls.flush().expect("move after first paint");
        assert!(ui.controls.flush().is_none(), "wake is emitted once");
        let mut inbox = UI_Inbox::new();
        inbox.post(e);
        // Opening another control before persistence must preserve its draft.
        ui.dispatch_evt(&UI_Evt::NewCard);
        ui.set_editor_title("another draft");
        drain(&mut inbox, &mut ui);
        assert!(!ui.persistence.has_outbox());
        assert_eq!(ui.editor_title(), Some("another draft"));
        assert!(ui.settling.is_some());
    }
    #[test]
    fn close_waits_for_all_results_and_does_not_accept_new_edits() {
        use std::{sync::mpsc, time::Duration};
        let (tx, rx) = mpsc::channel();
        let (_database, cards) = DataManager::open_in_memory(move |r| {
            tx.send(r).unwrap();
        })
        .unwrap();
        let mut ui = SM_UI::setup(
            SM_UI_HostOps {
                request_quit: || {},
                request_frame: || {},
            },
            super::super::Snapshot::new(cards),
            _database.client(),
        );
        ui.add_todo_card("queued");
        let id = ui.snapshot.cards[0].id.clone();
        ui.start_drag(&id);
        ui.drop_card(&id, Status::Doing, None);
        ui.begin_close(); // Also submits the drop before its first paint.
        assert!(!ui.ready_to_close());
        ui.dispatch_evt(&UI_Evt::NewCard);
        assert!(!ui.editor_is_open());
        ui.start_drag(&id);
        ui.drop_card(&id, Status::Done, None);
        assert!(ui.dragging.is_none());
        assert_eq!(ui.snapshot.cards[0].status, Status::Doing);
        for _ in 0..2 {
            ui.dispatch_evt(&UI_Evt::Persisted(
                rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            ));
        }
        assert!(ui.ready_to_close());
        assert_eq!(ui.snapshot.cards[0].status, Status::Doing);
    }

    #[test]
    fn failed_close_preserves_unsaved_cards_and_allows_editing_again() {
        let (database, cards) = DataManager::open_in_memory(|_| {}).unwrap();
        let mut ui = SM_UI::setup(
            SM_UI_HostOps {
                request_quit: || {},
                request_frame: || {},
            },
            super::super::Snapshot::new(cards),
            database.client(),
        );
        drop(database);
        ui.add_todo_card("not lost");
        ui.begin_close();
        assert!(!ui.ready_to_close());
        assert!(ui.close_failed());
        assert_eq!(ui.snapshot.cards[0].title, "not lost");
        assert_eq!(ui.persistence.error(), Some("数据库线程已退出"));
        ui.dispatch_evt(&UI_Evt::NewCard);
        assert!(ui.editor_is_open());
    }
}
