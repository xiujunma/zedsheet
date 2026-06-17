//! Parse clipboard content back into a [`ParsedGrid`]. The TSV path and the
//! table-positioning path (which honors rowspan/colspan) are pure and
//! host-tested; the actual DOM walk of pasted HTML lives in the browser glue
//! (`zedsheet::system_clipboard`) and feeds [`grid_from_rows`].

use std::collections::{HashMap, HashSet};

use super::model::{ParsedCell, ParsedGrid};

/// One `<td>`/`<th>` extracted from a pasted HTML table, before positioning.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawCell {
    pub text: String,
    pub row_span: usize,
    pub col_span: usize,
}

impl RawCell {
    pub fn new(text: impl Into<String>, row_span: usize, col_span: usize) -> Self {
        RawCell {
            text: text.into(),
            row_span,
            col_span,
        }
    }
}

/// Parse tab-separated text (the `text/plain` clipboard flavor) into a grid.
/// Handles RFC-4180-style quoting (fields wrapped in quotes may contain tabs,
/// newlines, and doubled quotes) and both LF and CRLF row endings.
pub fn parse_tsv(text: &str) -> ParsedGrid {
    let cells = parse_delimited(text, '\t')
        .into_iter()
        .map(|row| row.into_iter().map(ParsedCell::text).collect())
        .collect();
    ParsedGrid { cells }
}

/// Split delimited text into rows of fields, honoring quoted fields.
fn parse_delimited(text: &str, delim: char) -> Vec<Vec<String>> {
    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut in_quotes = false;
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.next() {
        if in_quotes {
            if ch == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    field.push('"'); // doubled quote → literal quote
                } else {
                    in_quotes = false;
                }
            } else {
                field.push(ch);
            }
        } else if ch == '"' && field.is_empty() {
            in_quotes = true;
        } else if ch == delim {
            row.push(std::mem::take(&mut field));
        } else if ch == '\n' {
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else if ch == '\r' {
            // End the row once for CR, CRLF, or a lone CR (old-Mac / LibreOffice
            // line endings) — consume a paired LF so CRLF isn't a double break.
            if chars.peek() == Some(&'\n') {
                chars.next();
            }
            row.push(std::mem::take(&mut field));
            rows.push(std::mem::take(&mut row));
        } else {
            field.push(ch);
        }
    }
    // Flush any trailing field/row that wasn't newline-terminated.
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

/// Position raw table cells into a dense row-major grid, honoring
/// `row_span`/`col_span`: the anchor keeps its span and every position it
/// covers becomes an empty `1×1` placeholder. Row spans are clamped to the
/// number of rows actually present.
pub fn grid_from_rows(rows: &[Vec<RawCell>]) -> ParsedGrid {
    let height = rows.len();
    let mut anchors: HashMap<(usize, usize), ParsedCell> = HashMap::new();
    let mut occupied: HashSet<(usize, usize)> = HashSet::new();
    let mut width = 0usize;

    for (r, raw_row) in rows.iter().enumerate() {
        let mut c = 0usize;
        for raw in raw_row {
            while occupied.contains(&(r, c)) || anchors.contains_key(&(r, c)) {
                c += 1;
            }
            let rs = raw.row_span.max(1).min(height - r);
            let cs = raw.col_span.max(1);
            anchors.insert(
                (r, c),
                ParsedCell {
                    text: raw.text.clone(),
                    row_span: rs,
                    col_span: cs,
                },
            );
            for rr in r..r + rs {
                for cc in c..c + cs {
                    if (rr, cc) != (r, c) {
                        occupied.insert((rr, cc));
                    }
                    width = width.max(cc + 1);
                }
            }
            c += cs;
        }
        width = width.max(c);
    }

    let cells = (0..height)
        .map(|r| {
            (0..width)
                .map(|c| anchors.remove(&(r, c)).unwrap_or_else(ParsedCell::empty))
                .collect()
        })
        .collect();
    ParsedGrid { cells }
}

