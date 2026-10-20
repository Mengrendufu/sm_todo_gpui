//! Data I/O at Application level. Not an AO.
//! File-backed move writes go to a persist thread so the UI frame never waits on SQLite.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use kanban_core::{AppError, Card, Priority, Status, Store};

enum PersistJob {
    Move {
        id: String,
        to_status: Status,
        before_id: Option<String>,
    },
}

pub struct DataManager {
    store: Store,
    persist_tx: Option<mpsc::Sender<PersistJob>>,
    persist_thread: Option<std::thread::JoinHandle<()>>,
}

impl DataManager {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        let store = Store::open(path)?;
        let (tx, rx) = mpsc::channel();
        let worker_path = path.to_path_buf();
        let persist_thread = std::thread::Builder::new()
            .name("sm_todo-persist".into())
            .spawn(move || persist_loop(worker_path, rx))
            .map_err(|err| AppError::Db(err.to_string()))?;
        Ok(Self {
            store,
            persist_tx: Some(tx),
            persist_thread: Some(persist_thread),
        })
    }

    pub fn open_in_memory() -> Result<Self, AppError> {
        Ok(Self {
            store: Store::open_in_memory()?,
            persist_tx: None,
            persist_thread: None,
        })
    }

    pub fn default_db_path() -> PathBuf {
        let mut dir = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        dir.push("sm_todo");
        dir.join("kanban.db")
    }

    pub fn getCards(&self) -> Result<Vec<Card>, AppError> {
        self.store.list_cards()
    }

    pub fn createCard(&mut self, title: &str) -> Result<Card, AppError> {
        self.store.create_card(title, "")
    }

    pub fn createCardWith(
        &mut self,
        title: &str,
        notes: &str,
        deadline_at: Option<i64>,
        priority: Priority,
    ) -> Result<Card, AppError> {
        self.store
            .create_card_with(title, notes, deadline_at, priority)
    }

    pub fn updateCard(
        &mut self,
        id: &str,
        title: &str,
        notes: &str,
        deadline_at: Option<i64>,
        priority: Priority,
    ) -> Result<Card, AppError> {
        self.store
            .update_card(id, title, notes, deadline_at, priority)
    }

    pub fn deleteCard(&mut self, id: &str) -> Result<(), AppError> {
        self.store.delete_card(id)
    }

    pub fn moveCard(
        &mut self,
        id: &str,
        to_status: Status,
        before_id: Option<&str>,
    ) -> Result<Card, AppError> {
        self.store.move_card(id, to_status, before_id)
    }

    pub fn persists_off_thread(&self) -> bool {
        self.persist_tx.is_some()
    }

    pub fn persist_move(
        &mut self,
        id: &str,
        to_status: Status,
        before_id: Option<&str>,
    ) -> Result<(), AppError> {
        if let Some(tx) = &self.persist_tx {
            if tx
                .send(PersistJob::Move {
                    id: id.to_string(),
                    to_status,
                    before_id: before_id.map(str::to_string),
                })
                .is_ok()
            {
                return Ok(());
            }
        }
        self.store.move_card(id, to_status, before_id)?;
        Ok(())
    }
}

fn persist_loop(path: PathBuf, rx: mpsc::Receiver<PersistJob>) {
    let mut store = match Store::open(&path) {
        Ok(store) => store,
        Err(_) => return,
    };
    while let Ok(job) = rx.recv() {
        match job {
            PersistJob::Move {
                id,
                to_status,
                before_id,
            } => {
                let _ = store.move_card(&id, to_status, before_id.as_deref());
            }
        }
    }
}

impl Drop for DataManager {
    fn drop(&mut self) {
        self.persist_tx.take();
        if let Some(handle) = self.persist_thread.take() {
            let _ = handle.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn create_card_shows_in_getCards() {
        let mut dm = DataManager::open_in_memory().unwrap();
        dm.createCard("Hello").unwrap();
        let cards = dm.getCards().unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "Hello");
        assert_eq!(cards[0].status, Status::Todo);
    }

    #[test]
    fn create_card_with_keeps_notes_deadline_and_priority() {
        let mut dm = DataManager::open_in_memory().unwrap();
        let card = dm
            .createCardWith("买菜", "今晚", Some(99), Priority::High)
            .unwrap();
        assert_eq!(card.title, "买菜");
        assert_eq!(card.notes, "今晚");
        assert_eq!(card.deadline_at, Some(99));
        assert_eq!(card.priority, Priority::High);
    }

    #[test]
    fn file_store_persists_move_off_the_ui_thread() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("sm-todo-persist-{nanos}"));
        let path = dir.join("kanban.db");
        let mut dm = DataManager::open(&path).unwrap();
        assert!(dm.persists_off_thread());
        let card = dm.createCard("A").unwrap();
        dm.persist_move(&card.id, Status::Doing, None).unwrap();
        drop(dm);
        let reopened = DataManager::open(&path).unwrap();
        let cards = reopened.getCards().unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].status, Status::Doing);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn memory_store_persists_on_the_caller_thread() {
        let dm = DataManager::open_in_memory().unwrap();
        assert!(!dm.persists_off_thread());
    }
}
