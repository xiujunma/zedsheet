//! Serialize a selected range to the clipboard's `text/plain` (TSV) and
//! `text/html` (`<table>`) flavors. Pure and host-tested.

use std::collections::HashSet;

use crate::core::cell_range::CellRange;
use crate::core::data_proxy::DataProxy;
use crate::core::html_util::{esc, td_style};

/// Serialize a range to tab-separated values.
///
/// Emits each cell's raw text — for a formula cell that is the formula itself
/// (e.g. `=SUM(A1:A3)`), so Excel re-interprets it as a live formula on paste.
/// Fields containing a tab, newline, or quote are wrapped in double quotes with
/// internal quotes doubled, matching how Excel reads pasted text. Rows are
/// joined with CRLF and there is no trailing newline (which would otherwise
/// paste as a spurious empty row).
pub fn to_tsv(sheet: &DataProxy, range: &CellRange) -> String {
    let rows: Vec<String> = (range.sri..=range.eri)
        .map(|r| {
            (range.sci..=range.eci)
                .map(|c| tsv_field(&sheet.get_cell_text(r, c)))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect();
    rows.join("\r\n")
}

/// Quote a TSV field only when it contains a tab, line break, or quote.
fn tsv_field(v: &str) -> String {
    if v.contains('\t') || v.contains('\n') || v.contains('\r') || v.contains('"') {
        format!("\"{}\"", v.replace('"', "\"\""))
    } else {
        v.to_string()
    }
}

/// Serialize a range to an HTML `<table>` fragment.
///
/// Merged cells become `colspan`/`rowspan` (clamped to the selection so a
/// partially-selected merge still copies its content), per-cell styles are
/// inlined (including conditional formatting), and `nonce` is embedded so a
/// subsequent in-app paste can detect our own payload and round-trip it
/// losslessly. Cell text is the raw formula text, consistent with the TSV.
pub fn to_html(sheet: &DataProxy, range: &CellRange, nonce: u64) -> String {
    let (sri, sci, eri, eci) = (range.sri, range.sci, range.eri, range.eci);
    let mut covered: HashSet<(usize, usize)> = HashSet::new();
    let mut body = String::new();
    for r in sri..=eri {
        body.push_str("<tr>");
        for c in sci..=eci {
            if covered.contains(&(r, c)) {
                continue;
            }
            // Resolve the cell's content source and clamped span.
            let (src_r, src_c, rowspan, colspan) = match sheet.cell_merge(r, c) {
                Some(m) => {
                    // Top-left of the merge as visible inside the selection.
                    let top = m.sri.max(sri);
                    let left = m.sci.max(sci);
                    if (r, c) != (top, left) {
                        // A covered remnant whose anchor sits above/left of the
                        // selection — skip it (and remember, defensively).
                        covered.insert((r, c));
                        continue;
                    }
                    let bottom = m.eri.min(eri);
                    let right = m.eci.min(eci);
                    for rr in top..=bottom {
                        for cc in left..=right {
                            if (rr, cc) != (top, left) {
                                covered.insert((rr, cc));
                            }
                        }
                    }
                    (m.sri, m.sci, bottom - top + 1, right - left + 1)
                }
                None => (r, c, 1, 1),
            };

            let mut style = sheet.get_cell_style(src_r, src_c);
            sheet.apply_cond_format(src_r, src_c, &mut style);
            let span = if rowspan > 1 || colspan > 1 {
                format!(" colspan=\"{colspan}\" rowspan=\"{rowspan}\"")
            } else {
                String::new()
            };
            body.push_str(&format!(
                "<td{span} style=\"{}\">{}</td>",
                td_style(&style),
                esc(&sheet.get_cell_text(src_r, src_c))
            ));
        }
        body.push_str("</tr>");
    }
    format!("<table data-zedsheet-nonce=\"{nonce}\">{body}</table>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::data_proxy::Style;

    fn range(sri: usize, sci: usize, eri: usize, eci: usize) -> CellRange {
        CellRange::new(sri, sci, eri, eci)
    }

    #[test]
    fn tsv_lays_out_rows_and_columns() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "a");
        d.set_cell_text(0, 1, "b");
        d.set_cell_text(1, 0, "c");
        d.set_cell_text(1, 1, "d");
        assert_eq!(to_tsv(&d, &range(0, 0, 1, 1)), "a\tb\r\nc\td");
    }

    #[test]
    fn tsv_emits_formula_text_not_computed_value() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "2");
        d.set_cell_text(0, 1, "=A1*3");
        // Excel should receive the formula, not "6".
        assert_eq!(to_tsv(&d, &range(0, 0, 0, 1)), "2\t=A1*3");
    }

    #[test]
    fn tsv_quotes_fields_with_tabs_newlines_quotes() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "a\tb");
        d.set_cell_text(0, 1, "line\nbreak");
        d.set_cell_text(0, 2, "say \"hi\"");
        assert_eq!(
            to_tsv(&d, &range(0, 0, 0, 2)),
            "\"a\tb\"\t\"line\nbreak\"\t\"say \"\"hi\"\"\""
        );
    }

    #[test]
    fn tsv_preserves_empty_cells_as_blank_fields() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "x");
        d.set_cell_text(0, 2, "z");
        assert_eq!(to_tsv(&d, &range(0, 0, 0, 2)), "x\t\tz");
    }

    #[test]
    fn html_escapes_and_carries_nonce() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "<b>&\"");
        let html = to_html(&d, &range(0, 0, 0, 0), 42);
        assert!(html.starts_with("<table data-zedsheet-nonce=\"42\">"));
        assert!(html.contains("&lt;b&gt;&amp;&quot;"));
        assert!(html.ends_with("</table>"));
    }

    #[test]
    fn html_emits_formula_text() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "=1+2");
        let html = to_html(&d, &range(0, 0, 0, 0), 1);
        assert!(html.contains(">=1+2</td>"), "got: {html}");
    }

    #[test]
    fn html_inlines_styles() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "x");
        let bold = Style {
            bold: true,
            ..Style::default()
        };
        let idx = d.add_style(bold);
        d.set_cell_style(0, 0, idx);
        let html = to_html(&d, &range(0, 0, 0, 0), 1);
        assert!(html.contains("font-weight:bold;"));
    }

    #[test]
    fn html_renders_full_merge_as_span_and_skips_covered() {
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "wide");
        d.merges.add(CellRange::new(0, 0, 0, 1)); // A1:B1
        d.get_cell_or_new(0, 0).merge = Some((0, 1));
        let html = to_html(&d, &range(0, 0, 0, 1), 1);
        assert!(html.contains("colspan=\"2\" rowspan=\"1\""));
        // Only one <td> in the row — the covered cell is skipped.
        assert_eq!(html.matches("<td").count(), 1);
        assert!(html.contains(">wide</td>"));
    }

    #[test]
    fn html_clamps_a_partially_selected_merge() {
        // A 1x3 merge A1:C1, but only B1:C1 is selected. The visible part keeps
        // the merge content and a clamped colspan of 2.
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "merged");
        d.merges.add(CellRange::new(0, 0, 0, 2));
        d.get_cell_or_new(0, 0).merge = Some((0, 2));
        let html = to_html(&d, &range(0, 1, 0, 2), 1);
        assert!(html.contains("colspan=\"2\" rowspan=\"1\""), "got: {html}");
        assert!(
            html.contains(">merged</td>"),
            "content comes from the origin"
        );
        assert_eq!(html.matches("<td").count(), 1);
    }
}
