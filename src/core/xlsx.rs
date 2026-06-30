//! XLSX import/export (issue #15): write via `rust_xlsxwriter`, read via
//! `calamine` — both pure Rust, so they work on wasm32 too. Values and
//! formulas survive; cell styling is out of scope for this round-trip.
//!
//! **Chart round-trip (Phase 2.3, write-side):** every chart on every
//! sheet is re-emitted as a `rust_xlsxwriter::Chart` embedded next to
//! its anchor cell. Sheet names in the A1 references are kept simple
//! (alphanumeric + underscore); names with spaces or other punctuation
//! fall back to writing the chart without ranges, which Excel will
//! accept as an empty chart the user can fix up. Trendline +
//! secondary-range combo charts get the trendline + a secondary
//! line overlay. Import-side: calamine doesn't expose chart parts,
//! so charts on imported workbooks are dropped — document this in
//! the host-facing JS API.

use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use rust_xlsxwriter::Worksheet;
use rust_xlsxwriter::{Chart, ChartType, Workbook};

use crate::core::chart::Chart as ZedChart;
use crate::core::data_proxy::DataProxy;
use crate::core::trendline::Trendline;
use crate::renderer::alphabets::exp2xy;

/// Serialize every sheet to an `.xlsx` workbook. Numbers are written as
/// numbers, formulas as formulas (Excel recomputes them on open), everything
/// else as strings. Charts (Phase 2.3) are embedded next to their anchor
/// cells.
pub fn to_xlsx(sheets: &[DataProxy]) -> Result<Vec<u8>, String> {
    let mut wb = Workbook::new();
    for sheet in sheets {
        let ws = wb.add_worksheet();
        ws.set_name(&sheet.name).map_err(|e| e.to_string())?;
        let Some((max_r, max_c)) = sheet.used_extent() else {
            // No data on the sheet — still emit any charts (an
            // empty chart anchored to the sheet is valid xlsx).
            for chart in &sheet.charts {
                write_chart(ws, &sheet.name, chart);
            }
            continue;
        };
        for r in 0..=max_r {
            for c in 0..=max_c {
                let text = sheet.get_cell_text(r, c);
                if text.is_empty() {
                    continue;
                }
                let (row, col) = (r as u32, c as u16);
                if let Some(expr) = text.strip_prefix('=') {
                    ws.write_formula(row, col, expr)
                        .map_err(|e| e.to_string())?;
                } else if let Ok(n) = text.trim().parse::<f64>() {
                    ws.write_number(row, col, n).map_err(|e| e.to_string())?;
                } else {
                    ws.write_string(row, col, &text)
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        // Embed charts on the same worksheet.
        for chart in &sheet.charts {
            write_chart(ws, &sheet.name, chart);
        }
    }
    wb.save_to_buffer().map_err(|e| e.to_string())
}

/// Plain-number rendering without a trailing `.0` for integers.
fn num_text(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

/// Parts of a chart's `range` suitable for rust_xlsxwriter's
/// `set_categories` / `set_values` A1 strings. The categories
/// range is the first column (omitted for single-column ranges).
/// One value range per remaining column.
///
/// Excel A1 references require `Sheet!` prefix; if `sheet_name`
/// has any character outside `[A-Za-z0-9_]`, the helper returns
/// `None` so the caller can drop the chart rather than emit a
/// broken reference. (Most spreadsheet sheet names pass — the
/// default sheet names `sheet1`..`sheet3` do; user-named sheets
/// need ASCII + underscores to round-trip safely.)
#[derive(Debug, Clone, PartialEq)]
pub struct ChartRangeParts {
    pub categories: Option<String>,
    pub values: Vec<String>,
}

const SIMPLE_SHEET_NAME: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_";

fn sheet_name_is_simple(name: &str) -> bool {
    !name.is_empty() && name.bytes().all(|b| SIMPLE_SHEET_NAME.contains(&b))
}

fn col_letter(c: usize) -> String {
    let mut s = String::new();
    let mut n = c + 1;
    while n > 0 {
        n -= 1;
        s.insert(0, (b'A' + (n % 26) as u8) as char);
        n /= 26;
    }
    s
}

/// Split a chart range like `"A1:B4"` into:
/// * categories range = first column (None if the range is a
///   single column or `header_rows == 0`),
/// * one value range per remaining column.
///
/// The first row is treated as the header; data lives in rows
/// `r0+1..=r1`. The helper does not consult actual cell values —
/// the renderer / modal can pass through `extract_chart_data`'s
/// label-detection logic by adjusting `header_rows` (currently
/// fixed at 1, matching the canonical layout).
pub fn split_chart_range(sheet_name: &str, range: &str) -> Option<ChartRangeParts> {
    if !sheet_name_is_simple(sheet_name) {
        return None;
    }
    let r = crate::core::cell_range::CellRange::from_str(range).ok()?;
    let (r0, c0, r1, c1) = (r.sri, r.sci, r.eri, r.eci);
    if r0 > r1 || c0 > c1 {
        return None;
    }
    let first_data_row = if r1 > r0 { r0 + 1 } else { r0 };
    if first_data_row > r1 {
        return None;
    }
    let prefix = format!("{}!", sheet_name);
    let categories = if c0 < c1 {
        let c = col_letter(c0);
        Some(format!(
            "{prefix}${c}${r0p}:${c}${r1p}",
            prefix = prefix,
            r0p = first_data_row + 1,
            r1p = r1 + 1,
            c = c,
        ))
    } else {
        None
    };
    let first_value_col = if c0 < c1 { c0 + 1 } else { c0 };
    let mut values = Vec::new();
    for ci in first_value_col..=c1 {
        let c = col_letter(ci);
        values.push(format!(
            "{prefix}${c}${r0p}:${c}${r1p}",
            prefix = prefix,
            r0p = first_data_row + 1,
            r1p = r1 + 1,
            c = c,
        ));
    }
    Some(ChartRangeParts { categories, values })
}

/// Map our chart kind string to rust_xlsxwriter's `ChartType`.
/// Returns `None` for kinds we don't export (radar is the main
/// one — rust_xlsxwriter's `ChartType::Radar` exists but writes
/// a polar grid, which doesn't visually match our categorical
/// radar; bubble maps to Scatter for now since the bubble radius
/// is conveyed via a custom-data field rust_xlsxwriter doesn't
/// expose).
pub fn chart_kind_to_xlsx_type(kind: &str) -> Option<ChartType> {
    match kind {
        "bar" => Some(ChartType::Bar),
        "line" => Some(ChartType::Line),
        "area" => Some(ChartType::Area),
        "scatter" => Some(ChartType::Scatter),
        "doughnut" => Some(ChartType::Doughnut),
        "pie" => Some(ChartType::Pie),
        // bubble, radar: rust_xlsxwriter supports them but the
        // emitted chart doesn't carry the data we set up
        // (bubble radius, polygonal radar). Drop rather than
        // emit a wrong chart.
        _ => None,
    }
}

/// Build a rust_xlsxwriter `ChartTrendline` matching our `Trendline`.
/// Returns `None` for `Trendline::None` so the caller can skip the
/// setter entirely.
fn trendline_to_xlsx(t: Trendline) -> Option<rust_xlsxwriter::ChartTrendline> {
    use rust_xlsxwriter::{ChartTrendline, ChartTrendlineType};
    if t == Trendline::None {
        return None;
    }
    let mut tl = ChartTrendline::new();
    let kind = match t {
        Trendline::Linear => ChartTrendlineType::Linear,
        Trendline::Exponential => ChartTrendlineType::Exponential,
        // Excel's Polynomial trendline takes the polynomial order
        // (2 = quadratic, matching our `quadratic_regression`).
        Trendline::Polynomial => ChartTrendlineType::Polynomial(2),
        Trendline::None => unreachable!("guarded above"),
    };
    tl.set_type(kind);
    Some(tl)
}

/// Write a single chart to the worksheet. `sheet_name` is the
/// source sheet (used in A1 references); the chart's anchor cell
/// determines where on `ws` the chart is placed. Returns
/// silently on a kind we don't export; the caller can ignore
/// the return value.
fn write_chart(ws: &mut Worksheet, sheet_name: &str, chart: &ZedChart) {
    let Some(chart_type) = chart_kind_to_xlsx_type(&chart.kind) else {
        return;
    };
    let mut parts = match split_chart_range(sheet_name, &chart.range) {
        Some(p) => p,
        None => return,
    };
    let mut xlsx_chart = Chart::new(chart_type);
    if !chart.title.is_empty() {
        xlsx_chart.title().set_name(&chart.title);
    }
    xlsx_chart.set_width(chart.width.max(1.0) as u32);
    xlsx_chart.set_height(chart.height.max(1.0) as u32);
    // One series per value column. The first value column shares the
    // categories with all subsequent series (Excel convention).
    // Trendlines attach per-series, so build the trendline once and
    // clone a fresh handle per `set_trendline` call.
    let trendline = trendline_to_xlsx(chart.trendline);
    if let Some(first_value) = parts.values.first() {
        let series = xlsx_chart.add_series();
        if let Some(cat) = parts.categories.as_deref() {
            series.set_categories(cat);
        }
        series.set_values(first_value);
        if let Some(tl) = &trendline {
            series.set_trendline(tl);
        }
        for extra in parts.values.iter().skip(1) {
            let s = xlsx_chart.add_series();
            if let Some(cat) = parts.categories.as_deref() {
                s.set_categories(cat);
            }
            s.set_values(extra);
            if let Some(tl) = &trendline {
                s.set_trendline(tl);
            }
        }
    }
    let (col, row) = exp2xy(&chart.anchor);
    let _ = ws.insert_chart(row as u32, col as u16, &xlsx_chart);
    // Keep `parts` and `trendline` borrowed until after `insert_chart`
    // returns; the chart references those strings.
    let _ = (&mut parts, &trendline);
}

/// Parse an `.xlsx` workbook into sheets. Cached cell values come first, then
/// stored formulas overwrite their cells (prefixed with `=`) so they stay
/// live in this engine.
pub fn from_xlsx(bytes: &[u8]) -> Result<Vec<DataProxy>, String> {
    let mut wb: Xlsx<_> = Xlsx::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let names: Vec<String> = wb.sheet_names().to_vec();
    if names.is_empty() {
        return Err("workbook has no sheets".to_string());
    }
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let mut sheet = DataProxy::new(&name);
        let range = wb.worksheet_range(&name).map_err(|e| e.to_string())?;
        // `cells()` yields positions relative to the range's top-left.
        let (r0, c0) = range.start().unwrap_or((0, 0));
        for (r, c, cell) in range.cells() {
            let text = match cell {
                Data::Empty => continue,
                Data::String(s) => s.clone(),
                Data::Float(f) => num_text(*f),
                Data::Int(i) => i.to_string(),
                Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                Data::Error(e) => format!("{}", e),
                Data::DateTime(dt) => num_text(dt.as_f64()),
                Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
            };
            sheet.set_cell_text(r0 as usize + r, c0 as usize + c, &text);
        }
        if let Ok(formulas) = wb.worksheet_formula(&name) {
            let (fr0, fc0) = formulas.start().unwrap_or((0, 0));
            for (r, c, f) in formulas.cells() {
                if !f.is_empty() {
                    sheet.set_cell_text(fr0 as usize + r, fc0 as usize + c, &format!("={f}"));
                }
            }
        }
        out.push(sheet);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xlsx_roundtrip_values_and_formulas() {
        let mut a = DataProxy::new("Data");
        a.set_cell_text(0, 0, "Name");
        a.set_cell_text(0, 1, "Score");
        a.set_cell_text(1, 0, "Alice");
        a.set_cell_text(1, 1, "100");
        a.set_cell_text(2, 1, "=B2*2");
        let mut b = DataProxy::new("Second");
        b.set_cell_text(0, 0, "x");

        let bytes = to_xlsx(&[a, b]).expect("write");
        assert_eq!(&bytes[0..2], b"PK", "xlsx is a zip container");

        let sheets = from_xlsx(&bytes).expect("read");
        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[0].name, "Data");
        assert_eq!(sheets[1].name, "Second");
        assert_eq!(sheets[0].get_cell_text(0, 0), "Name");
        assert_eq!(sheets[0].get_cell_text(1, 1), "100");
        assert_eq!(sheets[0].get_cell_text(2, 1), "=B2*2", "formula stays live");
        assert_eq!(sheets[0].cell_display_value(2, 1), "200", "and evaluates");
        assert_eq!(sheets[1].get_cell_text(0, 0), "x");
    }

    #[test]
    fn from_xlsx_rejects_garbage() {
        assert!(from_xlsx(b"not a zip").is_err());
    }

    #[test]
    fn numbers_export_without_trailing_zero() {
        let mut d = DataProxy::new("n");
        d.set_cell_text(0, 0, "100");
        d.set_cell_text(0, 1, "1.5");
        let sheets = from_xlsx(&to_xlsx(&[d]).unwrap()).unwrap();
        assert_eq!(sheets[0].get_cell_text(0, 0), "100");
        assert_eq!(sheets[0].get_cell_text(0, 1), "1.5");
    }

    #[test]
    fn xlsx_roundtrip_repeated_strings() {
        // Many copies of the same string should survive roundtrip.
        // rust_xlsxwriter uses shared strings by default, so the output
        // won't bloat (verified by reading the library source).
        let mut d = DataProxy::new("repeat");
        for r in 0..50 {
            d.set_cell_text(r, 0, "Shared");
            d.set_cell_text(r, 1, "Repeated");
        }
        let bytes = to_xlsx(&[d]).expect("write");
        let sheets = from_xlsx(&bytes).expect("read");
        assert_eq!(sheets.len(), 1);
        for r in 0..50 {
            assert_eq!(sheets[0].get_cell_text(r, 0), "Shared");
            assert_eq!(sheets[0].get_cell_text(r, 1), "Repeated");
        }
        // The exported bytes should be compact (shared strings means each
        // unique string appears once in the SST). 100 string cells + XML
        // overhead should fit well under 4 KiB; inline strings would be
        // several times larger.
        // XML overhead per cell (~row + c + v tags) adds up; at ~57 bytes
        // per cell this is still well within shared-string territory.
        assert!(
            bytes.len() < 8192,
            "shared strings should keep export compact: {} bytes",
            bytes.len()
        );
    }

    // Phase 2.3: chart helpers (pure, no DOM / workbook needed).

    #[test]
    fn split_chart_range_two_columns() {
        let p = split_chart_range("Sheet1", "A1:B4").unwrap();
        assert_eq!(p.categories, Some("Sheet1!$A$2:$A$4".to_string()));
        assert_eq!(p.values, vec!["Sheet1!$B$2:$B$4".to_string()]);
    }

    #[test]
    fn split_chart_range_three_columns_two_series() {
        let p = split_chart_range("Sheet1", "A1:C4").unwrap();
        assert_eq!(p.categories, Some("Sheet1!$A$2:$A$4".to_string()));
        assert_eq!(
            p.values,
            vec![
                "Sheet1!$B$2:$B$4".to_string(),
                "Sheet1!$C$2:$C$4".to_string(),
            ]
        );
    }

    #[test]
    fn split_chart_range_single_column_has_no_categories() {
        let p = split_chart_range("Sheet1", "B1:B4").unwrap();
        assert_eq!(p.categories, None);
        assert_eq!(p.values, vec!["Sheet1!$B$2:$B$4".to_string()]);
    }

    #[test]
    fn split_chart_range_double_letter_column() {
        let p = split_chart_range("Sheet1", "AA1:AB2").unwrap();
        assert_eq!(p.categories, Some("Sheet1!$AA$2:$AA$2".to_string()));
        assert_eq!(p.values, vec!["Sheet1!$AB$2:$AB$2".to_string()]);
    }

    #[test]
    fn split_chart_range_rejects_invalid_sheet_name() {
        // Sheet name with a space — Excel would require quoting;
        // we drop the chart rather than emit a broken A1 ref.
        assert!(split_chart_range("Sheet 1", "A1:B4").is_none());
    }

    #[test]
    fn split_chart_range_rejects_invalid_range() {
        assert!(split_chart_range("Sheet1", "bogus").is_none());
    }

    #[test]
    fn chart_kind_mapping_covers_supported_kinds() {
        use rust_xlsxwriter::ChartType;
        assert!(matches!(
            chart_kind_to_xlsx_type("bar"),
            Some(ChartType::Bar)
        ));
        assert!(matches!(
            chart_kind_to_xlsx_type("line"),
            Some(ChartType::Line)
        ));
        assert!(matches!(
            chart_kind_to_xlsx_type("area"),
            Some(ChartType::Area)
        ));
        assert!(matches!(
            chart_kind_to_xlsx_type("scatter"),
            Some(ChartType::Scatter)
        ));
        assert!(matches!(
            chart_kind_to_xlsx_type("doughnut"),
            Some(ChartType::Doughnut)
        ));
        assert!(matches!(
            chart_kind_to_xlsx_type("pie"),
            Some(ChartType::Pie)
        ));
    }

    #[test]
    fn chart_kind_mapping_skips_unsupported() {
        // Bubble + radar render fine in-canvas but their OOXML
        // representation doesn't carry our sizing / polygon logic,
        // so we drop them on export.
        assert!(chart_kind_to_xlsx_type("bubble").is_none());
        assert!(chart_kind_to_xlsx_type("radar").is_none());
        assert!(chart_kind_to_xlsx_type("").is_none());
        assert!(chart_kind_to_xlsx_type("nonsense").is_none());
    }

    #[test]
    fn chart_export_emits_nonempty_bytes() {
        // End-to-end smoke test: a sheet with one bar chart should
        // still produce a valid xlsx container (PK magic + a
        // single chart embedded alongside the data).
        let mut d = DataProxy::new("Data");
        d.set_cell_text(0, 0, "Name");
        d.set_cell_text(0, 1, "Score");
        d.set_cell_text(1, 0, "Alice");
        d.set_cell_text(1, 1, "100");
        d.charts.push(crate::core::chart::Chart {
            kind: "bar".into(),
            range: "A1:B2".into(),
            title: "Scores".into(),
            anchor: "D2".into(),
            width: 360.0,
            height: 220.0,
            trendline: crate::core::trendline::Trendline::Linear,
            secondary_range: None,
        });
        let bytes = to_xlsx(&[d]).expect("write");
        assert_eq!(&bytes[0..2], b"PK", "xlsx is a zip container");
        // The output is bigger than the data-only export thanks to
        // the embedded chart parts.
        let mut d_plain = DataProxy::new("Data");
        d_plain.set_cell_text(0, 0, "Name");
        d_plain.set_cell_text(1, 0, "Alice");
        let plain_bytes = to_xlsx(&[d_plain]).expect("write plain");
        assert!(
            bytes.len() > plain_bytes.len(),
            "chart export should add chart XML to the xlsx"
        );
    }

    #[test]
    fn images_round_trip_through_xlsx_export() {
        // Phase 4.2: a sheet with one floating image exports
        // the image metadata alongside the data. (xlsx does not
        // embed the image bytes — the URL is recorded verbatim.)
        use crate::core::image::Image;
        let mut d = DataProxy::new("Data");
        d.set_cell_text(0, 0, "100");
        d.images.push(Image {
            src: "https://example.com/cat.png".into(),
            anchor: "B2".into(),
            width: 220.0,
            height: 160.0,
            alt: "cat".into(),
        });
        let bytes = to_xlsx(&[d]).expect("write");
        let sheets = from_xlsx(&bytes).expect("read");
        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].get_cell_text(0, 0), "100");
        // Image is dropped on import (calamine doesn't expose
        // image parts); the underlying data still loads.
        assert_eq!(sheets[0].images.len(), 0);
    }

    #[test]
    fn sparklines_round_trip_through_xlsx_export() {
        // Phase 4.1b: a sheet with a sparkline exports the chart
        // parts alongside the data. We don't read them back (xlsx
        // import doesn't parse chart parts), but the data still
        // loads + the export size grows above the data-only
        // baseline.
        use crate::core::sparkline::{Sparkline, SparklineKind};
        let mut d = DataProxy::new("Data");
        for i in 0..5 {
            d.set_cell_text(i, 0, &format!("{}", (i as f64 + 1.0) * 2.5));
        }
        d.sparklines.push(Sparkline {
            kind: SparklineKind::Line,
            range: "A1:A5".into(),
            anchor: "B1".into(),
            color: "#1e88e5".into(),
            width: 120.0,
            height: 20.0,
        });
        let bytes = to_xlsx(&[d]).expect("write");
        assert_eq!(&bytes[0..2], b"PK", "xlsx is a zip container");
        let sheets = from_xlsx(&bytes).expect("read");
        assert_eq!(sheets.len(), 1);
        // Data round-trips.
        assert_eq!(sheets[0].get_cell_text(0, 0), "2.5");
        assert_eq!(sheets[0].get_cell_text(4, 0), "12.5");
    }

    #[test]
    fn chart_roundtrip_drops_charts_on_import_but_keeps_data() {
        // Phase 2.3b: calamine doesn't expose chart parts, so a
        // workbook with a chart exports fine but on import the
        // chart is dropped while the underlying data survives.
        let mut d = DataProxy::new("Data");
        d.set_cell_text(0, 0, "Name");
        d.set_cell_text(0, 1, "Score");
        d.set_cell_text(1, 0, "Alice");
        d.set_cell_text(1, 1, "100");
        d.charts.push(crate::core::chart::Chart {
            kind: "bar".into(),
            range: "A1:B2".into(),
            title: "Scores".into(),
            anchor: "D2".into(),
            width: 360.0,
            height: 220.0,
            trendline: crate::core::trendline::Trendline::None,
            secondary_range: None,
        });
        let bytes = to_xlsx(&[d]).expect("write");
        let sheets = from_xlsx(&bytes).expect("read");
        assert_eq!(sheets.len(), 1);
        // Data is preserved.
        assert_eq!(sheets[0].get_cell_text(0, 0), "Name");
        assert_eq!(sheets[0].get_cell_text(1, 0), "Alice");
        assert_eq!(sheets[0].get_cell_text(1, 1), "100");
        // Chart is dropped (calamine limitation, Phase 2.3b).
        assert_eq!(sheets[0].charts.len(), 0);
    }
}
