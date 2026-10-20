use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Row};

use super::domain::{normalize_title, overdue_cleared_for, sort_between, Card, Priority, Status};
use super::error::AppError;

pub struct Store {
    conn: Connection,
}

impl Store {
    pub fn open(path: &Path) -> Result<Self, AppError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|err| AppError::Db(err.to_string()))?;
            }
        }
        let conn = Connection::open(path)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        Self::from_connection(conn)
    }

    pub fn open_in_memory() -> Result<Self, AppError> {
        Self::from_connection(Connection::open_in_memory()?)
    }

    fn from_connection(conn: Connection) -> Result<Self, AppError> {
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS cards (
                id TEXT PRIMARY KEY,
                title TEXT NOT NULL,
                notes TEXT NOT NULL DEFAULT '',
                status TEXT NOT NULL CHECK (status IN ('todo', 'doing', 'done')),
                sort_order REAL NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL,
                deadline_at INTEGER,
                priority TEXT NOT NULL DEFAULT 'medium'
                    CHECK (priority IN ('low', 'medium', 'high')),
                overdue_cleared INTEGER NOT NULL DEFAULT 0
            );
            CREATE INDEX IF NOT EXISTS idx_cards_status_order
                ON cards (status, sort_order);",
        )?;
        migrate_cards(&conn)?;
        Ok(Self { conn })
    }

    pub fn create_card(&mut self, title: &str, notes: &str) -> Result<Card, AppError> {
        self.create_card_with(title, notes, None, Priority::Medium)
    }

    pub fn create_card_with(
        &mut self,
        title: &str,
        notes: &str,
        deadline_at: Option<i64>,
        priority: Priority,
    ) -> Result<Card, AppError> {
        let title = normalize_title(title)?;
        let now = now_secs();
        let id = uuid::Uuid::new_v4().to_string();
        let column = list_column(&self.conn, Status::Todo, None)?;
        let sort_order = sort_between(column.last().map(|card| card.sort_order), None);
        self.conn.execute(
            "INSERT INTO cards (id, title, notes, status, sort_order, created_at, updated_at, deadline_at, priority, overdue_cleared)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                id,
                title,
                notes,
                Status::Todo.as_str(),
                sort_order,
                now,
                now,
                deadline_at,
                priority.as_str(),
                overdue_cleared_for(Status::Todo),
            ],
        )?;
        load_card(&self.conn, &id)
    }

    pub fn list_cards(&self) -> Result<Vec<Card>, AppError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, title, notes, status, sort_order, created_at, updated_at, deadline_at, priority, overdue_cleared
             FROM cards
             ORDER BY CASE status
                 WHEN 'todo' THEN 0
                 WHEN 'doing' THEN 1
                 WHEN 'done' THEN 2
                 ELSE 3
             END, sort_order, created_at",
        )?;
        let rows = stmt.query_map([], card_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
    }

    pub fn update_card(
        &mut self,
        id: &str,
        title: &str,
        notes: &str,
        deadline_at: Option<i64>,
        priority: Priority,
    ) -> Result<Card, AppError> {
        let title = normalize_title(title)?;
        let changed = self.conn.execute(
            "UPDATE cards SET title = ?1, notes = ?2, deadline_at = ?3, priority = ?4, updated_at = ?5 WHERE id = ?6",
            params![title, notes, deadline_at, priority.as_str(), now_secs(), id],
        )?;
        if changed == 0 {
            return Err(AppError::CardNotFound);
        }
        load_card(&self.conn, id)
    }

    pub fn delete_card(&mut self, id: &str) -> Result<(), AppError> {
        let changed = self.conn.execute("DELETE FROM cards WHERE id = ?1", [id])?;
        if changed == 0 {
            return Err(AppError::CardNotFound);
        }
        Ok(())
    }

    pub fn move_card(
        &mut self,
        id: &str,
        to_status: Status,
        before_id: Option<&str>,
    ) -> Result<Card, AppError> {
        if before_id == Some(id) {
            return load_card(&self.conn, id);
        }

        let tx = self.conn.transaction()?;
        load_card(&tx, id)?;
        let column = list_column(&tx, to_status, Some(id))?;
        let (prev, next) = neighbors(&column, before_id)?;
        let sort_order = sort_between(prev, next);
        let now = now_secs();

        if collapsed(prev, next, sort_order) {
            let ids = ordered_ids(&column, id, before_id)?;
            for (index, card_id) in ids.iter().enumerate() {
                tx.execute(
                    "UPDATE cards SET status = ?1, sort_order = ?2, updated_at = ?3, overdue_cleared = ?4 WHERE id = ?5",
                    params![
                        to_status.as_str(),
                        (index as f64) + 1.0,
                        now,
                        overdue_cleared_for(to_status),
                        card_id
                    ],
                )?;
            }
        } else {
            tx.execute(
                "UPDATE cards SET status = ?1, sort_order = ?2, updated_at = ?3, overdue_cleared = ?4 WHERE id = ?5",
                params![
                    to_status.as_str(),
                    sort_order,
                    now,
                    overdue_cleared_for(to_status),
                    id
                ],
            )?;
        }
        tx.commit()?;
        load_card(&self.conn, id)
    }
}

