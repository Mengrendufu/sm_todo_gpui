use serde::{Deserialize, Serialize};

use super::error::AppError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Todo,
    Doing,
    Done,
}

impl Status {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Todo => "todo",
            Self::Doing => "doing",
            Self::Done => "done",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "todo" => Ok(Self::Todo),
            "doing" => Ok(Self::Doing),
            "done" => Ok(Self::Done),
            _ => Err(AppError::InvalidStatus),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    Low,
    Medium,
    High,
}

impl Priority {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }

    pub fn parse(value: &str) -> Result<Self, AppError> {
        match value {
            "low" => Ok(Self::Low),
            "medium" => Ok(Self::Medium),
            "high" => Ok(Self::High),
            _ => Err(AppError::InvalidPriority),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Card {
    pub id: String,
    pub title: String,
    pub notes: String,
    pub status: Status,
    pub sort_order: f64,
    pub created_at: i64,
    pub updated_at: i64,
    pub deadline_at: Option<i64>,
    pub priority: Priority,
    pub overdue_cleared: bool,
}

pub fn overdue_cleared_for(status: Status) -> bool {
    matches!(status, Status::Done)
}

pub fn normalize_title(title: &str) -> Result<String, AppError> {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return Err(AppError::EmptyTitle);
    }
    Ok(trimmed.to_string())
}

/// Insert between `prev` (card above) and `next` (card below).
pub fn sort_between(prev: Option<f64>, next: Option<f64>) -> f64 {
    match (prev, next) {
        (None, None) => 1.0,
        (Some(p), None) => p + 1.0,
        (None, Some(n)) => n - 1.0,
        (Some(p), Some(n)) => (p + n) / 2.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_blank_title() {
        assert!(matches!(normalize_title(""), Err(AppError::EmptyTitle)));
        assert!(matches!(
            normalize_title("   \t"),
            Err(AppError::EmptyTitle)
        ));
    }

    #[test]
    fn trims_title() {
        assert_eq!(normalize_title("  hello  ").unwrap(), "hello");
    }

    #[test]
    fn parses_known_status() {
        assert_eq!(Status::parse("todo").unwrap(), Status::Todo);
        assert_eq!(Status::parse("doing").unwrap(), Status::Doing);
        assert_eq!(Status::parse("done").unwrap(), Status::Done);
        assert_eq!(Status::Todo.as_str(), "todo");
    }

    #[test]
    fn rejects_unknown_status() {
        assert!(matches!(
            Status::parse("blocked"),
            Err(AppError::InvalidStatus)
        ));
    }

    #[test]
    fn parses_known_priority() {
        assert_eq!(Priority::parse("low").unwrap(), Priority::Low);
        assert_eq!(Priority::parse("medium").unwrap(), Priority::Medium);
        assert_eq!(Priority::parse("high").unwrap(), Priority::High);
        assert_eq!(Priority::High.as_str(), "high");
    }

    #[test]
    fn rejects_unknown_priority() {
        assert!(matches!(
            Priority::parse("urgent"),
            Err(AppError::InvalidPriority)
        ));
    }

    #[test]
    fn sort_between_empty_column() {
        assert_eq!(sort_between(None, None), 1.0);
    }

    #[test]
    fn sort_between_appends_after_last() {
        assert_eq!(sort_between(Some(3.0), None), 4.0);
    }

    #[test]
    fn sort_between_prepends_before_first() {
        assert_eq!(sort_between(None, Some(2.0)), 1.0);
    }

    #[test]
    fn sort_between_midpoint() {
        assert_eq!(sort_between(Some(2.0), Some(4.0)), 3.0);
    }
}
