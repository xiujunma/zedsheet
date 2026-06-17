//! Date/time serial-number conversion, parsing, and rendering.
//!
//! Dates are stored as **serial numbers**: the integer part counts days since
//! the 1899-12-30 epoch (serial 0) and the fractional part is the time of day
//! (`0.5` == 12:00:00). This matches Excel and Google Sheets for every date on
//! or after 1900-03-01; earlier dates differ by one day because Excel keeps the
//! historical "1900 is a leap year" bug, which we intentionally do not emulate.
//!
//! This module is pure (no `js-sys`/`web-sys`) so it is fully unit-testable with
//! plain `cargo test`. The current-clock functions (`TODAY`/`NOW`) live in the
//! evaluator, which feeds the resulting serial through `to_serial`/`from_serial`.

/// How a serial number should be rendered as text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateKind {
    Date,
    Time,
    DateTime,
    Duration,
}

/// Excel/Sheets serial for the Unix epoch 1970-01-01 under the 1899-12-30 base.
const UNIX_EPOCH_SERIAL: i64 = 25569;

/// Days from the civil epoch 1970-01-01 for `y-m-d` (proleptic Gregorian).
/// Howard Hinnant's `days_from_civil`; assumes `1 <= m <= 12` and a valid day.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146097 + doe - 719468
}

/// Inverse of [`days_from_civil`]: days-since-1970 back to `(year, month, day)`.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32; // [1, 12]
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Serial number for a calendar date. Out-of-range months and days roll over
/// like Excel's `DATE` (e.g. `to_serial(2024, 13, 1)` == 2025-01-01).
pub fn to_serial(y: i64, m: i64, d: i64) -> f64 {
    // Normalize the month into 1..=12, carrying whole years.
    let year = y + (m - 1).div_euclid(12);
    let month = (m - 1).rem_euclid(12) + 1;
    // Anchor on the first of the month, then add (d - 1) so day overflow rolls.
    let base = days_from_civil(year, month, 1) + UNIX_EPOCH_SERIAL;
    (base + (d - 1)) as f64
}

/// `(year, month, day)` for the integer (date) part of a serial.
pub fn from_serial(serial: f64) -> (i64, u32, u32) {
    civil_from_days(serial.floor() as i64 - UNIX_EPOCH_SERIAL)
}

/// `(hour, minute, second)` for the fractional (time) part of a serial.
pub fn time_parts(serial: f64) -> (u32, u32, u32) {
    let frac = serial - serial.floor();
    let total = ((frac * 86_400.0).round() as i64).rem_euclid(86_400);
    (
        (total / 3600) as u32,
        ((total % 3600) / 60) as u32,
        (total % 60) as u32,
    )
}

/// Parse a date / time / datetime string into a serial number, or `None` if it
/// is not a recognizable date. Supports `YYYY-MM-DD`, `YYYY/MM/DD`, `M/D/YYYY`,
/// an optional ` HH:MM[:SS]` suffix, and a bare `HH:MM[:SS]` (time-only).
pub fn parse_date(s: &str) -> Option<f64> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    let mut parts = s.split_whitespace();
    let first = parts.next()?;
    let second = parts.next();
    if parts.next().is_some() {
        return None; // more than two whitespace-separated tokens
    }

    // Time-only: `HH:MM[:SS]` with no date separators and no second token.
    if second.is_none() && first.contains(':') && !first.contains('-') && !first.contains('/') {
        return parse_time(first);
    }

    let (y, m, d) = parse_ymd(first)?;
    let serial = to_serial(y, m as i64, d as i64);
    match second {
        Some(t) => Some(serial + parse_time(t)?),
        None => Some(serial),
    }
}

/// Render a serial number as text for the given [`DateKind`].
pub fn format_serial(serial: f64, kind: DateKind) -> String {
    match kind {
        DateKind::Date => {
            let (y, m, d) = from_serial(serial);
            format!("{y:04}-{m:02}-{d:02}")
        }
        DateKind::Time => {
            let (h, mi, s) = time_parts(serial);
            format!("{h:02}:{mi:02}:{s:02}")
        }
        DateKind::DateTime => {
            // Round to the nearest second across the whole serial so a time that
            // rounds up to 24:00:00 carries into the next day.
            let total = (serial * 86_400.0).round() as i64;
            let (y, m, d) = civil_from_days(total.div_euclid(86_400) - UNIX_EPOCH_SERIAL);
            let sod = total.rem_euclid(86_400);
            format!(
                "{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}",
                sod / 3600,
                (sod % 3600) / 60,
                sod % 60
            )
        }
        DateKind::Duration => {
            // Elapsed time; hours may exceed 24. Sign-aware.
            let neg = serial < 0.0;
            let total = (serial.abs() * 86_400.0).round() as i64;
            format!(
                "{}{}:{:02}:{:02}",
                if neg { "-" } else { "" },
                total / 3600,
                (total % 3600) / 60,
                total % 60
            )
        }
    }
}

