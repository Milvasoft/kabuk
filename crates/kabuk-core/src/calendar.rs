//! The month the calendar panel shows (DECISIONS D88): its days laid out in weeks from
//! Monday, as the design's `calPanelHtml`. Pure.

/// Whether `year` has a 29th of February.
pub fn leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

/// The days of `month` (1 to 12) of `year`.
pub fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        2 if leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to a date, by Howard Hinnant's civil-from-days inverse.
fn day_number(year: i32, month: u32, day: u32) -> i64 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The day of the week of a date, 0 for Monday to 6 for Sunday.
pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    // 1970-01-01 is a Thursday: 3 days after Monday.
    u32::try_from((day_number(year, month, day) + 3).rem_euclid(7)).unwrap_or(0)
}

/// How many days `to` is after `from` (negative when before); dates as (year, month, day).
pub fn days_between(from: (i32, u32, u32), to: (i32, u32, u32)) -> i64 {
    day_number(to.0, to.1, to.2) - day_number(from.0, from.1, from.2)
}

/// A date's ISO 8601 week: weeks start on Monday and belong to the year of their Thursday.
pub fn iso_week(year: i32, month: u32, day: u32) -> u32 {
    let thursday = day_number(year, month, day) - i64::from(weekday(year, month, day)) + 3;
    let week_year = if thursday < day_number(year, 1, 1) {
        year - 1
    } else if thursday >= day_number(year + 1, 1, 1) {
        year + 1
    } else {
        year
    };
    u32::try_from((thursday - day_number(week_year, 1, 1)) / 7 + 1).unwrap_or(1)
}

/// A day on the month's sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridDay {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    /// Of the month the sheet is for, not a neighbour's.
    pub inside: bool,
}

/// Six weeks of seven days on every sheet, so the panel keeps its height month to month.
pub const GRID_DAYS: usize = 42;

/// The month's sheet in weeks from Monday: the end of the month before, its days, and the
/// start of the next, [`GRID_DAYS`] in all.
pub fn grid(year: i32, month: u32) -> Vec<GridDay> {
    let lead = weekday(year, month, 1);
    let (before_year, before_month) = shifted(year, month, -1);
    let before_days = days_in_month(before_year, before_month);
    let (after_year, after_month) = shifted(year, month, 1);
    let mine = days_in_month(year, month);
    let at = |year, month, day, inside| GridDay {
        year,
        month,
        day,
        inside,
    };
    (before_days + 1 - lead..=before_days)
        .map(|d| at(before_year, before_month, d, false))
        .chain((1..=mine).map(|d| at(year, month, d, true)))
        .chain((1..).map(|d| at(after_year, after_month, d, false)))
        .take(GRID_DAYS)
        .collect()
}

/// The month `by` months from `month` of `year` (negative goes back).
pub fn shifted(year: i32, month: u32, by: i32) -> (i32, u32) {
    let index = year * 12 + i32::try_from(month).unwrap_or(1) - 1 + by;
    (
        index.div_euclid(12),
        u32::try_from(index.rem_euclid(12)).unwrap_or(0) + 1,
    )
}

#[cfg(test)]
mod tests {
    //! Edge cases: February in a leap year, a century and a 400th year; known weekdays
    //! (including the epoch and dates before it); a month starting on a Monday (no empty
    //! cells) and on a Sunday (six); going back over January and forward over December.
    use super::*;

    #[test]
    fn months_have_their_days() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 10), 31);
        assert_eq!(days_in_month(2026, 11), 30);
    }

    #[test]
    fn known_dates_fall_on_their_weekdays() {
        assert_eq!(weekday(1970, 1, 1), 3, "a Thursday");
        assert_eq!(weekday(2026, 10, 22), 3, "a Thursday");
        assert_eq!(weekday(2026, 10, 1), 3);
        assert_eq!(weekday(2000, 1, 1), 5, "a Saturday");
        assert_eq!(weekday(1969, 12, 31), 2, "a Wednesday");
        assert_eq!(weekday(2024, 2, 29), 3);
    }

    #[test]
    fn a_month_is_laid_out_from_monday() {
        // June 2026 starts on a Monday: nothing of May.
        let june = grid(2026, 6);
        assert_eq!((june[0].month, june[0].day, june[0].inside), (6, 1, true));
        // November 2026 starts on a Sunday: six days of October, then six of December.
        let november = grid(2026, 11);
        assert_eq!(
            (november[0].month, november[0].day, november[0].inside),
            (10, 26, false)
        );
        assert_eq!((november[6].month, november[6].day, november[6].inside), (11, 1, true));
        let last = november[GRID_DAYS - 1];
        assert_eq!((last.month, last.day, last.inside), (12, 6, false));
        // Over a year's end, either way.
        let january = grid(2026, 1);
        assert_eq!((january[0].year, january[0].month, january[0].day), (2025, 12, 29));
        let december = grid(2026, 12);
        assert_eq!(december[GRID_DAYS - 1].year, 2027);
        // Every sheet is six weeks with its whole month, and every date is real.
        for month in 1..=12 {
            let sheet = grid(2024, month);
            assert_eq!(sheet.len(), GRID_DAYS, "{month}");
            let inside = sheet.iter().filter(|d| d.inside).count();
            assert_eq!(inside as u32, days_in_month(2024, month), "{month}");
            for d in sheet {
                assert!(d.day >= 1 && d.day <= days_in_month(d.year, d.month), "{d:?}");
            }
        }
    }

    #[test]
    fn weeks_are_numbered_as_iso_8601() {
        assert_eq!(iso_week(2026, 11, 9), 46);
        assert_eq!(iso_week(2026, 1, 1), 1, "a Thursday");
        // Early January in the last year's last week.
        assert_eq!(iso_week(2021, 1, 3), 53);
        assert_eq!(iso_week(2023, 1, 1), 52);
        // Late December in next year's first week.
        assert_eq!(iso_week(2024, 12, 30), 1);
        assert_eq!(iso_week(2020, 12, 31), 53);
    }

    #[test]
    fn days_are_counted_either_way_and_over_a_leap_day() {
        assert_eq!(days_between((2026, 11, 9), (2026, 11, 11)), 2);
        assert_eq!(days_between((2026, 11, 11), (2026, 11, 9)), -2);
        assert_eq!(days_between((2026, 11, 9), (2026, 11, 9)), 0);
        assert_eq!(days_between((2024, 2, 28), (2024, 3, 1)), 2);
        assert_eq!(days_between((2025, 2, 28), (2025, 3, 1)), 1);
        assert_eq!(days_between((2026, 12, 31), (2027, 1, 1)), 1);
    }

    #[test]
    fn months_shift_over_the_year() {
        assert_eq!(shifted(2026, 1, -1), (2025, 12));
        assert_eq!(shifted(2026, 12, 1), (2027, 1));
        assert_eq!(shifted(2026, 10, 0), (2026, 10));
        assert_eq!(shifted(2026, 10, -22), (2024, 12));
    }
}