fn migrate_cards(conn: &Connection) -> Result<(), AppError> {
    let names = column_names(conn)?;
    if !names.iter().any(|name| name == "deadline_at") {
        conn.execute("ALTER TABLE cards ADD COLUMN deadline_at INTEGER", [])?;
    }
    if !names.iter().any(|name| name == "priority") {
        conn.execute(
            "ALTER TABLE cards ADD COLUMN priority TEXT NOT NULL DEFAULT 'medium'",
            [],
        )?;
    }
    if !names.iter().any(|name| name == "overdue_cleared") {
        conn.execute(
            "ALTER TABLE cards ADD COLUMN overdue_cleared INTEGER NOT NULL DEFAULT 0",
            [],
        )?;
        conn.execute(
            "UPDATE cards SET overdue_cleared = 1 WHERE status = 'done'",
            [],
        )?;
    }
    Ok(())
}

fn column_names(conn: &Connection) -> Result<Vec<String>, AppError> {
    let mut stmt = conn.prepare("PRAGMA table_info(cards)")?;
    let names = stmt.query_map([], |row| row.get::<_, String>(1))?;
    names.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or(0)
}

fn card_from_row(row: &Row<'_>) -> rusqlite::Result<Card> {
    let status: String = row.get(3)?;
    let status = Status::parse(&status).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(err))
    })?;
    Ok(Card {
        id: row.get(0)?,
        title: row.get(1)?,
        notes: row.get(2)?,
        status,
        sort_order: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
        deadline_at: row.get(7)?,
        priority: {
            let value: String = row.get(8)?;
            Priority::parse(&value).map_err(|err| {
                rusqlite::Error::FromSqlConversionFailure(
                    8,
                    rusqlite::types::Type::Text,
                    Box::new(err),
                )
            })?
        },
        overdue_cleared: row.get::<_, i64>(9)? != 0,
    })
}

fn load_card(conn: &Connection, id: &str) -> Result<Card, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, notes, status, sort_order, created_at, updated_at, deadline_at, priority, overdue_cleared
         FROM cards WHERE id = ?1",
    )?;
    stmt.query_row([id], card_from_row)
        .optional()?
        .ok_or(AppError::CardNotFound)
}

fn list_column(
    conn: &Connection,
    status: Status,
    exclude_id: Option<&str>,
) -> Result<Vec<Card>, AppError> {
    let mut stmt = conn.prepare(
        "SELECT id, title, notes, status, sort_order, created_at, updated_at, deadline_at, priority, overdue_cleared
         FROM cards
         WHERE status = ?1 AND (?2 IS NULL OR id != ?2)
         ORDER BY sort_order, created_at",
    )?;
    let rows = stmt.query_map(params![status.as_str(), exclude_id], card_from_row)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(AppError::from)
}

fn neighbors(
    column: &[Card],
    before_id: Option<&str>,
) -> Result<(Option<f64>, Option<f64>), AppError> {
    match before_id {
        None => Ok((column.last().map(|card| card.sort_order), None)),
        Some(target) => {
            let index = column
                .iter()
                .position(|card| card.id == target)
                .ok_or(AppError::InvalidTarget)?;
            let prev = index
                .checked_sub(1)
                .and_then(|i| column.get(i))
                .map(|card| card.sort_order);
            Ok((prev, Some(column[index].sort_order)))
        }
    }
}

