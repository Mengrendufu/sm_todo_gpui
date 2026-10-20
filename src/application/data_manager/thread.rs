//! Owns the only SQLite connection. No live UI snapshot enters this thread.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::thread::{self, JoinHandle};

use super::{store::Store, AppError, PersistBatch, PersistResult};
use crate::application::snapshot::Card;

const QUEUE_CAPACITY: usize = 32;

enum Request {
    Persist(PersistBatch),
    Shutdown,
}

#[derive(Clone)]
pub struct DataClient {
    tx: SyncSender<Request>,
}

impl DataClient {
    /// Acceptance only; SQL completion arrives separately through the result callback.
    pub fn post(&self, batch: PersistBatch) -> Result<(), String> {
        self.tx
            .try_send(Request::Persist(batch))
            .map_err(|error| match error {
                TrySendError::Full(_) => "保存队列已满，请重试".into(),
                TrySendError::Disconnected(_) => "数据库线程已退出".into(),
            })
    }
}

/// Application-owned thread lifetime. UI owns only a DataClient, never a Store.
pub struct DataManager {
    tx: SyncSender<Request>,
    worker: Option<JoinHandle<()>>,
}

impl DataManager {
    pub fn open(
        path: &Path,
        on_result: impl Fn(PersistResult) + Send + 'static,
    ) -> Result<(Self, Vec<Card>), AppError> {
        Self::start(Some(path.to_path_buf()), on_result)
    }

    pub fn open_in_memory(
        on_result: impl Fn(PersistResult) + Send + 'static,
    ) -> Result<(Self, Vec<Card>), AppError> {
        Self::start(None, on_result)
    }

    fn start(
        path: Option<PathBuf>,
        on_result: impl Fn(PersistResult) + Send + 'static,
    ) -> Result<(Self, Vec<Card>), AppError> {
        let (tx, rx) = mpsc::sync_channel(QUEUE_CAPACITY);
        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let worker = thread::Builder::new()
            .name("sm_todo-db".into())
            .spawn(move || {
                let startup = (|| {
                    let store = match path {
                        Some(path) => Store::open(&path)?,
                        None => Store::open_in_memory()?,
                    };
                    let cards = store.list_cards()?;
                    Ok::<_, AppError>((store, cards))
                })();
                let mut store = match startup {
                    Ok((store, cards)) => {
                        if ready_tx.send(Ok(cards)).is_err() {
                            return;
                        }
                        store
                    }
                    Err(error) => {
                        let _ = ready_tx.send(Err(error));
                        return;
                    }
                };
                while let Ok(request) = rx.recv() {
                    match request {
                        Request::Persist(batch) => {
                            let error = store.apply(&batch.changes).err().map(|e| e.to_string());
                            on_result(PersistResult {
                                seq: batch.seq,
                                error,
                            });
                        }
                        Request::Shutdown => break,
                    }
                }
            })
            .map_err(|e| AppError::Db(e.to_string()))?;
        // This is startup, before the interactive GUI loop. SQL stays on the worker.
        let startup = ready_rx.recv().map_err(|e| AppError::Db(e.to_string()));
        match startup.and_then(|result| result) {
            Ok(cards) => Ok((
                Self {
                    tx,
                    worker: Some(worker),
                },
                cards,
            )),
            Err(error) => {
                let _ = worker.join();
                Err(error)
            }
        }
    }

    pub fn client(&self) -> DataClient {
        DataClient {
            tx: self.tx.clone(),
        }
    }

    pub fn default_db_path() -> PathBuf {
        let mut dir = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        dir.push("sm_todo");
        dir.join("kanban.db")
    }
}