/// Extract the `data-zedsheet-nonce` attribute value from clipboard HTML, if
/// present. Used to recognize our own clipboard payload for a lossless in-app
/// paste.
pub fn nonce_in_html(html: &str) -> Option<u64> {
    // Scope the search to the first `<table …>` opening tag so the nonce can't
    // be spoofed by cell text, and so it still resolves when the clipboard
    // wraps our fragment (`<html><body><table>…`).
    let table_start = html.find("<table")?;
    let tag = &html[table_start..];
    let tag = &tag[..tag.find('>')?];
    let key = "data-zedsheet-nonce=\"";
    let start = tag.find(key)? + key.len();
    let rest = &tag[start..];
    let end = rest.find('"')?;
    rest[..end].parse::<u64>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn texts(g: &ParsedGrid) -> Vec<Vec<&str>> {
        g.cells
            .iter()
            .map(|row| row.iter().map(|c| c.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn tsv_splits_rows_and_columns() {
        let g = parse_tsv("a\tb\nc\td");
        assert_eq!(texts(&g), vec![vec!["a", "b"], vec!["c", "d"]]);
    }

    #[test]
    fn tsv_handles_crlf_and_trailing_newline() {
        let g = parse_tsv("a\tb\r\nc\td\r\n");
        assert_eq!(texts(&g), vec![vec!["a", "b"], vec!["c", "d"]]);
    }

    #[test]
    fn tsv_handles_lone_cr_row_endings() {
        // Old-Mac / some LibreOffice exports use bare CR between rows.
        let g = parse_tsv("a\tb\rc\td");
        assert_eq!(texts(&g), vec![vec!["a", "b"], vec!["c", "d"]]);
    }

    #[test]
    fn tsv_keeps_blank_fields_and_ragged_rows() {
        let g = parse_tsv("x\t\tz\np");
        assert_eq!(texts(&g), vec![vec!["x", "", "z"], vec!["p"]]);
    }

    #[test]
    fn tsv_unquotes_fields_with_embedded_delimiters() {
        let g = parse_tsv("\"a\tb\"\t\"line\nbreak\"\t\"say \"\"hi\"\"\"");
        assert_eq!(texts(&g), vec![vec!["a\tb", "line\nbreak", "say \"hi\""]]);
    }

    #[test]
    fn tsv_preserves_formula_text() {
        let g = parse_tsv("=SUM(A1:A3)");
        assert_eq!(texts(&g), vec![vec!["=SUM(A1:A3)"]]);
    }

    #[test]
    fn grid_from_plain_rows_is_dense() {
        let rows = vec![
            vec![RawCell::new("a", 1, 1), RawCell::new("b", 1, 1)],
            vec![RawCell::new("c", 1, 1), RawCell::new("d", 1, 1)],
        ];
        let g = grid_from_rows(&rows);
        assert_eq!(texts(&g), vec![vec!["a", "b"], vec!["c", "d"]]);
    }

    #[test]
    fn grid_honors_colspan() {
        // <tr><td colspan=2>wide</td></tr><tr><td>a</td><td>b</td></tr>
        let rows = vec![
            vec![RawCell::new("wide", 1, 2)],
            vec![RawCell::new("a", 1, 1), RawCell::new("b", 1, 1)],
        ];
        let g = grid_from_rows(&rows);
        assert_eq!(
            g.cells[0][0],
            ParsedCell {
                text: "wide".into(),
                row_span: 1,
                col_span: 2
            }
        );
        assert_eq!(g.cells[0][1], ParsedCell::empty());
        assert_eq!(texts(&g)[1], vec!["a", "b"]);
    }

    #[test]
    fn grid_honors_rowspan_shifting_later_columns() {
        // Row 0: [A (rowspan 2)] [B]   Row 1: [C] — C must land in column 1,
        // because column 0 of row 1 is covered by A's rowspan.
        let rows = vec![
            vec![RawCell::new("A", 2, 1), RawCell::new("B", 1, 1)],
            vec![RawCell::new("C", 1, 1)],
        ];
        let g = grid_from_rows(&rows);
        assert_eq!(
            g.cells[0][0],
            ParsedCell {
                text: "A".into(),
                row_span: 2,
                col_span: 1
            }
        );
        assert_eq!(g.cells[0][1].text, "B");
        assert_eq!(g.cells[1][0], ParsedCell::empty(), "covered by A's rowspan");
        assert_eq!(g.cells[1][1].text, "C");
    }

    #[test]
    fn grid_clamps_rowspan_beyond_table() {
        let rows = vec![vec![RawCell::new("x", 5, 1)]];
        let g = grid_from_rows(&rows);
        assert_eq!(g.cells[0][0].row_span, 1, "clamped to available rows");
    }

    #[test]
    fn nonce_extraction() {
        assert_eq!(
            nonce_in_html("<table data-zedsheet-nonce=\"99\"><tr></tr></table>"),
            Some(99)
        );
        assert_eq!(nonce_in_html("<table><tr><td>x</td></tr></table>"), None);
        assert_eq!(nonce_in_html("data-zedsheet-nonce=\"notnum\""), None);
    }

    #[test]
    fn nonce_survives_clipboard_wrapper() {
        let wrapped = "<html><body><!--StartFragment--><table data-zedsheet-nonce=\"7\"><tr><td>x</td></tr></table><!--EndFragment--></body></html>";
        assert_eq!(nonce_in_html(wrapped), Some(7));
    }

    #[test]
    fn nonce_not_spoofable_by_cell_text() {
        // A cell literally containing the marker must not be read as our nonce.
        let html = "<table><tr><td>data-zedsheet-nonce=\"42\"</td></tr></table>";
        assert_eq!(nonce_in_html(html), None);
    }
}
