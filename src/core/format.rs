//! Cell value formatting, ported from x-spreadsheet's `core/format.js`.
//!
//! Numeric formats render with two decimals and thousands separators; currency
//! formats prepend a symbol; percent appends `%`. String/date formats pass the
//! value through unchanged (x-spreadsheet does no real date parsing here).

/// Format a raw cell value according to a format key, or a custom format
/// string like `#,##0.00`, `0.0%`, or `$#,##0.00;(#,##0.00)`.
pub fn format_value(text: &str, format: &str) -> String {
    use crate::core::date::DateKind;
    match format {
        "number" => format_number_str(text),
        "percent" => format!("{}%", text),
        "rmb" => with_prefix("￥", text),
        "usd" => with_prefix("$", text),
        "eur" => with_prefix("€", text),
        "date" => format_temporal(text, DateKind::Date),
        "time" => format_temporal(text, DateKind::Time),
        "datetime" => format_temporal(text, DateKind::DateTime),
        "duration" => format_temporal(text, DateKind::Duration),
        "normal" | "text" | "general" => text.to_string(),
        // A pattern with date/time token letters (y/m/d/h/s) is an Excel-style
        // date format code (issue #40); anything else is a custom number
        // pattern. Either way a non-numeric value passes through unchanged.
        pattern => match text.trim().parse::<f64>() {
            Ok(n) if looks_like_date_pattern(pattern) => format_date_pattern(n, pattern),
            Ok(n) => format_custom(n, pattern),
            Err(_) => text.to_string(),
        },
    }
}

const MONTH_ABBR: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const MONTH_FULL: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// True if `pattern` is an Excel date/time format code rather than a number
/// pattern. Quoted/escaped literals are stripped first so a literal like
/// `"kg"` or `"items"` doesn't falsely trigger date mode; what remains is a
/// date code if it contains any token letter (y/m/d/h/s).
fn looks_like_date_pattern(pattern: &str) -> bool {
    let chars: Vec<char> = pattern.chars().collect();
    let mut i = 0;
    let mut bare = String::new();
    while i < chars.len() {
        match chars[i] {
            '"' => {
                i += 1;
                while i < chars.len() && chars[i] != '"' {
                    i += 1;
                }
                i += 1; // closing quote
            }
            '\\' => i += 2, // escaped literal char
            c => {
                bare.push(c.to_ascii_lowercase());
                i += 1;
            }
        }
    }
    bare.chars()
        .any(|c| matches!(c, 'y' | 'm' | 'd' | 'h' | 's'))
}

enum DateSeg {
    /// A run of `count` of the same token letter (lower-cased).
    Tok(char, usize),
    /// AM/PM (or A/P) indicator.
    AmPm,
    /// A literal passed through verbatim.
    Lit(String),
}

fn parse_date_segments(pattern: &str) -> Vec<DateSeg> {
    let chars: Vec<char> = pattern.chars().collect();
    let lower: Vec<char> = chars.iter().map(|c| c.to_ascii_lowercase()).collect();
    let starts_with = |i: usize, s: &str| lower[i..].iter().collect::<String>().starts_with(s);
    let mut segs = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '"' {
            i += 1;
            let mut s = String::new();
            while i < chars.len() && chars[i] != '"' {
                s.push(chars[i]);
                i += 1;
            }
            i += 1; // closing quote
            segs.push(DateSeg::Lit(s));
        } else if c == '\\' && i + 1 < chars.len() {
            segs.push(DateSeg::Lit(chars[i + 1].to_string()));
            i += 2;
        } else if starts_with(i, "am/pm") {
            segs.push(DateSeg::AmPm);
            i += 5;
        } else if starts_with(i, "a/p") {
            segs.push(DateSeg::AmPm);
            i += 3;
        } else {
            let lc = c.to_ascii_lowercase();
            if matches!(lc, 'y' | 'm' | 'd' | 'h' | 's') {
                let mut n = 0;
                while i < chars.len() && chars[i].to_ascii_lowercase() == lc {
                    n += 1;
                    i += 1;
                }
                segs.push(DateSeg::Tok(lc, n));
            } else {
                segs.push(DateSeg::Lit(c.to_string()));
                i += 1;
            }
        }
    }
    segs
}

