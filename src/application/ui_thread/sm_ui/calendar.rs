//! Month grid for the editor date picker. Week starts Monday.

use chrono::Datelike;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ymd {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthCell {
    pub ymd: Ymd,
    pub in_month: bool,
}

pub fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let mut m = month as i32 - 1 + delta;
    let mut y = year;
    while m < 0 {
        m += 12;
        y -= 1;
    }
    while m >= 12 {
        m -= 12;
        y += 1;
    }
    (y, (m + 1) as u32)
}

pub fn month_cells(year: i32, month: u32) -> Vec<MonthCell> {
    let first = chrono::NaiveDate::from_ymd_opt(year, month, 1)
        .unwrap_or_else(|| chrono::NaiveDate::from_ymd_opt(year, 1, 1).unwrap());
    let pad = first.weekday().num_days_from_monday() as i64;
    let start = first - chrono::Duration::days(pad);
    (0..42)
        .map(|i| {
            let date = start + chrono::Duration::days(i);
            MonthCell {
                ymd: Ymd {
                    year: date.year(),
                    month: date.month(),
                    day: date.day(),
                },
                in_month: date.year() == year && date.month() == month,
            }
        })
        .collect()
}

pub fn ymd_from_date_string(value: &str) -> Option<Ymd> {
    let value = value.trim();
    if value.len() != 10 {
        return None;
    }
    let bytes = value.as_bytes();
    if bytes[4] != b'-' || bytes[7] != b'-' {
        return None;
    }
    Some(Ymd {
        year: std::str::from_utf8(&bytes[0..4]).ok()?.parse().ok()?,
        month: std::str::from_utf8(&bytes[5..7]).ok()?.parse().ok()?,
        day: std::str::from_utf8(&bytes[8..10]).ok()?.parse().ok()?,
    })
}

pub fn date_string_from_ymd(ymd: Ymd) -> String {
    format!("{:04}-{:02}-{:02}", ymd.year, ymd.month, ymd.day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_grid_is_six_weeks_monday_first() {
        let cells = month_cells(2026, 8);
        assert_eq!(cells.len(), 42);
        assert_eq!(
            cells[0],
            MonthCell {
                ymd: Ymd {
                    year: 2026,
                    month: 7,
                    day: 27
                },
                in_month: false,
            }
        );
        let first = cells.iter().find(|c| c.in_month).unwrap();
        assert_eq!(first.ymd.day, 1);
        assert_eq!(first.ymd.month, 8);
        assert_eq!(cells[5].ymd.day, 1);
        assert!(cells[5].in_month);
    }

    #[test]
    fn shift_month_wraps_the_year() {
        assert_eq!(shift_month(2026, 1, -1), (2025, 12));
        assert_eq!(shift_month(2026, 12, 1), (2027, 1));
        assert_eq!(shift_month(2026, 8, 0), (2026, 8));
    }

    #[test]
    fn date_string_round_trips_ymd() {
        let ymd = Ymd {
            year: 2026,
            month: 8,
            day: 24,
        };
        assert_eq!(date_string_from_ymd(ymd), "2026-08-24");
        assert_eq!(ymd_from_date_string("2026-08-24"), Some(ymd));
        assert_eq!(ymd_from_date_string(""), None);
    }
}
