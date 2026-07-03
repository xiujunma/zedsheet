//! Chart model + data extraction (issue #16). A chart binds a kind
//! (bar/line/pie) to a data `CellRange` and floats over the grid anchored at
//! a cell; the renderer draws it as the last body layer so it scrolls with
//! the sheet and updates with the data. This module is pure and host-tested;
//! the canvas drawing lives in `renderer::chart_render`.

use crate::core::cell_range::CellRange;
use crate::core::data_proxy::DataProxy;
use crate::core::trendline::Trendline;
use serde::{Deserialize, Serialize};

fn default_chart_w() -> f64 {
    360.0
}
fn default_chart_h() -> f64 {
    220.0
}

/// One chart on a sheet.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chart {
    /// `"bar" | "line" | "pie"`.
    pub kind: String,
    /// Data range expression, e.g. `"A1:B4"`.
    pub range: String,
    #[serde(default)]
    pub title: String,
    /// Anchor cell (the chart's top-left corner), e.g. `"F2"`.
    pub anchor: String,
    #[serde(default = "default_chart_w")]
    pub width: f64,
    #[serde(default = "default_chart_h")]
    pub height: f64,
    /// Optional trendline overlaid on the chart's series (Phase 1.2).
    /// `#[serde(default)]` keeps old workbooks (which lack the key)
    /// loading with `Trendline::None`.
    #[serde(default)]
    pub trendline: Trendline,
    /// Optional Y-axis tick-label format string. Excel-like:
    /// `0` (integer), `0.00` (2 decimals), `#,##0` (thousands separator),
    /// `$0.00` (currency), `0%` (percent). `None` (or empty) keeps the
    /// default pretty-printer. `#[serde(default)]` keeps pre-1.4
    /// workbooks loading with `None`.
    #[serde(default)]
    pub y_axis_format: Option<String>,
    /// Optional secondary-axis data range (Phase 2.2). When `Some`,
    /// the renderer draws a right-hand Y axis scaled to this range's
    /// values, and the secondary range's series are drawn as a line
    /// overlay on top of the primary bars. The primary range's
    /// labels are reused — both ranges must have the same row count.
    /// `#[serde(default)]` keeps pre-2.2 workbooks loading with
    /// `None`.
    #[serde(default)]
    pub secondary_range: Option<String>,
}

/// Chart-ready table: one label per category, one or more named series.
#[derive(Debug, Clone, PartialEq)]
pub struct ChartData {
    pub labels: Vec<String>,
    pub series: Vec<(String, Vec<f64>)>,
}

fn is_numeric(s: &str) -> bool {
    !s.trim().is_empty() && s.trim().parse::<f64>().is_ok()
}