/// Parse a `YYYY-MM-DD` / `YYYY/MM/DD` / `M/D/YYYY` date into `(y, m, d)`.
/// A 4-digit first field means ISO (year first); otherwise US month/day/year.
fn parse_ymd(s: &str) -> Option<(i64, u32, u32)> {
    let sep = if s.contains('-') {
        '-'
    } else if s.contains('/') {
        '/'
    } else {
        return None;
    };
    let nums: Vec<&str> = s.split(sep).collect();
    if nums.len() != 3 {
        return None;
    }
    let p0: i64 = nums[0].parse().ok()?;
    let p1: i64 = nums[1].parse().ok()?;
    let p2: i64 = nums[2].parse().ok()?;
    let (y, m, d) = if nums[0].len() == 4 {
        (p0, p1, p2)
    } else {
        (p2, p0, p1)
    };
    let m = u32::try_from(m).ok()?;
    let d = u32::try_from(d).ok()?;
    if m >= 1 && m <= 12 && d >= 1 && d <= days_in_month(y, m) {
        Some((y, m, d))
    } else {
        None
    }
}

/// Parse `HH:MM[:SS]` into a day fraction in `[0, 1)`.
fn parse_time(s: &str) -> Option<f64> {
    let nums: Vec<&str> = s.split(':').collect();
    if nums.len() < 2 || nums.len() > 3 {
        return None;
    }
    let h: i64 = nums[0].parse().ok()?;
    let mi: i64 = nums[1].parse().ok()?;
    let se: i64 = if nums.len() == 3 {
        nums[2].parse().ok()?
    } else {
        0
    };
    if !(0..=23).contains(&h) || !(0..=59).contains(&mi) || !(0..=59).contains(&se) {
        return None;
    }
    Some((h * 3600 + mi * 60 + se) as f64 / 86_400.0)
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(y) => 29,
        2 => 28,
        _ => 0,
    }
}

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serial_round_trip() {
        assert_eq!(to_serial(2024, 1, 15), 45306.0);
        assert_eq!(to_serial(1970, 1, 1), 25569.0);
        assert_eq!(from_serial(45306.0), (2024, 1, 15));
        assert_eq!(from_serial(25569.0), (1970, 1, 1));
        for s in [1.0, 1000.0, 25569.0, 45306.0, 50000.0] {
            let (y, m, d) = from_serial(s);
            assert_eq!(to_serial(y, m as i64, d as i64), s, "round-trip serial {s}");
        }
    }

    #[test]
    fn date_normalization() {
        assert_eq!(from_serial(to_serial(2024, 13, 1)), (2025, 1, 1)); // month overflow
        assert_eq!(from_serial(to_serial(2024, 0, 1)), (2023, 12, 1)); // month underflow
        assert_eq!(from_serial(to_serial(2024, 1, 32)), (2024, 2, 1)); // day overflow
    }

    #[test]
    fn leap_years() {
        assert!(parse_date("2024-02-29").is_some()); // leap
        assert!(parse_date("2023-02-29").is_none()); // common
        assert!(parse_date("2000-02-29").is_some()); // div 400
        assert!(parse_date("1900-02-29").is_none()); // div 100, not 400
    }

    #[test]
    fn parse_formats() {
        let s = to_serial(2024, 1, 15);
        assert_eq!(parse_date("2024-01-15"), Some(s));
        assert_eq!(parse_date("2024/01/15"), Some(s));
        assert_eq!(parse_date("1/15/2024"), Some(s));
        assert_eq!(parse_date("2024-1-5"), Some(to_serial(2024, 1, 5)));
        assert_eq!(parse_date("not a date"), None);
        assert_eq!(parse_date("2024-13-01"), None);
        assert_eq!(parse_date(""), None);
    }

    #[test]
    fn parse_time_and_datetime() {
        assert_eq!(parse_date("12:00"), Some(0.5));
        assert_eq!(parse_date("06:00:00"), Some(0.25));
        assert_eq!(
            parse_date("2024-01-15 12:00:00"),
            Some(to_serial(2024, 1, 15) + 0.5)
        );
        assert_eq!(parse_date("25:00"), None); // hour out of range
    }

    #[test]
    fn render_kinds() {
        let s = to_serial(2024, 1, 15);
        assert_eq!(format_serial(s, DateKind::Date), "2024-01-15");
        assert_eq!(format_serial(s + 0.5, DateKind::Time), "12:00:00");
        assert_eq!(
            format_serial(s + 0.5, DateKind::DateTime),
            "2024-01-15 12:00:00"
        );
        assert_eq!(format_serial(1.5, DateKind::Duration), "36:00:00");
    }

    #[test]
    fn datetime_second_carry() {
        // 23:59:59.7 rounds up into the next day at 00:00:00.
        let s = to_serial(2024, 1, 15) + 86_399.7 / 86_400.0;
        assert_eq!(format_serial(s, DateKind::DateTime), "2024-01-16 00:00:00");
    }
}
