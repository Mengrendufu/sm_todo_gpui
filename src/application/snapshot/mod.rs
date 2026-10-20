//! Shared working snapshot and card changes, independent of either thread.
//! Loaded from the database; local edits remain unsaved until a successful receipt.

mod domain;
mod error;

pub use domain::{Card, Priority, Status};
pub use error::AppError;

use domain::{normalize_title, overdue_cleared_for, sort_between};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Default)]
pub struct Changes {
    pub upserts: Vec<Card>,
    pub deletes: Vec<String>,
}

impl Changes {
    pub fn is_empty(&self) -> bool {
        self.upserts.is_empty() && self.deletes.is_empty()
    }
}

pub struct Snapshot {
    pub(crate) cards: Vec<Card>,
}

impl Snapshot {
    pub fn new(cards: Vec<Card>) -> Self {
        Self { cards }
    }

    pub fn create_card(&mut self, title: &str, notes: &str) -> Result<Changes, AppError> {
        self.create_card_with(title, notes, None, Priority::Medium)
    }

    pub fn create_card_with(
        &mut self,
        title: &str,
        notes: &str,
        deadline_at: Option<i64>,
        priority: Priority,
    ) -> Result<Changes, AppError> {
        let title = normalize_title(title)?;
        let now = now_secs();
        let sort_order = sort_between(
            self.cards
                .iter()
                .filter(|c| c.status == Status::Todo)
                .last()
                .map(|c| c.sort_order),
            None,
        );
        let card = Card {
            id: uuid::Uuid::new_v4().to_string(),
            title,
            notes: notes.into(),
            status: Status::Todo,
            sort_order,
            created_at: now,
            updated_at: now,
            deadline_at,
            priority,
            overdue_cleared: false,
        };
        self.cards.insert(
            self.cards
                .iter()
                .rposition(|c| c.status == Status::Todo)
                .map_or(0, |i| i + 1),
            card.clone(),
        );
        Ok(Changes {
            upserts: vec![card],
            deletes: vec![],
        })
    }

    pub fn update_card(
        &mut self,
        id: &str,
        title: &str,
        notes: &str,
        deadline_at: Option<i64>,
        priority: Priority,
    ) -> Result<Changes, AppError> {
        let title = normalize_title(title)?;
        let card = self
            .cards
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(AppError::CardNotFound)?;
        card.title = title;
        card.notes = notes.into();
        card.deadline_at = deadline_at;
        card.priority = priority;
        card.updated_at = now_secs();
        Ok(Changes {
            upserts: vec![card.clone()],
            deletes: vec![],
        })
    }

    pub fn delete_card(&mut self, id: &str) -> Result<Changes, AppError> {
        let index = self
            .cards
            .iter()
            .position(|c| c.id == id)
            .ok_or(AppError::CardNotFound)?;
        self.cards.remove(index);
        Ok(Changes {
            upserts: vec![],
            deletes: vec![id.into()],
        })
    }

    pub fn move_card(
        &mut self,
        id: &str,
        to_status: Status,
        before_id: Option<&str>,
    ) -> Result<Changes, AppError> {
        let index = self
            .cards
            .iter()
            .position(|c| c.id == id)
            .ok_or(AppError::CardNotFound)?;
        if before_id == Some(id) {
            return Ok(Changes::default());
        }
        let column: Vec<_> = self
            .cards
            .iter()
            .filter(|c| c.status == to_status && c.id != id)
            .collect();
        let slot = match before_id {
            None => column.len(),
            Some(before) => column
                .iter()
                .position(|c| c.id == before)
                .ok_or(AppError::InvalidTarget)?,
        };
        let prev = slot.checked_sub(1).map(|i| column[i].sort_order);
        let next = column.get(slot).map(|c| c.sort_order);
        let order = sort_between(prev, next);
        let collapsed = prev.is_some_and(|p| order <= p) || next.is_some_and(|n| order >= n);
        let mut ids: Vec<_> = column.iter().map(|c| c.id.clone()).collect();
        ids.insert(slot, id.into());
        let mut moving = self.cards.remove(index);
        moving.status = to_status;
        moving.sort_order = order;
        moving.updated_at = now_secs();
        moving.overdue_cleared = overdue_cleared_for(to_status);
        let insert_at = match before_id {
            Some(before) => self.cards.iter().position(|c| c.id == before).unwrap(),
            None => self
                .cards
                .iter()
                .rposition(|c| c.status == to_status)
                .map_or_else(
                    || {
                        self.cards
                            .iter()
                            .position(|c| status_key(c.status) > status_key(to_status))
                            .unwrap_or(self.cards.len())
                    },
                    |i| i + 1,
                ),
        };
        self.cards.insert(insert_at, moving);
        let mut upserts = Vec::new();
        if collapsed {
            let now = now_secs();
            for (i, id) in ids.iter().enumerate() {
                let card = self.cards.iter_mut().find(|c| c.id == *id).unwrap();
                card.sort_order = (i + 1) as f64;
                card.updated_at = now;
                card.overdue_cleared = overdue_cleared_for(to_status);
                upserts.push(card.clone());
            }
        } else {
            upserts.push(self.cards.iter().find(|c| c.id == id).unwrap().clone());
        }
        Ok(Changes {
            upserts,
            deletes: vec![],
        })
    }
}

