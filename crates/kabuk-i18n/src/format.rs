//! Date, time and number formats. Names and separators come from the translation
//! files, so a new language only needs a new `.ftl` file.
//!
//! Edge cases tested: every weekday and month in both languages, midnight and noon in
//! 12-hour form, single-digit hours and minutes, rounding (including to the next
//! integer and negative values), zero digits, out-of-range calendar inputs.

use crate::{Arg, I18n};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClockStyle {
    H24,
    H12,
}

/// The clock split into its parts, so the bar can style each one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TimeText {
    pub hours: String,
    pub minutes: String,
    /// `ÖÖ`/`ÖS` or `AM`/`PM` in 12-hour form.
    pub suffix: Option<String>,
}

impl I18n {
    /// Short date as on the bar, e.g. `Pzt 12 Eki` / `Mon 12 Oct`.
    /// `weekday`: 0 = Sunday … 6; `month`: 1 … 12. Out-of-range values are clamped.
    pub fn date_short(&self, weekday: u8, day: u8, month: u8) -> String {
        let weekday = self.tr(&format!("days-{}", weekday.min(6)));
        let month = self.tr(&format!("months-{}", month.clamp(1, 12) - 1));
        self.tr_args(
            "date-short",
            &[
                ("weekday", Arg::Str(weekday)),
                ("day", Arg::Str(day.to_string())),
                ("month", Arg::Str(month)),
            ],
        )
    }

    /// Long date as on the screen saver's clock, e.g. `12 Ekim Pazartesi` / `Monday, 12
    /// October`. `weekday`: 0 = Sunday … 6; `month`: 1 … 12. Out-of-range values are clamped.
    pub fn date_long(&self, weekday: u8, day: u8, month: u8) -> String {
        let weekday = self.tr(&format!("day-long-{}", weekday.min(6)));
        let month = self.tr(&format!("month-long-{}", month.clamp(1, 12) - 1));
        self.tr_args(
            "date-long",
            &[
                ("weekday", Arg::Str(weekday)),
                ("day", Arg::Str(day.to_string())),
                ("month", Arg::Str(month)),
            ],
        )
    }

    /// Clock text. 24-hour form pads the hour (`08`); 12-hour form does not (`8`).
    pub fn time(&self, hour: u8, minute: u8, style: ClockStyle) -> TimeText {
        let hour = hour % 24;
        let minutes = format!("{:02}", minute % 60);
        match style {
            ClockStyle::H24 => TimeText {
                hours: format!("{hour:02}"),
                minutes,
                suffix: None,
            },
            ClockStyle::H12 => {
                let shown = match hour % 12 {
                    0 => 12,
                    h => h,
                };
                let suffix = if hour < 12 { self.tr("am") } else { self.tr("pm") };
                TimeText {
                    hours: shown.to_string(),
                    minutes,
                    suffix: Some(suffix),
                }
            }
        }
    }