/// Render a date serial with an Excel date/time format code. Supports
/// `yyyy/yy`, `mmmm/mmm/mm/m` (month, or minutes next to `h`/`s`), `dd/d`,
/// `hh/h`, `ss/s`, and `AM/PM`. Unknown characters pass through as literals.
pub fn format_date_pattern(serial: f64, pattern: &str) -> String {
    use crate::core::date::{from_serial, time_parts};
    let (year, month, day) = from_serial(serial);
    let (hour24, minute, second) = time_parts(serial);
    let segs = parse_date_segments(pattern);
    let ampm = segs.iter().any(|s| matches!(s, DateSeg::AmPm));
    let hour = if ampm {
        let h = hour24 % 12;
        if h == 0 {
            12
        } else {
            h
        }
    } else {
        hour24
    };
    // Token letters in order, so an `m` knows its neighbors for the
    // month-vs-minute rule (minute when adjacent to hours or seconds).
    let letters: Vec<char> = segs
        .iter()
        .filter_map(|s| {
            if let DateSeg::Tok(c, _) = s {
                Some(*c)
            } else {
                None
            }
        })
        .collect();
    let mut ti: usize = 0;
    let mut out = String::new();
    for seg in &segs {
        match seg {
            DateSeg::Lit(s) => out.push_str(s),
            DateSeg::AmPm => out.push_str(if hour24 < 12 { "AM" } else { "PM" }),
            DateSeg::Tok(c, n) => {
                let n = *n;
                match c {
                    'y' => out.push_str(&if n <= 2 {
                        format!("{:02}", year.rem_euclid(100))
                    } else {
                        format!("{:04}", year)
                    }),
                    'd' => out.push_str(&if n >= 2 {
                        format!("{day:02}")
                    } else {
                        format!("{day}")
                    }),
                    'h' => out.push_str(&if n >= 2 {
                        format!("{hour:02}")
                    } else {
                        format!("{hour}")
                    }),
                    's' => out.push_str(&if n >= 2 {
                        format!("{second:02}")
                    } else {
                        format!("{second}")
                    }),
                    'm' => {
                        let prev = ti.checked_sub(1).and_then(|p| letters.get(p)).copied();
                        let next = letters.get(ti + 1).copied();
                        let is_minute = prev == Some('h') || next == Some('s');
                        if is_minute {
                            out.push_str(&if n >= 2 {
                                format!("{minute:02}")
                            } else {
                                format!("{minute}")
                            });
                        } else {
                            let idx = (month as usize).saturating_sub(1).min(11);
                            match n {
                                1 => out.push_str(&format!("{month}")),
                                2 => out.push_str(&format!("{month:02}")),
                                3 => out.push_str(MONTH_ABBR[idx]),
                                _ => out.push_str(MONTH_FULL[idx]),
                            }
                        }
                    }
                    _ => {}
                }
                ti += 1;
            }
        }
    }
    out
}

/// Render a date/time-formatted cell. The cell value is interpreted as a serial
/// number (e.g. a formula result like `=TODAY()`) or as a date string; either
/// way it is normalized to the format's canonical rendering. Anything that is
/// not a recognizable date or number passes through unchanged.
fn format_temporal(text: &str, kind: crate::core::date::DateKind) -> String {
    let t = text.trim();
    if t.is_empty() {
        return text.to_string();
    }
    let serial = match t.parse::<f64>() {
        Ok(n) => n,
        Err(_) => match crate::core::date::parse_date(t) {
            Some(s) => s,
            None => return text.to_string(),
        },
    };
    crate::core::date::format_serial(serial, kind)
}