fn status_key(status: Status) -> u8 {
    match status {
        Status::Todo => 0,
        Status::Doing => 1,
        Status::Done => 2,
    }
}
fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn titles_in(snapshot: &Snapshot, status: Status) -> Vec<String> {
        snapshot
            .cards
            .iter()
            .filter(|c| c.status == status)
            .map(|c| c.title.clone())
            .collect()
    }
    #[test]
    fn create_card_lands_in_todo() {
        let mut store = Snapshot::new(Vec::new());
        let card = store
            .create_card("  Write plan  ", "details")
            .unwrap()
            .upserts[0]
            .clone();
        assert_eq!(card.title, "Write plan");
        assert_eq!(card.notes, "details");
        assert_eq!(card.status, Status::Todo);
        assert_eq!(titles_in(&store, Status::Todo), vec!["Write plan"]);
    }

    #[test]
    fn create_card_rejects_blank_title() {
        let mut store = Snapshot::new(Vec::new());
        assert!(matches!(
            store.create_card("   ", ""),
            Err(AppError::EmptyTitle)
        ));
    }

    #[test]
    fn later_cards_append_in_todo() {
        let mut store = Snapshot::new(Vec::new());
        store.create_card("A", "").unwrap();
        store.create_card("B", "").unwrap();
        store.create_card("C", "").unwrap();
        assert_eq!(titles_in(&store, Status::Todo), vec!["A", "B", "C"]);
    }

    #[test]
    fn update_card_changes_title_and_notes() {
        let mut store = Snapshot::new(Vec::new());
        let card = store.create_card("Old", "").unwrap().upserts[0].clone();
        let updated = store
            .update_card(&card.id, "New", "notes", None, Priority::Medium)
            .unwrap()
            .upserts[0]
            .clone();
        assert_eq!(updated.title, "New");
        assert_eq!(updated.notes, "notes");
        assert_eq!(titles_in(&store, Status::Todo), vec!["New"]);
    }

    #[test]
    fn update_missing_card_fails() {
        let mut store = Snapshot::new(Vec::new());
        assert!(matches!(
            store.update_card("missing", "X", "", None, Priority::Medium),
            Err(AppError::CardNotFound)
        ));
    }

    #[test]
    fn delete_card_removes_it() {
        let mut store = Snapshot::new(Vec::new());
        let card = store.create_card("Gone", "").unwrap().upserts[0].clone();
        store.delete_card(&card.id).unwrap();
        assert!(store.cards.clone().is_empty());
    }

    #[test]
    fn delete_missing_card_fails() {
        let mut store = Snapshot::new(Vec::new());
        assert!(matches!(
            store.delete_card("missing"),
            Err(AppError::CardNotFound)
        ));
    }

    #[test]
    fn move_card_to_doing() {
        let mut store = Snapshot::new(Vec::new());
        let card = store.create_card("A", "").unwrap().upserts[0].clone();
        let moved = store
            .move_card(&card.id, Status::Doing, None)
            .unwrap()
            .upserts[0]
            .clone();
        assert_eq!(moved.status, Status::Doing);
        assert!(titles_in(&store, Status::Todo).is_empty());
        assert_eq!(titles_in(&store, Status::Doing), vec!["A"]);
    }

    #[test]
    fn move_card_inserts_before_target() {
        let mut store = Snapshot::new(Vec::new());
        let a = store.create_card("A", "").unwrap().upserts[0].clone();
        store.create_card("B", "").unwrap();
        let c = store.create_card("C", "").unwrap().upserts[0].clone();
        store.move_card(&c.id, Status::Todo, Some(&a.id)).unwrap();
        assert_eq!(titles_in(&store, Status::Todo), vec!["C", "A", "B"]);
    }

    #[test]
    fn move_card_appends_when_before_id_is_none() {
        let mut store = Snapshot::new(Vec::new());
        let a = store.create_card("A", "").unwrap().upserts[0].clone();
        store.create_card("B", "").unwrap();
        store.move_card(&a.id, Status::Done, None).unwrap();
        let b = store.create_card("Later", "").unwrap().upserts[0].clone();
        store.move_card(&b.id, Status::Done, None).unwrap();
        assert_eq!(titles_in(&store, Status::Done), vec!["A", "Later"]);
    }

    #[test]
    fn move_missing_card_fails() {
        let mut store = Snapshot::new(Vec::new());
        assert!(matches!(
            store.move_card("missing", Status::Done, None),
            Err(AppError::CardNotFound)
        ));
    }

    #[test]
    fn move_before_unknown_target_fails() {
        let mut store = Snapshot::new(Vec::new());
        let card = store.create_card("A", "").unwrap().upserts[0].clone();
        assert!(matches!(
            store.move_card(&card.id, Status::Doing, Some("nope")),
            Err(AppError::InvalidTarget)
        ));
    }

    #[test]
    fn create_card_defaults_medium_priority_and_no_deadline() {
        let mut store = Snapshot::new(Vec::new());
        let card = store.create_card("A", "").unwrap().upserts[0].clone();
        assert_eq!(card.priority, Priority::Medium);
        assert_eq!(card.deadline_at, None);
    }

    #[test]
    fn update_card_persists_deadline_and_priority() {
        let mut store = Snapshot::new(Vec::new());
        let card = store.create_card("A", "").unwrap().upserts[0].clone();
        let updated = store
            .update_card(&card.id, "A", "", Some(1_777_000_000), Priority::High)
            .unwrap()
            .upserts[0]
            .clone();
        assert_eq!(updated.deadline_at, Some(1_777_000_000));
        assert_eq!(updated.priority, Priority::High);
        let listed = store.cards.clone();
        assert_eq!(listed[0].deadline_at, Some(1_777_000_000));
        assert_eq!(listed[0].priority, Priority::High);
    }

    #[test]
    fn create_card_with_keeps_deadline() {
        let mut store = Snapshot::new(Vec::new());
        let card = store
            .create_card_with("A", "", Some(99), Priority::Low)
            .unwrap()
            .upserts[0]
            .clone();
        assert_eq!(card.deadline_at, Some(99));
        assert_eq!(card.priority, Priority::Low);
    }

    #[test]
    fn create_card_starts_with_overdue_marks() {
        let mut store = Snapshot::new(Vec::new());
        let card = store.create_card("A", "").unwrap().upserts[0].clone();
        assert!(!card.overdue_cleared);
    }

    #[test]
    fn moving_to_done_clears_overdue_marks() {
        let mut store = Snapshot::new(Vec::new());
        let card = store
            .create_card_with("A", "", Some(99), Priority::Medium)
            .unwrap()
            .upserts[0]
            .clone();
        let moved = store
            .move_card(&card.id, Status::Done, None)
            .unwrap()
            .upserts[0]
            .clone();
        assert!(moved.overdue_cleared);
        assert_eq!(moved.deadline_at, Some(99));
        let back = store
            .move_card(&card.id, Status::Todo, None)
            .unwrap()
            .upserts[0]
            .clone();
        assert!(!back.overdue_cleared);
        assert_eq!(back.deadline_at, Some(99));
    }
}
