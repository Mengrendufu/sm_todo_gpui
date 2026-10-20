//! Card face labels. Same wording as the Tauri `cardMeta` helpers.

use std::time::{SystemTime, UNIX_EPOCH};

use crate::application::snapshot::Priority;

const HOUR_MS: i64 = 60 * 60 * 1000;
const DAY_HOURS: i64 = 24;

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

pub fn priority_label(priority: Priority) -> &'static str {
    match priority {
        Priority::Low => "低",
        Priority::Medium => "中",
        Priority::High => "高",
    }
}

fn hour_diff(deadline_at: i64, now_ms: i64) -> i64 {
    let diff_ms = deadline_at.saturating_mul(1000) - now_ms;
    if diff_ms >= 0 {
        diff_ms / HOUR_MS
    } else {
        -((-diff_ms) / HOUR_MS)
    }
}

pub fn remaining_label(deadline_at: Option<i64>, now_ms: i64) -> Option<String> {
    let deadline_at = deadline_at?;
    let hours = hour_diff(deadline_at, now_ms);
    if hours >= DAY_HOURS {
        Some(format!("还剩 {} 天", hours / DAY_HOURS))
    } else if hours >= 1 {
        Some(format!("还剩 {hours} 小时"))
    } else if hours == 0 && deadline_at.saturating_mul(1000) >= now_ms {
        Some("不到 1 小时".to_string())
    } else if hours == 0 {
        Some("已逾期不到 1 小时".to_string())
    } else if hours <= -DAY_HOURS {
        Some(format!("已逾期 {} 天", -hours / DAY_HOURS))
    } else {
        Some(format!("已逾期 {} 小时", -hours))
    }
}

pub fn remaining_label_for_card(
    deadline_at: Option<i64>,
    overdue_cleared: bool,
    now_ms: i64,
) -> Option<String> {
    if overdue_cleared {
        return None;
    }
    remaining_label(deadline_at, now_ms)
}

pub fn is_overdue(deadline_at: Option<i64>, now_ms: i64) -> bool {
    deadline_at.is_some_and(|deadline| deadline.saturating_mul(1000) < now_ms)
}

pub fn is_overdue_for_card(deadline_at: Option<i64>, overdue_cleared: bool, now_ms: i64) -> bool {
    !overdue_cleared && is_overdue(deadline_at, now_ms)
}

pub fn deadline_label(deadline_at: Option<i64>) -> Option<String> {
    use chrono::{Datelike, TimeZone, Timelike};
    let secs = deadline_at?;
    let local = chrono::Local.timestamp_opt(secs, 0).single()?;
    Some(format!(
        "截止 {}/{} {:02}:{:02}",
        local.month(),
        local.day(),
        local.hour(),
        local.minute()
    ))
}

pub const DEFAULT_DEADLINE_HOUR: u32 = 18;
pub const DEFAULT_DEADLINE_MINUTE: u32 = 0;

pub fn deadline_from_date_and_hour(value: &str, hour: u32) -> Option<i64> {
    deadline_from_date_and_time(value, hour, 0)
}