/// Extract labels and series from `range` (computed raw values, so formula
/// cells chart their results):
/// * the first column becomes the labels when it holds non-numeric data;
/// * the first row becomes the series names when every value cell in it is
///   non-numeric (a header row);
/// * remaining columns are the series. `None` when the range is invalid or
///   holds no data rows/columns.
pub fn extract_chart_data(sheet: &DataProxy, range: &str) -> Option<ChartData> {
    let r = CellRange::from_str(range).ok()?;
    let (r0, c0, r1, c1) = (r.sri, r.sci, r.eri, r.eci);
    let raw = |ri: usize, ci: usize| sheet.cell_raw_value(ri, ci);

    // Label column: any non-numeric, non-empty cell below the first row.
    let label_col = c1 > c0
        && (r0..=r1)
            .skip(1)
            .any(|ri| !raw(ri, c0).trim().is_empty() && !is_numeric(&raw(ri, c0)));
    let first_value_col = if label_col { c0 + 1 } else { c0 };
    if first_value_col > c1 {
        return None;
    }

    // Header row: more than one row, and every non-empty value cell in the
    // first row is non-numeric (with at least one non-empty).
    let header_cells: Vec<String> = (first_value_col..=c1).map(|ci| raw(r0, ci)).collect();
    let header_row = r1 > r0
        && header_cells.iter().any(|v| !v.trim().is_empty())
        && header_cells
            .iter()
            .all(|v| v.trim().is_empty() || !is_numeric(v));
    let first_data_row = if header_row { r0 + 1 } else { r0 };
    if first_data_row > r1 {
        return None;
    }

    let labels: Vec<String> = (first_data_row..=r1)
        .enumerate()
        .map(|(i, ri)| {
            if label_col {
                let v = raw(ri, c0);
                if v.trim().is_empty() {
                    (i + 1).to_string()
                } else {
                    v
                }
            } else {
                (i + 1).to_string()
            }
        })
        .collect();

    let series: Vec<(String, Vec<f64>)> = (first_value_col..=c1)
        .map(|ci| {
            let name = if header_row {
                let h = raw(r0, ci);
                if h.trim().is_empty() {
                    format!("Series {}", ci - first_value_col + 1)
                } else {
                    h
                }
            } else {
                format!("Series {}", ci - first_value_col + 1)
            };
            let values = (first_data_row..=r1)
                .map(|ri| raw(ri, ci).trim().parse::<f64>().unwrap_or(0.0))
                .collect();
            (name, values)
        })
        .collect();

    Some(ChartData { labels, series })
}

/// Extract a secondary-axis data range (Phase 2.2). Behaviour
/// mirrors [`extract_chart_data`] but reuses the primary range's
/// labels — the secondary range's first row (header) and first
/// column (labels) are ignored; the secondary columns are treated
/// as a pure value block aligned with `primary_labels`.
///
/// Returns `None` when:
/// * the range string is invalid,
/// * the range parses to fewer rows than `primary_labels.len()`,
/// * the range has zero value columns.
///
/// Used by the renderer to draw a dual-axis chart with a line
/// overlay scaled to the secondary range's values.
pub fn extract_secondary_chart_data(
    sheet: &DataProxy,
    range: &str,
    primary_labels: &[String],
) -> Option<ChartData> {
    let r = CellRange::from_str(range).ok()?;
    let (r0, c0, r1, c1) = (r.sri, r.sci, r.eri, r.eci);
    let raw = |ri: usize, ci: usize| sheet.cell_raw_value(ri, ci);
    // Treat the whole range as a header-less, label-less value
    // block. We need at least `primary_labels.len()` rows; we accept
    // more and clip, matching Excel's behaviour where the
    // secondary range's row count must equal the primary's.
    let n_rows = primary_labels.len();
    if r1 + 1 < r0 + n_rows {
        return None;
    }
    if c0 > c1 {
        return None;
    }
    let labels: Vec<String> = primary_labels.to_vec();
    let series: Vec<(String, Vec<f64>)> = (c0..=c1)
        .map(|ci| {
            let name = format!("Secondary {}", ci - c0 + 1);
            let values = (0..n_rows)
                .map(|i| {
                    let ri = r0 + i;
                    raw(ri, ci).trim().parse::<f64>().unwrap_or(0.0)
                })
                .collect();
            (name, values)
        })
        .collect();
    if series.is_empty() {
        return None;
    }
    Some(ChartData { labels, series })
}

/// True when `range` parses and has at least `min_rows` data rows
/// available (r1 - r0 + 1 >= min_rows). Used by the modal to gate
/// the "Apply" button on the secondary-range field before the
/// renderer tries to draw.
pub fn range_has_rows(range: &str, min_rows: usize) -> bool {
    let Ok(r) = CellRange::from_str(range) else {
        return false;
    };
    r.eri + 1 >= r.sri + min_rows
}

