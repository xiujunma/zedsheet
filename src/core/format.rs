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
        // Anything else is treated as a custom number-format pattern.
        pattern => match text.trim().parse::<f64>() {
            Ok(n) => format_custom(n, pattern),
            Err(_) => text.to_string(),
        },
    }
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
    let scaled = if percent { value.abs() * 100.0 } else { value.abs() };

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
}