pub fn deadline_from_date_and_time(value: &str, hour: u32, minute: u32) -> Option<i64> {
    use chrono::{TimeZone, NaiveDate};
    let value = value.trim();
    if value.len() != 10 {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    let year: i32 = std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?;
    let month: u32 = std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?;
    let day: u32 = std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?;
    NaiveDate::from_ymd_opt(year, month, day)?;
    let hour = hour.min(23);
    let minute = if minute >= 30 { 30 } else { 0 };
    chrono::Local
        .with_ymd_and_hms(year, month, day, hour, minute, 0)
        .single()
        .map(|t| t.timestamp())
}

pub fn date_input_from_deadline(deadline_at: Option<i64>) -> String {
    use chrono::{Datelike, TimeZone};
    let Some(secs) = deadline_at else {
        return String::new();
    };
    let Some(local) = chrono::Local.timestamp_opt(secs, 0).single() else {
        return String::new();
    };
    format!("{:04}-{:02}-{:02}", local.year(), local.month(), local.day())
}

pub fn hour_from_deadline(deadline_at: Option<i64>) -> u32 {
    time_from_deadline(deadline_at).0
}

pub fn time_from_deadline(deadline_at: Option<i64>) -> (u32, u32) {
    use chrono::{TimeZone, Timelike};
    let Some(secs) = deadline_at else {
        return (DEFAULT_DEADLINE_HOUR, DEFAULT_DEADLINE_MINUTE);
    };
    chrono::Local
        .timestamp_opt(secs, 0)
        .single()
        .map(|t| snap_to_half_hour(t.hour(), t.minute()))
        .unwrap_or((DEFAULT_DEADLINE_HOUR, DEFAULT_DEADLINE_MINUTE))
}

pub fn step_half_hour(hour: u32, minute: u32, delta: i32) -> (u32, u32) {
    let slot = hour.min(23) * 2 + if minute >= 30 { 1 } else { 0 };
    let next = (slot as i32 + delta).rem_euclid(48) as u32;
    (next / 2, if next % 2 == 0 { 0 } else { 30 })
}

fn snap_to_half_hour(hour: u32, minute: u32) -> (u32, u32) {
    if minute < 15 {
        (hour, 0)
    } else if minute < 45 {
        (hour, 30)
    } else {
        step_half_hour(hour, 0, 2)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    const NOW_MS: i64 = 1_777_000_000_000;

    fn deadline_secs_from_offset_ms(offset_ms: i64) -> i64 {
        (NOW_MS + offset_ms) / 1000
    }

    #[test]
    fn priority_label_matches_tauri() {
        assert_eq!(priority_label(Priority::Low), "低");
        assert_eq!(priority_label(Priority::Medium), "中");
        assert_eq!(priority_label(Priority::High), "高");
    }

    #[test]
    fn no_deadline_means_no_remaining_time() {
        assert_eq!(remaining_label(None, NOW_MS), None);
    }

    #[test]
    fn remaining_under_24_hours_uses_hours() {
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(HOUR_MS)), NOW_MS).as_deref(),
            Some("还剩 1 小时")
        );
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(23 * HOUR_MS)), NOW_MS).as_deref(),
            Some("还剩 23 小时")
        );
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(HOUR_MS / 2)), NOW_MS).as_deref(),
            Some("不到 1 小时")
        );
    }

    #[test]
    fn remaining_of_24_hours_or_more_uses_days() {
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(24 * HOUR_MS)), NOW_MS).as_deref(),
            Some("还剩 1 天")
        );
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(27 * HOUR_MS)), NOW_MS).as_deref(),
            Some("还剩 1 天")
        );
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(48 * HOUR_MS)), NOW_MS).as_deref(),
            Some("还剩 2 天")
        );
    }

    #[test]
    fn overdue_under_24_hours_uses_hours() {
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(-HOUR_MS)), NOW_MS).as_deref(),
            Some("已逾期 1 小时")
        );
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(-HOUR_MS / 2)), NOW_MS).as_deref(),
            Some("已逾期不到 1 小时")
        );
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(-23 * HOUR_MS)), NOW_MS).as_deref(),
            Some("已逾期 23 小时")
        );
    }

    #[test]
    fn overdue_of_24_hours_or_more_uses_days() {
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(-24 * HOUR_MS)), NOW_MS).as_deref(),
            Some("已逾期 1 天")
        );
        assert_eq!(
            remaining_label(Some(deadline_secs_from_offset_ms(-48 * HOUR_MS)), NOW_MS).as_deref(),
            Some("已逾期 2 天")
        );
    }

    #[test]
    fn cleared_overdue_hides_remaining() {
        let past = Some(deadline_secs_from_offset_ms(-24 * HOUR_MS));
        assert_eq!(remaining_label_for_card(past, true, NOW_MS), None);
        assert_eq!(
            remaining_label_for_card(past, false, NOW_MS).as_deref(),
            Some("已逾期 1 天")
        );
        assert!(!is_overdue_for_card(past, true, NOW_MS));
        assert!(is_overdue_for_card(past, false, NOW_MS));
    }

    #[test]
    fn deadline_label_is_none_without_deadline() {
        assert_eq!(deadline_label(None), None);
    }

    #[test]
    fn deadline_label_includes_local_month_day_and_hour() {
        let secs = chrono::Local
            .with_ymd_and_hms(2026, 8, 24, 18, 0, 0)
            .single()
            .unwrap()
            .timestamp();
        assert_eq!(deadline_label(Some(secs)).as_deref(), Some("截止 8/24 18:00"));
    }

    #[test]
    fn deadline_label_includes_half_hour_minutes() {
        let secs = chrono::Local
            .with_ymd_and_hms(2026, 8, 24, 18, 30, 0)
            .single()
            .unwrap()
            .timestamp();
        assert_eq!(deadline_label(Some(secs)).as_deref(), Some("截止 8/24 18:30"));
    }

    #[test]
    fn date_and_hour_round_trip_to_a_local_whole_hour() {
        let deadline = deadline_from_date_and_hour("2026-08-24", 18);
        assert_eq!(date_input_from_deadline(deadline), "2026-08-24");
        assert_eq!(hour_from_deadline(deadline), 18);
        assert_eq!(deadline_from_date_and_hour("", 18), None);
        assert_eq!(deadline_from_date_and_hour("2026-8-24", 18), None);
    }

    #[test]
    fn date_and_time_round_trip_includes_half_hour() {
        let deadline = deadline_from_date_and_time("2026-08-24", 18, 30);
        assert_eq!(date_input_from_deadline(deadline), "2026-08-24");
        assert_eq!(time_from_deadline(deadline), (18, 30));
        assert_eq!(deadline_from_date_and_time("", 18, 30), None);
    }

    #[test]
    fn hour_from_missing_deadline_defaults_to_18() {
        assert_eq!(hour_from_deadline(None), DEFAULT_DEADLINE_HOUR);
        assert_eq!(time_from_deadline(None), (DEFAULT_DEADLINE_HOUR, DEFAULT_DEADLINE_MINUTE));
        assert_eq!(date_input_from_deadline(None), "");
    }

    #[test]
    fn step_half_hour_walks_thirty_minute_slots() {
        assert_eq!(step_half_hour(18, 0, 1), (18, 30));
        assert_eq!(step_half_hour(18, 30, 1), (19, 0));
        assert_eq!(step_half_hour(23, 30, 1), (0, 0));
        assert_eq!(step_half_hour(0, 0, -1), (23, 30));
    }

    #[test]
    fn time_from_deadline_snaps_to_half_hour() {
        let early = chrono::Local
            .with_ymd_and_hms(2026, 8, 24, 18, 14, 0)
            .single()
            .unwrap()
            .timestamp();
        let mid = chrono::Local
            .with_ymd_and_hms(2026, 8, 24, 18, 20, 0)
            .single()
            .unwrap()
            .timestamp();
        let late = chrono::Local
            .with_ymd_and_hms(2026, 8, 24, 18, 50, 0)
            .single()
            .unwrap()
            .timestamp();
        assert_eq!(time_from_deadline(Some(early)), (18, 0));
        assert_eq!(time_from_deadline(Some(mid)), (18, 30));
        assert_eq!(time_from_deadline(Some(late)), (19, 0));
    }
}
