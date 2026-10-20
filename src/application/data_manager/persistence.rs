//! Database save interface used on the caller's thread.
//! Tracks requests and receipts; only the database worker touches SQLite.

use crate::application::snapshot::{Changes, Snapshot};
use super::{DataClient, PersistBatch, PersistResult};
use std::collections::{BTreeMap, HashMap, VecDeque};

pub struct Persistence {
    client: DataClient,
    next_seq: u64,
    outbox: VecDeque<PersistBatch>,
    pending: BTreeMap<u64, Vec<String>>,
    dirty: HashMap<String, u64>,
    error: Option<String>,
}

impl Persistence {
    pub fn new(client: DataClient) -> Self {
        Self {
            client,
            next_seq: 0,
            outbox: VecDeque::new(),
            pending: BTreeMap::new(),
            dirty: HashMap::new(),
            error: None,
        }
    }

    pub fn stage(&mut self, changes: Changes) {
        if changes.is_empty() {
            return;
        }
        self.next_seq += 1;
        let ids: Vec<_> = changes
            .upserts
            .iter()
            .map(|c| c.id.clone())
            .chain(changes.deletes.iter().cloned())
            .collect();
        for id in &ids {
            self.dirty.insert(id.clone(), self.next_seq);
        }
        self.pending.insert(self.next_seq, ids);
        self.outbox.push_back(PersistBatch {
            seq: self.next_seq,
            changes,
        });
    }

    pub fn has_outbox(&self) -> bool {
        !self.outbox.is_empty()
    }
    pub fn is_saved(&self) -> bool {
        self.dirty.is_empty() && self.pending.is_empty()
    }
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn post_pending(&mut self) {
        while let Some(batch) = self.outbox.pop_front() {
            let seq = batch.seq;
            if let Err(error) = self.client.post(batch) {
                self.complete(&PersistResult {
                    seq,
                    error: Some(error),
                });
            }
        }
    }

    pub fn complete(&mut self, result: &PersistResult) {
        let Some(ids) = self.pending.remove(&result.seq) else {
            return;
        };
        match &result.error {
            None => {
                for id in ids {
                    if self.dirty.get(&id) == Some(&result.seq) {
                        self.dirty.remove(&id);
                    }
                }
                if self.is_saved() {
                    self.error = None;
                }
            }
            Some(error) => {
                if ids.iter().any(|id| self.dirty.get(id) == Some(&result.seq)) {
                    self.error = Some(error.clone());
                }
            }
        }
    }

    /// Retry current working snapshot values, never replay an obsolete failed payload.
    pub fn retry(&mut self, snapshot: &Snapshot) {
        if !self.pending.is_empty() {
            return;
        }
        let mut changes = Changes::default();
        for id in self.dirty.keys() {
            match snapshot.cards.iter().find(|card| &card.id == id) {
                Some(card) => changes.upserts.push(card.clone()),
                None => changes.deletes.push(id.clone()),
            }
        }
        self.error = None;
        self.stage(changes);
        self.post_pending();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::DataManager;
    use crate::application::snapshot::Priority;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn old_ack_cannot_clear_a_newer_local_change() {
        let (tx, rx) = mpsc::channel();
        let (database, cards) = DataManager::open_in_memory(move |r| {
            tx.send(r).unwrap();
        })
        .unwrap();
        let mut snapshot = Snapshot::new(cards);
        let mut persistence = Persistence::new(database.client());
        let changes = snapshot.create_card("A", "").unwrap();
        let id = changes.upserts[0].id.clone();
        persistence.stage(changes);
        persistence.stage(
            snapshot
                .update_card(&id, "B", "", None, Priority::Medium)
                .unwrap(),
        );
        persistence.post_pending();
        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        persistence.complete(&first);
        assert!(!persistence.is_saved());
        persistence.complete(&rx.recv_timeout(Duration::from_secs(5)).unwrap());
        assert!(persistence.is_saved());
        assert_eq!(snapshot.cards[0].title, "B");
    }

    #[test]
    fn retry_uses_current_snapshot_and_cannot_resurrect_a_deleted_card() {
        let (tx, rx) = mpsc::channel();
        let (database, cards) = DataManager::open_in_memory(move |r| {
            tx.send(r).unwrap();
        })
        .unwrap();
        let mut snapshot = Snapshot::new(cards);
        let mut persistence = Persistence::new(database.client());
        let changes = snapshot.create_card("A", "").unwrap();
        let id = changes.upserts[0].id.clone();
        persistence.stage(changes);
        persistence.post_pending();
        let first = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        persistence.complete(&PersistResult {
            seq: first.seq,
            error: Some("write failed".into()),
        });
        assert!(persistence.error().is_some());
        // The working snapshot value has since become a deletion, not the failed old card.
        snapshot.delete_card(&id).unwrap();
        persistence.retry(&snapshot);
        let result = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(result.seq, 2);
        assert!(result.error.is_none());
        persistence.complete(&result);
        assert!(persistence.is_saved());
        assert!(snapshot.cards.is_empty());
    }

    #[test]
    fn stopped_worker_leaves_snapshot_dirty_and_reports_submission_error() {
        let (database, cards) = DataManager::open_in_memory(|_| {}).unwrap();
        let mut persistence = Persistence::new(database.client());
        let mut snapshot = Snapshot::new(cards);
        drop(database);
        persistence.stage(snapshot.create_card("unsaved", "").unwrap());
        persistence.post_pending();
        assert!(!persistence.is_saved());
        assert_eq!(persistence.error(), Some("数据库线程已退出"));
        assert_eq!(snapshot.cards[0].title, "unsaved");
    }
}