fn collapsed(prev: Option<f64>, next: Option<f64>, mid: f64) -> bool {
    prev.is_some_and(|value| mid <= value) || next.is_some_and(|value| mid >= value)
}

fn ordered_ids(
    column: &[Card],
    moving_id: &str,
    before_id: Option<&str>,
) -> Result<Vec<String>, AppError> {
    let mut ids: Vec<String> = column.iter().map(|card| card.id.clone()).collect();
    match before_id {
        None => ids.push(moving_id.to_string()),
        Some(target) => {
            let index = ids
                .iter()
                .position(|id| id == target)
                .ok_or(AppError::InvalidTarget)?;
            ids.insert(index, moving_id.to_string());
        }
    }
    Ok(ids)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::AppError;
    use rusqlite::Connection;

    fn titles_in(store: &Store, status: Status) -> Vec<String> {
        store
            .list_cards()
            .unwrap()
            .into_iter()
            .filter(|card| card.status == status)
            .map(|card| card.title)
            .collect()
    }

    #[test]
    fn create_card_lands_in_todo() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store.create_card("  Write plan  ", "details").unwrap();
        assert_eq!(card.title, "Write plan");
        assert_eq!(card.notes, "details");
        assert_eq!(card.status, Status::Todo);
        assert_eq!(titles_in(&store, Status::Todo), vec!["Write plan"]);
    }

    #[test]
    fn create_card_rejects_blank_title() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(matches!(
            store.create_card("   ", ""),
            Err(AppError::EmptyTitle)
        ));
    }

    #[test]
    fn later_cards_append_in_todo() {
        let mut store = Store::open_in_memory().unwrap();
        store.create_card("A", "").unwrap();
        store.create_card("B", "").unwrap();
        store.create_card("C", "").unwrap();
        assert_eq!(titles_in(&store, Status::Todo), vec!["A", "B", "C"]);
    }

    #[test]
    fn update_card_changes_title_and_notes() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store.create_card("Old", "").unwrap();
        let updated = store
            .update_card(&card.id, "New", "notes", None, Priority::Medium)
            .unwrap();
        assert_eq!(updated.title, "New");
        assert_eq!(updated.notes, "notes");
        assert_eq!(titles_in(&store, Status::Todo), vec!["New"]);
    }

    #[test]
    fn update_missing_card_fails() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(matches!(
            store.update_card("missing", "X", "", None, Priority::Medium),
            Err(AppError::CardNotFound)
        ));
    }

    #[test]
    fn delete_card_removes_it() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store.create_card("Gone", "").unwrap();
        store.delete_card(&card.id).unwrap();
        assert!(store.list_cards().unwrap().is_empty());
    }

    #[test]
    fn delete_missing_card_fails() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(matches!(
            store.delete_card("missing"),
            Err(AppError::CardNotFound)
        ));
    }

    #[test]
    fn move_card_to_doing() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store.create_card("A", "").unwrap();
        let moved = store.move_card(&card.id, Status::Doing, None).unwrap();
        assert_eq!(moved.status, Status::Doing);
        assert!(titles_in(&store, Status::Todo).is_empty());
        assert_eq!(titles_in(&store, Status::Doing), vec!["A"]);
    }

    #[test]
    fn move_card_inserts_before_target() {
        let mut store = Store::open_in_memory().unwrap();
        let a = store.create_card("A", "").unwrap();
        store.create_card("B", "").unwrap();
        let c = store.create_card("C", "").unwrap();
        store.move_card(&c.id, Status::Todo, Some(&a.id)).unwrap();
        assert_eq!(titles_in(&store, Status::Todo), vec!["C", "A", "B"]);
    }

    #[test]
    fn move_card_appends_when_before_id_is_none() {
        let mut store = Store::open_in_memory().unwrap();
        let a = store.create_card("A", "").unwrap();
        store.create_card("B", "").unwrap();
        store.move_card(&a.id, Status::Done, None).unwrap();
        let b = store.create_card("Later", "").unwrap();
        store.move_card(&b.id, Status::Done, None).unwrap();
        assert_eq!(titles_in(&store, Status::Done), vec!["A", "Later"]);
    }

    #[test]
    fn move_missing_card_fails() {
        let mut store = Store::open_in_memory().unwrap();
        assert!(matches!(
            store.move_card("missing", Status::Done, None),
            Err(AppError::CardNotFound)
        ));
    }

    #[test]
    fn move_before_unknown_target_fails() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store.create_card("A", "").unwrap();
        assert!(matches!(
            store.move_card(&card.id, Status::Doing, Some("nope")),
            Err(AppError::InvalidTarget)
        ));
    }

    #[test]
    fn file_store_survives_reopen() {
        let dir = std::env::temp_dir().join(format!("kanban-test-{}", uuid::Uuid::new_v4()));
        let path = dir.join("kanban.db");
        {
            let mut store = Store::open(&path).unwrap();
            store.create_card("Persisted", "n").unwrap();
        }
        let store = Store::open(&path).unwrap();
        assert_eq!(titles_in(&store, Status::Todo), vec!["Persisted"]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn migrates_old_cards_table() {
        let dir = std::env::temp_dir().join(format!("kanban-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kanban.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE cards (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    notes TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL,
                    sort_order REAL NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL
                );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO cards (id, title, notes, status, sort_order, created_at, updated_at)
                 VALUES ('c1', 'Old', '', 'todo', 1.0, 1, 1)",
                [],
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let card = &store.list_cards().unwrap()[0];
        assert_eq!(card.title, "Old");
        assert_eq!(card.priority, Priority::Medium);
        assert_eq!(card.deadline_at, None);
        assert!(!card.overdue_cleared);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn create_card_defaults_medium_priority_and_no_deadline() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store.create_card("A", "").unwrap();
        assert_eq!(card.priority, Priority::Medium);
        assert_eq!(card.deadline_at, None);
    }

    #[test]
    fn update_card_persists_deadline_and_priority() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store.create_card("A", "").unwrap();
        let updated = store
            .update_card(&card.id, "A", "", Some(1_777_000_000), Priority::High)
            .unwrap();
        assert_eq!(updated.deadline_at, Some(1_777_000_000));
        assert_eq!(updated.priority, Priority::High);
        let listed = store.list_cards().unwrap();
        assert_eq!(listed[0].deadline_at, Some(1_777_000_000));
        assert_eq!(listed[0].priority, Priority::High);
    }

    #[test]
    fn create_card_with_keeps_deadline() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store
            .create_card_with("A", "", Some(99), Priority::Low)
            .unwrap();
        assert_eq!(card.deadline_at, Some(99));
        assert_eq!(card.priority, Priority::Low);
    }

    #[test]
    fn create_card_starts_with_overdue_marks() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store.create_card("A", "").unwrap();
        assert!(!card.overdue_cleared);
    }

    #[test]
    fn moving_to_done_clears_overdue_marks() {
        let mut store = Store::open_in_memory().unwrap();
        let card = store
            .create_card_with("A", "", Some(99), Priority::Medium)
            .unwrap();
        let moved = store.move_card(&card.id, Status::Done, None).unwrap();
        assert!(moved.overdue_cleared);
        assert_eq!(moved.deadline_at, Some(99));
        let back = store.move_card(&card.id, Status::Todo, None).unwrap();
        assert!(!back.overdue_cleared);
        assert_eq!(back.deadline_at, Some(99));
    }

    #[test]
    fn migrates_done_cards_to_cleared_overdue_marks() {
        let dir = std::env::temp_dir().join(format!("kanban-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kanban.db");
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE cards (
                    id TEXT PRIMARY KEY,
                    title TEXT NOT NULL,
                    notes TEXT NOT NULL DEFAULT '',
                    status TEXT NOT NULL,
                    sort_order REAL NOT NULL,
                    created_at INTEGER NOT NULL,
                    updated_at INTEGER NOT NULL,
                    deadline_at INTEGER,
                    priority TEXT NOT NULL DEFAULT 'medium'
                );",
            )
            .unwrap();
            conn.execute(
                "INSERT INTO cards (id, title, notes, status, sort_order, created_at, updated_at, deadline_at, priority)
                 VALUES ('c1', 'Done', '', 'done', 1.0, 1, 1, 99, 'medium')",
                [],
            )
            .unwrap();
        }
        let store = Store::open(&path).unwrap();
        let card = &store.list_cards().unwrap()[0];
        assert!(card.overdue_cleared);
        assert_eq!(card.deadline_at, Some(99));
        let _ = std::fs::remove_dir_all(dir);
    }
}