    /// A number with `digits` decimals and the language's decimal separator (`9,8` / `9.8`).
    pub fn decimal(&self, value: f64, digits: usize) -> String {
        let text = format!("{value:.digits$}");
        // A value that rounds to zero is shown without a sign.
        let text = if text.starts_with('-') && text[1..].chars().all(|c| c == '0' || c == '.') {
            text[1..].to_string()
        } else {
            text
        };
        text.replace('.', &self.tr("decimal-separator"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Lang;

    #[test]
    fn short_dates() {
        let tr = I18n::new(Lang::Tr);
        let en = I18n::new(Lang::En);
        assert_eq!(tr.date_short(1, 12, 10), "Pzt 12 Eki");
        assert_eq!(en.date_short(1, 12, 10), "Mon 12 Oct");
        let tr_days: Vec<String> = (0..7).map(|d| tr.date_short(d, 1, 1)).collect();
        assert_eq!(tr_days[0], "Paz 1 Oca");
        assert_eq!(tr_days[3], "Çar 1 Oca");
        assert_eq!(tr_days[6], "Cmt 1 Oca");
        let en_months: Vec<String> = (1..=12).map(|m| en.date_short(0, 31, m)).collect();
        assert_eq!(en_months[0], "Sun 31 Jan");
        assert_eq!(en_months[11], "Sun 31 Dec");
        assert_eq!(tr.date_short(0, 5, 8), "Paz 5 Ağu");
        assert_eq!(tr.date_short(0, 5, 2), "Paz 5 Şub");
        // Clamped instead of showing a key name.
        assert_eq!(en.date_short(9, 1, 0), "Sat 1 Jan");
        assert_eq!(en.date_short(0, 1, 13), "Sun 1 Dec");
    }

    #[test]
    fn long_dates() {
        let tr = I18n::new(Lang::Tr);
        let en = I18n::new(Lang::En);
        assert_eq!(tr.date_long(1, 12, 10), "12 Ekim Pazartesi");
        assert_eq!(en.date_long(1, 12, 10), "Monday, 12 October");
        assert_eq!(tr.date_long(4, 1, 8), "1 Ağustos Perşembe");
        assert_eq!(tr.date_long(9, 1, 0), "1 Ocak Cumartesi", "clamped");
        assert_eq!(en.date_long(0, 31, 13), "Sunday, 31 December");
    }

    #[test]
    fn clock_24_hour() {
        let tr = I18n::new(Lang::Tr);
        let t = tr.time(8, 5, ClockStyle::H24);
        assert_eq!((t.hours.as_str(), t.minutes.as_str(), t.suffix), ("08", "05", None));
        let t = tr.time(0, 0, ClockStyle::H24);
        assert_eq!((t.hours.as_str(), t.minutes.as_str()), ("00", "00"));
        let t = tr.time(23, 59, ClockStyle::H24);
        assert_eq!((t.hours.as_str(), t.minutes.as_str()), ("23", "59"));
    }

    #[test]
    fn clock_12_hour() {
        let tr = I18n::new(Lang::Tr);
        let en = I18n::new(Lang::En);
        let t = tr.time(0, 0, ClockStyle::H12);
        assert_eq!((t.hours.as_str(), t.suffix.as_deref()), ("12", Some("ÖÖ")));
        let t = tr.time(12, 30, ClockStyle::H12);
        assert_eq!((t.hours.as_str(), t.suffix.as_deref()), ("12", Some("ÖS")));
        let t = en.time(20, 31, ClockStyle::H12);
        assert_eq!(
            (t.hours.as_str(), t.minutes.as_str(), t.suffix.as_deref()),
            ("8", "31", Some("PM"))
        );
        let t = en.time(11, 59, ClockStyle::H12);
        assert_eq!((t.hours.as_str(), t.suffix.as_deref()), ("11", Some("AM")));
    }

    #[test]
    fn decimals() {
        let tr = I18n::new(Lang::Tr);
        let en = I18n::new(Lang::En);
        assert_eq!(tr.decimal(9.8, 1), "9,8");
        assert_eq!(en.decimal(9.8, 1), "9.8");
        assert_eq!(tr.decimal(9.96, 1), "10,0");
        assert_eq!(en.decimal(2.0, 0), "2");
        assert_eq!(tr.decimal(-1.25, 2), "-1,25");
        assert_eq!(tr.decimal(-0.01, 1), "0,0");
        assert_eq!(en.decimal(0.0, 1), "0.0");
    }

    #[test]
    fn bar_value_formats() {
        let tr = I18n::new(Lang::Tr);
        let ram = tr.decimal(9.8, 1);
        assert_eq!(tr.tr_args("bar-ram", &[("value", ram.into())]), "9,8G");
        assert_eq!(tr.tr_args("bar-percent", &[("value", "12".into())]), "12%");
        assert_eq!(tr.tr_args("bar-temp", &[("value", "46".into())]), "46°");
        let en = I18n::new(Lang::En);
        let net = en.decimal(2.1, 1);
        assert_eq!(en.tr_args("bar-net-rate", &[("value", net.into())]), "2.1 MB/s");
        assert_eq!(tr.tr("layout-float"), "yüzen");
    }
}
