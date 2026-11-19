use chrono::{Datelike, Local};

/// Local wall-clock time in the SQLite text format the database stores.
pub fn local_timestamp() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

/// Index order is the order of the reference application's English date dropdown.
/// Both endpoints include the full local calendar day.
pub fn date_range(index: usize) -> Result<(String, String), String> {
    let now = Local::now().date_naive();
    range_for_day(
        index,
        Day {
            year: now.year(),
            month: now.month() as u16,
            day: now.day() as u16,
        },
        now.weekday().num_days_from_sunday() as u16,
    )
}

#[derive(Clone, Copy)]
struct Day {
    year: i32,
    month: u16,
    day: u16,
}

impl Day {
    fn months_before(self, months: i32) -> Self {
        let ordinal = self.year * 12 + i32::from(self.month) - 1 - months;
        let year = ordinal.div_euclid(12);
        let month = (ordinal.rem_euclid(12) + 1) as u16;
        Self {
            year,
            month,
            day: self.day.min(month_length(year, month)),
        }
    }

    fn days_before(self, days: u16) -> Self {
        if self.day > days {
            return Self {
                day: self.day - days,
                ..self
            };
        }
        // The supported presets move back by at most seven days in one call.
        let previous = self.months_before(1);
        Self {
            day: month_length(previous.year, previous.month) - (days - self.day),
            ..previous
        }
    }

    fn iso(self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

fn month_length(year: i32, month: u16) -> u16 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        _ => 31,
    }
}

fn range_for_day(
    index: usize,
    today: Day,
    weekday_sunday_zero: u16,
) -> Result<(String, String), String> {
    let month_start = Day { day: 1, ..today };
    let monday = today.days_before((weekday_sunday_zero + 6) % 7);
    let (start, end) = match index {
        0 => return Ok((String::new(), "9999-12-31 23:59:59".into())),
        1 => (today.days_before(1), today.days_before(1)),
        2 => (today.days_before(2), today),
        3 => (monday, today),
        4 => (monday.days_before(7), monday.days_before(1)),
        5 => (month_start, today),
        6 => (month_start.months_before(1), month_start.days_before(1)),
        7 => (today.months_before(3), today),
        8 => (today.months_before(6), today),
        _ => return Err("Unknown date filter".into()),
    };
    Ok((
        format!("{} 00:00:00", start.iso()),
        format!("{} 23:59:59", end.iso()),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_cover_local_dates_across_year_boundary() {
        let today = Day {
            year: 2025,
            month: 1,
            day: 1,
        };
        let cases = [
            ("", "9999-12-31"),
            ("2024-12-31", "2024-12-31"),
            ("2024-12-30", "2025-01-01"),
            ("2024-12-30", "2025-01-01"),
            ("2024-12-23", "2024-12-29"),
            ("2025-01-01", "2025-01-01"),
            ("2024-12-01", "2024-12-31"),
            ("2024-10-01", "2025-01-01"),
            ("2024-07-01", "2025-01-01"),
        ];
        for (i, (start, end)) in cases.into_iter().enumerate() {
            let start = if start.is_empty() {
                String::new()
            } else {
                format!("{start} 00:00:00")
            };
            assert_eq!(
                range_for_day(i, today, 3).unwrap(),
                (start, format!("{end} 23:59:59"))
            );
        }
        assert!(range_for_day(9, today, 3).is_err());
    }

    #[test]
    fn month_end_respects_leap_year_and_shorter_months() {
        let may = Day {
            year: 2024,
            month: 5,
            day: 31,
        };
        assert_eq!(range_for_day(7, may, 5).unwrap().0, "2024-02-29 00:00:00");
        let march = Day {
            year: 2100,
            month: 3,
            day: 1,
        };
        assert_eq!(range_for_day(1, march, 1).unwrap().0, "2100-02-28 00:00:00");
        let sunday = Day {
            year: 2024,
            month: 3,
            day: 3,
        };
        assert_eq!(
            range_for_day(3, sunday, 0).unwrap().0,
            "2024-02-26 00:00:00"
        );
    }
}
