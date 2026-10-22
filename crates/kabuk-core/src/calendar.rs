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

/// The day of the week of a date, 0 for Monday to 6 for Sunday.
pub fn weekday(year: i32, month: u32, day: u32) -> u32 {
    // Days from 1970-01-01 (a Thursday), by Howard Hinnant's civil-from-days inverse.
    let y = i64::from(year) - i64::from(month <= 2);
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    // 1970-01-01 is a Thursday: 3 days after Monday.
    u32::try_from((days + 3).rem_euclid(7)).unwrap_or(0)
}

/// The month's cells in weeks from Monday: empty cells before its first day, then its
/// days.
pub fn cells(year: i32, month: u32) -> Vec<Option<u32>> {
    let lead = weekday(year, month, 1) as usize;
    std::iter::repeat_n(None, lead)
        .chain((1..=days_in_month(year, month)).map(Some))
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
        // June 2026 starts on a Monday, March 2026 on a Sunday.
        assert_eq!(cells(2026, 6)[0], Some(1));
        let march = cells(2026, 3);
        assert_eq!(march.iter().take_while(|c| c.is_none()).count(), 6);
        assert_eq!(march.last(), Some(&Some(31)));
        assert_eq!(cells(2026, 10).iter().filter(|c| c.is_some()).count(), 31);
    }

    #[test]
    fn months_shift_over_the_year() {
        assert_eq!(shifted(2026, 1, -1), (2025, 12));
        assert_eq!(shifted(2026, 12, 1), (2027, 1));
        assert_eq!(shifted(2026, 10, 0), (2026, 10));
        assert_eq!(shifted(2026, 10, -22), (2024, 12));
    }
}