impl Drop for DataManager {
    fn drop(&mut self) {
        // FIFO: accepted batches finish before Shutdown, even if clients still exist.
        let _ = self.tx.send(Request::Shutdown);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::snapshot::{Changes, Snapshot};
    use std::time::Duration;

    fn path() -> PathBuf {
        std::env::temp_dir()
            .join(format!("sm-todo-db-{}", uuid::Uuid::new_v4()))
            .join("kanban.db")
    }

    #[test]
    fn startup_and_frozen_batches_roundtrip_all_fields_on_worker_thread() {
        let path = path();
        let caller = thread::current().id();
        let (tx, rx) = mpsc::channel();
        let (database, cards) = DataManager::open(&path, move |r| {
            tx.send((thread::current().id(), r)).unwrap();
        })
        .unwrap();
        assert!(cards.is_empty());
        let client = database.client();
        let mut snapshot = Snapshot::new(cards);
        let changes = snapshot
            .create_card_with("Original", "notes", Some(123), crate::application::snapshot::Priority::High)
            .unwrap();
        let frozen = changes.upserts[0].clone();
        let id = frozen.id.clone();
        client.post(PersistBatch { seq: 1, changes }).unwrap();
        // Mutating UI memory does not mutate the already owned event payload.
        snapshot
            .update_card(
                &id,
                "Later local edit",
                "",
                None,
                crate::application::snapshot::Priority::Low,
            )
            .unwrap();
        let (worker, result) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_ne!(caller, worker);
        assert_eq!(
            result,
            PersistResult {
                seq: 1,
                error: None
            }
        );
        drop(database); // Must terminate with a live client handle.
        assert!(client
            .post(PersistBatch {
                seq: 2,
                changes: Changes::default()
            })
            .is_err());
        let (reopened, cards) = DataManager::open(&path, |_| {}).unwrap();
        assert_eq!(cards, vec![frozen]);
        drop(reopened);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn startup_failure_is_an_error_not_an_empty_snapshot() {
        let dir = path().parent().unwrap().to_path_buf();
        std::fs::create_dir_all(&dir).unwrap();
        assert!(DataManager::open(&dir, |_| {}).is_err());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn transaction_failure_is_reported_and_rolls_back_the_entire_batch() {
        let path = path();
        {
            let store = Store::open(&path).unwrap();
            // Add a real database failure without changing production validation.
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch("CREATE TRIGGER reject_card BEFORE INSERT ON cards WHEN NEW.title='reject' BEGIN SELECT RAISE(ABORT,'rejected'); END;").unwrap();
            drop(store);
        }
        let (tx, rx) = mpsc::channel();
        let (database, cards) = DataManager::open(&path, move |r| {
            tx.send(r).unwrap();
        })
        .unwrap();
        let mut snapshot = Snapshot::new(cards);
        let mut changes = snapshot.create_card("accepted", "").unwrap();
        changes
            .upserts
            .extend(snapshot.create_card("reject", "").unwrap().upserts);
        let client = database.client();
        client.post(PersistBatch { seq: 1, changes }).unwrap();
        assert!(rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .error
            .is_some());
        drop(database);
        let (reopened, cards) = DataManager::open(&path, |_| {}).unwrap();
        assert!(
            cards.is_empty(),
            "the earlier upsert must also be rolled back"
        );
        drop(reopened);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    #[test]
    fn bounded_queue_rejects_without_falling_back_to_caller_sql() {
        let (result_tx, result_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (database, _) = DataManager::open_in_memory(move |r| {
            let seq = r.seq;
            result_tx.send(r).unwrap();
            if seq == 1 {
                release_rx.recv().unwrap();
            }
        })
        .unwrap();
        let client = database.client();
        client
            .post(PersistBatch {
                seq: 1,
                changes: Changes::default(),
            })
            .unwrap();
        result_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let accepted = (0..QUEUE_CAPACITY).all(|i| {
            client
                .post(PersistBatch {
                    seq: i as u64 + 2,
                    changes: Changes::default(),
                })
                .is_ok()
        });
        let overflow = client.post(PersistBatch {
            seq: 100,
            changes: Changes::default(),
        });
        release_tx.send(()).unwrap();
        assert!(accepted);
        assert_eq!(overflow.unwrap_err(), "保存队列已满，请重试");
        drop(database);
    }

    #[test]
    fn shutdown_drains_accepted_upserts_and_deletes_before_closing() {
        let path = path();
        let (database, cards) = DataManager::open(&path, |_| {}).unwrap();
        let mut snapshot = Snapshot::new(cards);
        let client = database.client();
        let create = snapshot.create_card("A", "").unwrap();
        let id = create.upserts[0].id.clone();
        client
            .post(PersistBatch {
                seq: 1,
                changes: create,
            })
            .unwrap();
        client
            .post(PersistBatch {
                seq: 2,
                changes: snapshot.delete_card(&id).unwrap(),
            })
            .unwrap();
        client
            .post(PersistBatch {
                seq: 3,
                changes: snapshot.create_card("B", "kept").unwrap(),
            })
            .unwrap();
        drop(database);
        let (reopened, cards) = DataManager::open(&path, |_| {}).unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "B");
        assert_eq!(cards[0].notes, "kept");
        drop(reopened);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}

#[cfg(test)]
mod reorder_tests {
    use super::*;
    use crate::application::snapshot::{Changes, Snapshot, Status};
    use std::time::Duration;

    #[test]
    fn collapsed_sort_values_rewrite_all_affected_cards_in_one_batch() {
        let path = std::env::temp_dir()
            .join(format!("sm-todo-sort-{}", uuid::Uuid::new_v4()))
            .join("kanban.db");
        let (tx, rx) = mpsc::channel();
        let (database, cards) = DataManager::open(&path, move |r| {
            tx.send(r).unwrap();
        })
        .unwrap();
        let mut snapshot = Snapshot::new(cards);
        snapshot.create_card("A", "").unwrap();
        snapshot.create_card("B", "").unwrap();
        snapshot.create_card("C", "").unwrap();
        snapshot.cards[0].sort_order = 1.0;
        snapshot.cards[1].sort_order = 1.0 + f64::EPSILON;
        let moving = snapshot.cards[2].id.clone();
        let before = snapshot.cards[1].id.clone();
        let client = database.client();
        client
            .post(PersistBatch {
                seq: 1,
                changes: Changes {
                    upserts: snapshot.cards.clone(),
                    deletes: vec![],
                },
            })
            .unwrap();
        assert!(rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .error
            .is_none());
        let changes = snapshot
            .move_card(&moving, Status::Todo, Some(&before))
            .unwrap();
        assert_eq!(changes.upserts.len(), 3);
        assert_eq!(
            snapshot
                .cards
                .iter()
                .map(|c| c.title.as_str())
                .collect::<Vec<_>>(),
            vec!["A", "C", "B"]
        );
        client.post(PersistBatch { seq: 2, changes }).unwrap();
        assert!(rx
            .recv_timeout(Duration::from_secs(5))
            .unwrap()
            .error
            .is_none());
        drop(database);
        let (database, cards) = DataManager::open(&path, |_| {}).unwrap();
        assert_eq!(cards, snapshot.cards);
        drop(database);
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
}
