//! Editor persist commands. Same split as Tauri `cardEditor.ts`.

use crate::application::data::Priority;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EditorKind {
    Create,
    Edit { id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditorFields {
    pub title: String,
    pub notes: String,
    pub deadline_at: Option<i64>,
    pub priority: Priority,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PersistCommand {
    None,
    Create(EditorFields),
    Update {
        id: String,
        fields: EditorFields,
    },
    Delete {
        id: String,
    },
}

pub fn command_on_create_click() -> PersistCommand {
    PersistCommand::None
}

pub fn command_on_editor_save(kind: &EditorKind, fields: EditorFields) -> PersistCommand {
    match kind {
        EditorKind::Create => PersistCommand::Create(fields),
        EditorKind::Edit { id } => PersistCommand::Update {
            id: id.clone(),
            fields,
        },
    }
}

pub fn command_on_editor_cancel() -> PersistCommand {
    PersistCommand::None
}

pub fn command_on_editor_delete(kind: &EditorKind) -> PersistCommand {
    match kind {
        EditorKind::Create => PersistCommand::None,
        EditorKind::Edit { id } => PersistCommand::Delete { id: id.clone() },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::data::Priority;

    fn fields() -> EditorFields {
        EditorFields {
            title: "买菜".into(),
            notes: "今晚".into(),
            deadline_at: Some(1_777_000_000),
            priority: Priority::High,
        }
    }

    #[test]
    fn opening_create_does_not_persist() {
        assert_eq!(command_on_create_click(), PersistCommand::None);
    }

    #[test]
    fn cancel_never_persists() {
        assert_eq!(command_on_editor_cancel(), PersistCommand::None);
    }

    #[test]
    fn saving_create_persists_a_new_card() {
        assert_eq!(
            command_on_editor_save(&EditorKind::Create, fields()),
            PersistCommand::Create(fields())
        );
    }

    #[test]
    fn saving_edit_updates_the_existing_card() {
        let mut fields = fields();
        fields.notes = String::new();
        assert_eq!(
            command_on_editor_save(
                &EditorKind::Edit {
                    id: "c1".into()
                },
                fields.clone()
            ),
            PersistCommand::Update {
                id: "c1".into(),
                fields
            }
        );
    }

    #[test]
    fn delete_only_applies_to_edit() {
        assert_eq!(
            command_on_editor_delete(&EditorKind::Create),
            PersistCommand::None
        );
        assert_eq!(
            command_on_editor_delete(&EditorKind::Edit {
                id: "c1".into()
            }),
            PersistCommand::Delete { id: "c1".into() }
        );
    }
}
