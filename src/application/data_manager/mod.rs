//! Database authority: exclusive SQLite ownership and the public save interface.
//! UI edits a working snapshot; successful receipts confirm committed changes.

mod store;
mod thread;
mod persistence;

use super::snapshot::{AppError, Changes};
pub use persistence::Persistence;
pub use thread::{DataClient, DataManager};

#[derive(Debug)]
pub struct PersistBatch {
    pub seq: u64,
    pub changes: Changes,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PersistResult {
    pub seq: u64,
    pub error: Option<String>,
}

impl From<rusqlite::Error> for AppError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Db(value.to_string())
    }
}