/// Render a number with a custom format pattern. Supports digit placeholders
/// (`0`, `#`), decimal point, thousands grouping (`,`), percent (`%`), literal
/// prefix/suffix text, and `;`-separated positive/negative/zero sections.
pub fn format_custom(value: f64, pattern: &str) -> String {
    let sections: Vec<&str> = pattern.split(';').collect();
    // Pick the section by sign; a single section also formats negatives (with a
    // leading minus we add ourselves).
    let (section, add_minus) = if value < 0.0 {
        if sections.len() >= 2 {
            (sections[1], false)
        } else {
            (sections[0], true)
        }
    } else if value == 0.0 && sections.len() >= 3 {
        (sections[2], false)
    } else {
        (sections.first().copied().unwrap_or(""), false)
    };

    let is_ph = |c: char| matches!(c, '0' | '#' | ',' | '.');
    let chars: Vec<char> = section.chars().collect();
    let (start, end) = match (
        chars.iter().position(|&c| is_ph(c)),
        chars.iter().rposition(|&c| is_ph(c)),
    ) {
        (Some(s), Some(e)) => (s, e),
        // No numeric placeholders: a literal-only section.
        _ => return section.to_string(),
    };

    let prefix: String = chars[..start].iter().collect();
    let numpat: String = chars[start..=end].iter().collect();
    let suffix: String = chars[end + 1..].iter().collect();

    let percent = section.contains('%');
    let scaled = if percent {
        value.abs() * 100.0
    } else {
        value.abs()
    };

    let (int_pat, frac_pat) = match numpat.split_once('.') {
        Some((i, f)) => (i.to_string(), Some(f.to_string())),
        None => (numpat.clone(), None),
    };
    let decimals = frac_pat
        .as_ref()
        .map(|f| f.chars().filter(|&c| c == '0' || c == '#').count())
        .unwrap_or(0);
    let grouping = int_pat.contains(',');
    let min_int = int_pat.chars().filter(|&c| c == '0').count();

    // Round half away from zero (Excel-style), then format to the digit count.
    let factor = 10f64.powi(decimals as i32);
    let rounded_val = (scaled * factor).round() / factor;
    let rounded = format!("{:.*}", decimals, rounded_val);
    let (int_part, frac_part) = match rounded.split_once('.') {
        Some((i, f)) => (i.to_string(), f.to_string()),
        None => (rounded.clone(), String::new()),
    };

    let mut int_str: String = int_part.trim_start_matches('0').to_string();
    while int_str.len() < min_int {
        int_str.insert(0, '0');
    }
    if int_str.is_empty() {
        int_str.push('0');
    }
    if grouping {
        int_str = group_thousands(&int_str);
    }

    let mut out = String::new();
    out.push_str(&prefix);
    if add_minus {
        out.push('-');
    }
    out.push_str(&int_str);
    if decimals > 0 {
        out.push('.');
        out.push_str(&frac_part);
    }
    out.push_str(&suffix);
    out
}

fn with_prefix(prefix: &str, text: &str) -> String {
    format!("{}{}", prefix, format_number_str(text))
}

/// Render a numeric string as `1,234.50`; non-numeric input is returned as-is.
fn format_number_str(v: &str) -> String {
    match v.trim().parse::<f64>() {
        Ok(n) => add_thousands(&format!("{:.2}", n)),
        Err(_) => v.to_string(),
    }
}

/// Insert thousands separators into a fixed-point numeric string like `-1234.50`.
fn add_thousands(s: &str) -> String {
    let neg = s.starts_with('-');
    let body = s.trim_start_matches('-');
    let (int_part, frac) = match body.split_once('.') {
        Some((i, f)) => (i, Some(f)),
        None => (body, None),
    };

    let mut out = String::new();
    if neg {
        out.push('-');
    }
    out.push_str(&group_thousands(int_part));
    if let Some(f) = frac {
        out.push('.');
        out.push_str(f);
    }
    out
}

