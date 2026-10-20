use std::path::Path;

use rusqlite::{params, Connection, Row};

use super::AppError;
use super::Changes;
use crate::application::snapshot::{Card, Priority, Status};

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

    /// Apply exactly the UI's frozen values; one batch is one transaction.
    pub fn apply(&mut self, changes: &Changes) -> Result<(), AppError> {
        let tx = self.conn.transaction()?;
        for card in &changes.upserts {
            tx.execute(
                "INSERT INTO cards (id,title,notes,status,sort_order,created_at,updated_at,deadline_at,priority,overdue_cleared)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)
                 ON CONFLICT(id) DO UPDATE SET title=excluded.title,notes=excluded.notes,
                 status=excluded.status,sort_order=excluded.sort_order,created_at=excluded.created_at,
                 updated_at=excluded.updated_at,deadline_at=excluded.deadline_at,
                 priority=excluded.priority,overdue_cleared=excluded.overdue_cleared",
                params![card.id,card.title,card.notes,card.status.as_str(),card.sort_order,
                    card.created_at,card.updated_at,card.deadline_at,card.priority.as_str(),card.overdue_cleared],
            )?;
        }
        for id in &changes.deletes {
            tx.execute("DELETE FROM cards WHERE id=?1", [id])?;
        }
        tx.commit()?;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;
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
