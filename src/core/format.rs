//! Cell value formatting, ported from x-spreadsheet's `core/format.js`.
//!
//! Numeric formats render with two decimals and thousands separators; currency
//! formats prepend a symbol; percent appends `%`. String/date formats pass the
//! value through unchanged (x-spreadsheet does no real date parsing here).

/// Format a raw cell value according to a format key.
pub fn format_value(text: &str, format: &str) -> String {
    match format {
        "number" => format_number_str(text),
        "percent" => format!("{}%", text),
        "rmb" => with_prefix("￥", text),
        "usd" => with_prefix("$", text),
        "eur" => with_prefix("€", text),
        // normal | text | date | time | datetime | duration | unknown
        _ => text.to_string(),
    }
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
}