fn group_thousands(int_part: &str) -> String {
    let chars: Vec<char> = int_part.chars().collect();
    let len = chars.len();
    let mut out = String::with_capacity(len + len / 3);
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (len - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn number_format() {
        assert_eq!(format_value("1234.5", "number"), "1,234.50");
        assert_eq!(format_value("-1234567", "number"), "-1,234,567.00");
        assert_eq!(format_value("12", "number"), "12.00");
    }

    #[test]
    fn currency_and_percent() {
        assert_eq!(format_value("1000", "usd"), "$1,000.00");
        assert_eq!(format_value("1000", "rmb"), "￥1,000.00");
        assert_eq!(format_value("1000", "eur"), "€1,000.00");
        assert_eq!(format_value("10.12", "percent"), "10.12%");
    }

    #[test]
    fn passthrough_and_non_numeric() {
        assert_eq!(format_value("hello", "normal"), "hello");
        assert_eq!(format_value("hello", "number"), "hello");
        assert_eq!(format_value("2008-09-26", "date"), "2008-09-26");
    }

    #[test]
    fn date_and_time_rendering() {
        // A serial number (e.g. a formula result) renders as a date.
        assert_eq!(format_value("45306", "date"), "2024-01-15");
        // Date strings are normalized to ISO.
        assert_eq!(format_value("2024/01/15", "date"), "2024-01-15");
        assert_eq!(format_value("1/15/2024", "date"), "2024-01-15");
        // Time and datetime from fractional serials.
        assert_eq!(format_value("0.5", "time"), "12:00:00");
        assert_eq!(format_value("45306.5", "datetime"), "2024-01-15 12:00:00");
        assert_eq!(format_value("1.5", "duration"), "36:00:00");
        // Non-date text falls through unchanged.
        assert_eq!(format_value("hello", "date"), "hello");
        assert_eq!(format_value("", "date"), "");
    }

    #[test]
    fn custom_patterns() {
        assert_eq!(format_value("1234.5", "#,##0.00"), "1,234.50");
        assert_eq!(format_value("1234.5", "0"), "1235"); // rounds, no decimals
        assert_eq!(format_value("0.1234", "0.0%"), "12.3%");
        assert_eq!(format_value("1234.5", "$#,##0.00"), "$1,234.50");
        assert_eq!(format_value("5", "00000"), "00005"); // zero-padding
        assert_eq!(format_value("-5", "0.00"), "-5.00"); // single section, minus
        assert_eq!(format_value("1234.567", "#,##0.0 \"kg\""), "1,234.6 \"kg\"");
    }

    #[test]
    fn custom_sections() {
        // positive;negative;zero
        let p = "#,##0;(#,##0);-";
        assert_eq!(format_value("1234", p), "1,234");
        assert_eq!(format_value("-1234", p), "(1,234)");
        assert_eq!(format_value("0", p), "-");
    }

    #[test]
    fn custom_non_numeric_passthrough() {
        assert_eq!(format_value("abc", "#,##0.00"), "abc");
    }

    // Excel-style date/time format codes via TEXT() (issue #40).
    #[test]
    fn date_format_codes() {
        use crate::core::date::to_serial;
        let s = to_serial(2024, 12, 25).to_string();
        assert_eq!(format_value(&s, "yyyy-mm-dd"), "2024-12-25");
        assert_eq!(format_value(&s, "mm/dd/yyyy"), "12/25/2024");
        assert_eq!(format_value(&s, "yyyy/m/d"), "2024/12/25");
        assert_eq!(format_value(&s, "yy-mm-dd"), "24-12-25");
        assert_eq!(format_value(&s, "mmm d, yyyy"), "Dec 25, 2024");
        assert_eq!(format_value(&s, "mmmm"), "December");
        // `mm` between `hh` and `ss` is minutes, not month.
        assert_eq!(format_value("0.5", "hh:mm:ss"), "12:00:00");
        let dt = (to_serial(2024, 1, 15) + 0.5).to_string();
        assert_eq!(
            format_value(&dt, "yyyy-mm-dd hh:mm:ss"),
            "2024-01-15 12:00:00"
        );
        // Non-numeric passes through unchanged.
        assert_eq!(format_value("hello", "yyyy-mm-dd"), "hello");
        // A literal containing date letters (quoted) must NOT trigger date mode.
        assert_eq!(format_value("1234.5", "#,##0.0 \"kg\""), "1,234.5 \"kg\"");
    }
}
