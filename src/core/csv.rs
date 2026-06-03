//! CSV import/export (issue #15). RFC 4180: fields containing commas, quotes,
//! or newlines are quoted; embedded quotes double. Export writes computed
//! values (formulas resolve); import produces a plain value grid.

use crate::core::data_proxy::DataProxy;

/// Serialize the sheet's used extent to CSV. Formula cells export their
/// computed raw value (no display formatting), matching what Excel writes.
pub fn to_csv(sheet: &DataProxy) -> String {
    let Some((max_r, max_c)) = sheet.used_extent() else {
        return String::new();
    };
    let mut out = String::new();
    for r in 0..=max_r {
        for c in 0..=max_c {
            if c > 0 {
                out.push(',');
            }
            out.push_str(&quote_field(&sheet.cell_raw_value(r, c)));
        }
        out.push_str("\r\n");
    }
    out
}

/// Quote a field when it contains a comma, quote, or line break.
fn quote_field(v: &str) -> String {
    if v.contains(',') || v.contains('"') || v.contains('\n') || v.contains('\r') {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

/// Parse CSV text into a sheet named `name`. Handles quoted fields, doubled
/// quotes, embedded commas/newlines, and both LF and CRLF row endings.
pub fn from_csv(name: &str, text: &str) -> DataProxy {
    let mut sheet = DataProxy::new(name);
    let mut field = String::new();
    let mut in_quotes = false;
    let mut r = 0usize;
    let mut c = 0usize;
    let mut chars = text.chars().peekable();

    let commit = |sheet: &mut DataProxy, r: usize, c: usize, field: &mut String| {
        if !field.is_empty() {
            sheet.set_cell_text(r, c, field);
        }
        field.clear();
    };

    while let Some(ch) = chars.next() {
        if in_quotes {
            match ch {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next(); // doubled quote → literal quote
                        field.push('"');
                    } else {
                        in_quotes = false;
                    }
                }
                _ => field.push(ch),
            }
            continue;
        }
        match ch {
            '"' if field.is_empty() => in_quotes = true,
            ',' => {
                commit(&mut sheet, r, c, &mut field);
                c += 1;
            }
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                commit(&mut sheet, r, c, &mut field);
                r += 1;
                c = 0;
            }
            '\n' => {
                commit(&mut sheet, r, c, &mut field);
                r += 1;
                c = 0;
            }
            _ => field.push(ch),
        }
    }
    commit(&mut sheet, r, c, &mut field);
    sheet
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn export_quotes_only_when_needed() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "plain");
        d.set_cell_text(0, 1, "with,comma");
        d.set_cell_text(0, 2, "with \"quote\"");
        d.set_cell_text(1, 0, "line\nbreak");
        assert_eq!(
            to_csv(&d),
            "plain,\"with,comma\",\"with \"\"quote\"\"\"\r\n\"line\nbreak\",,\r\n"
        );
    }

    #[test]
    fn export_resolves_formulas_to_raw_values() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "2");
        d.set_cell_text(0, 1, "=A1*3");
        assert_eq!(to_csv(&d), "2,6\r\n");
    }

    #[test]
    fn import_parses_quotes_commas_and_newlines() {
        let d = from_csv("t", "a,\"b,1\",\"say \"\"hi\"\"\"\r\n\"multi\nline\",x\n");
        assert_eq!(d.get_cell_text(0, 0), "a");
        assert_eq!(d.get_cell_text(0, 1), "b,1");
        assert_eq!(d.get_cell_text(0, 2), "say \"hi\"");
        assert_eq!(d.get_cell_text(1, 0), "multi\nline");
        assert_eq!(d.get_cell_text(1, 1), "x");
    }

    #[test]
    fn csv_roundtrip_preserves_grid() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "Name");
        d.set_cell_text(0, 1, "Score");
        d.set_cell_text(1, 0, "Alice, A.");
        d.set_cell_text(1, 1, "100");
        let back = from_csv("t", &to_csv(&d));
        assert_eq!(back.get_cell_text(0, 0), "Name");
        assert_eq!(back.get_cell_text(1, 0), "Alice, A.");
        assert_eq!(back.get_cell_text(1, 1), "100");
    }

    #[test]
    fn import_handles_lf_and_crlf_and_empty() {
        let d = from_csv("t", "a,b\nc,d\r\ne");
        assert_eq!(d.get_cell_text(1, 0), "c");
        assert_eq!(d.get_cell_text(2, 0), "e");
        assert_eq!(from_csv("t", "").used_extent(), None);
    }
}