/// Round `v` up to a "nice" axis maximum: 1, 2, or 5 × 10^k.
pub fn nice_ceil(v: f64) -> f64 {
    if v <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(v.log10().floor());
    let n = v / mag;
    let nice = if n <= 1.0 {
        1.0
    } else if n <= 2.0 {
        2.0
    } else if n <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * mag
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sheet(cells: &[(usize, usize, &str)]) -> DataProxy {
        let mut d = DataProxy::new("t");
        for (r, c, t) in cells {
            d.set_cell_text(*r, *c, t);
        }
        d
    }

    #[test]
    fn extracts_labels_header_and_series() {
        // The demo shape: header row + label column + one value column.
        let d = sheet(&[
            (0, 0, "Name"),
            (0, 1, "Score"),
            (1, 0, "Alice"),
            (1, 1, "100"),
            (2, 0, "Bob"),
            (2, 1, "200"),
            (3, 0, "Total"),
            (3, 1, "=B2+B3"),
        ]);
        let data = extract_chart_data(&d, "A1:B4").unwrap();
        assert_eq!(data.labels, vec!["Alice", "Bob", "Total"]);
        assert_eq!(data.series.len(), 1);
        assert_eq!(data.series[0].0, "Score");
        assert_eq!(data.series[0].1, vec![100.0, 200.0, 300.0]); // formula charted
    }

    #[test]
    fn multiple_series_and_no_labels() {
        let d = sheet(&[(0, 0, "10"), (0, 1, "1"), (1, 0, "20"), (1, 1, "2")]);
        let data = extract_chart_data(&d, "A1:B2").unwrap();
        assert_eq!(data.labels, vec!["1", "2"]); // positional
        assert_eq!(data.series.len(), 2);
        assert_eq!(data.series[0].0, "Series 1");
        assert_eq!(data.series[0].1, vec![10.0, 20.0]);
        assert_eq!(data.series[1].1, vec![1.0, 2.0]);
    }

    #[test]
    fn single_numeric_column() {
        let d = sheet(&[(0, 0, "5"), (1, 0, "7")]);
        let data = extract_chart_data(&d, "A1:A2").unwrap();
        assert_eq!(data.labels, vec!["1", "2"]);
        assert_eq!(data.series[0].1, vec![5.0, 7.0]);
    }

    #[test]
    fn rejects_invalid_or_empty() {
        let d = sheet(&[]);
        assert!(extract_chart_data(&d, "nonsense").is_none());
        // A text-only column charts as zeros (first cell read as the header).
        let d = sheet(&[(0, 0, "only"), (1, 0, "labels")]);
        let data = extract_chart_data(&d, "A1:A2").unwrap();
        assert_eq!(data.series[0].0, "only");
        assert!(data.series[0].1.iter().all(|v| *v == 0.0));
    }

    #[test]
    fn nice_ceil_rounds_to_1_2_5() {
        assert_eq!(nice_ceil(7.0), 10.0);
        assert_eq!(nice_ceil(43.0), 50.0);
        assert_eq!(nice_ceil(120.0), 200.0);
        assert_eq!(nice_ceil(300.0), 500.0);
        assert_eq!(nice_ceil(0.0), 1.0);
        assert_eq!(nice_ceil(1.0), 1.0);
    }

    #[test]
    fn chart_serde_roundtrip() {
        let c = Chart {
            kind: "bar".into(),
            range: "A1:B4".into(),
            title: "Scores".into(),
            anchor: "F2".into(),
            width: 360.0,
            height: 220.0,
            trendline: Trendline::Linear,
            y_axis_format: None,
            secondary_range: None,
        };
        let json = serde_json::to_value(&c).unwrap();
        let back: Chart = serde_json::from_value(json).unwrap();
        assert_eq!(back.kind, "bar");
        assert_eq!(back.anchor, "F2");
        assert_eq!(back.width, 360.0);
        assert_eq!(back.height, 220.0);
        assert_eq!(back.trendline, Trendline::Linear);
        // Backward-compat: pre-2.2 workbooks without `secondary_range`
        // round-trip with `None`.
        let legacy = serde_json::json!({
            "kind": "bar",
            "range": "A1:B4",
            "anchor": "F2",
        });
        let back: Chart = serde_json::from_value(legacy).unwrap();
        assert_eq!(back.secondary_range, None);
        assert_eq!(back.y_axis_format, None);
    }

    #[test]
    fn secondary_round_trips_with_secondary_range() {
        let c = Chart {
            kind: "bar".into(),
            range: "A1:B4".into(),
            title: "Sales".into(),
            anchor: "F2".into(),
            width: 360.0,
            height: 220.0,
            trendline: Trendline::None,
            y_axis_format: None,
            secondary_range: Some("D1:E4".into()),
        };
        let json = serde_json::to_value(&c).unwrap();
        let back: Chart = serde_json::from_value(json).unwrap();
        assert_eq!(back.secondary_range, Some("D1:E4".to_string()));
    }

    #[test]
    fn y_axis_format_round_trips() {
        let c = Chart {
            kind: "bar".into(),
            range: "A1:B4".into(),
            title: "Revenue".into(),
            anchor: "F2".into(),
            width: 360.0,
            height: 220.0,
            trendline: Trendline::None,
            y_axis_format: Some("$#,##0.00".into()),
            secondary_range: None,
        };
        let json = serde_json::to_string(&c).unwrap();
        assert!(json.contains("y_axis_format"));
        let back: Chart = serde_json::from_str(&json).unwrap();
        assert_eq!(back.y_axis_format.as_deref(), Some("$#,##0.00"));
        // Backward-compat: a pre-1.4 workbook without `y_axis_format` loads
        // with None — `#[serde(default)]` on the new field.
        let legacy = serde_json::json!({
            "kind": "bar",
            "range": "A1:B4",
            "anchor": "F2",
        });
        let back: Chart = serde_json::from_value(legacy).unwrap();
        assert_eq!(back.y_axis_format, None);
    }

    #[test]
    fn extract_secondary_uses_primary_labels() {
        // Primary range: labels in col A + values in col B, 3 rows.
        let d = sheet(&[
            (0, 0, "Q1"),
            (0, 1, "10"),
            (1, 0, "Q2"),
            (1, 1, "20"),
            (2, 0, "Q3"),
            (2, 1, "30"),
        ]);
        let primary = extract_chart_data(&d, "A1:B3").unwrap();
        // Secondary range: a 3-row, 1-col block — labels ignored.
        let d2 = sheet(&[(0, 3, "100"), (1, 3, "200"), (2, 3, "300")]);
        let secondary = extract_secondary_chart_data(&d2, "D1:D3", &primary.labels).unwrap();
        // Labels are reused from primary.
        assert_eq!(secondary.labels, vec!["Q1", "Q2", "Q3"]);
        // One series with three values from the secondary range.
        assert_eq!(secondary.series.len(), 1);
        assert_eq!(secondary.series[0].0, "Secondary 1");
        assert_eq!(secondary.series[0].1, vec![100.0, 200.0, 300.0]);
    }

    #[test]
    fn extract_secondary_rejects_short_range() {
        // Primary has 3 rows; secondary has only 1.
        let d = sheet(&[
            (0, 0, "Q1"),
            (0, 1, "10"),
            (1, 0, "Q2"),
            (1, 1, "20"),
            (2, 0, "Q3"),
            (2, 1, "30"),
        ]);
        let primary = extract_chart_data(&d, "A1:B3").unwrap();
        assert_eq!(primary.labels.len(), 3);
        // Secondary range with only 1 row can't cover 3 labels.
        let d2 = sheet(&[(0, 3, "100")]);
        assert!(extract_secondary_chart_data(&d2, "D1:D1", &primary.labels).is_none());
    }

    #[test]
    fn range_has_rows_accepts_equal_or_larger() {
        assert!(range_has_rows("A1:A3", 3));
        assert!(range_has_rows("A1:A5", 3));
        assert!(!range_has_rows("A1:A2", 3));
        assert!(!range_has_rows("bogus", 3));
    }
}
