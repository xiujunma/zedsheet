//! Chart model + data extraction (issue #16). A chart binds a kind
//! (bar/line/pie) to a data `CellRange` and floats over the grid anchored at
//! a cell; the renderer draws it as the last body layer so it scrolls with
//! the sheet and updates with the data. This module is pure and host-tested;
//! the canvas drawing lives in `renderer::chart_render`.

use crate::core::cell_range::CellRange;
use crate::core::data_proxy::DataProxy;
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
        };
        let json = serde_json::to_value(&c).unwrap();
        let back: Chart = serde_json::from_value(json).unwrap();
        assert_eq!(back.kind, "bar");
        assert_eq!(back.anchor, "F2");
        assert_eq!(back.width, 360.0);
    }
}
