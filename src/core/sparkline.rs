//! Sparklines (Phase 4.1): inline mini-charts drawn inside a single
//! cell. Three kinds are supported:
//!
//! - [`SparklineKind::Line`]: a connected polyline, normalised to
//!   the cell's plot rectangle (4px padding all around so the line
//!   doesn't touch the cell edges).
//! - [`SparklineKind::Column`]: a thin mini bar per data point, with
//!   positive bars drawn up from the zero line and negative bars down.
//! - [`SparklineKind::WinLoss`]: a green/red block per data point
//!   depending on the sign of the value (Excel convention).
//!
//! Sparklines live on `DataProxy` as `Vec<Sparkline>` (parallel to
//! `Vec<Chart>`); each entry has its own anchor cell + data range.
//! The renderer reads them after the body so they overlay on top
//! of the cell's normal text / number rendering — text is still
//! visible underneath a sparkline.

use serde::{Deserialize, Serialize};

/// Style of sparkline to render. Persisted lowercase so the workbook
/// JSON stays readable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SparklineKind {
    #[default]
    Line,
    Column,
    WinLoss,
}

impl SparklineKind {
    /// True if this kind has a positive/negative split (column
    /// draws bars above and below zero; win-loss is the same).
    pub fn has_zero_line(self) -> bool {
        !matches!(self, SparklineKind::Line)
    }
}

/// One sparkline anchored to a single cell, sourcing values from
/// `range`. `range` is a cell-range expression like `"A1:A12"`;
/// the same parser the chart engine uses (`extract_chart_data`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Sparkline {
    pub kind: SparklineKind,
    pub range: String,
    /// Anchor cell — top-left corner of the cell that hosts the
    /// sparkline. Excel convention is a single cell.
    pub anchor: String,
    /// Hex colour, e.g. `"#1e88e5"`. Defaults to a neutral blue
    /// if empty / unparseable.
    #[serde(default)]
    pub color: String,
    /// Width / height in CSS px (Excel-style). Defaults match the
    /// default cell dimensions in the renderer.
    #[serde(default = "default_sparkline_w")]
    pub width: f64,
    #[serde(default = "default_sparkline_h")]
    pub height: f64,
}

fn default_sparkline_w() -> f64 {
    120.0
}
fn default_sparkline_h() -> f64 {
    20.0
}

impl Sparkline {
    /// Normalise `color` to a guaranteed-valid hex string. Falls
    /// back to a neutral blue (`"#1e88e5"`) for empty input or
    /// invalid hex. Pure, host-tested.
    pub fn effective_color(&self) -> String {
        let s = self.color.strip_prefix('#').unwrap_or(&self.color);
        if s.len() == 6 && s.chars().all(|c| c.is_ascii_hexdigit()) {
            format!("#{}", s.to_lowercase())
        } else {
            "#1e88e5".to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_kind_is_line() {
        assert_eq!(SparklineKind::default(), SparklineKind::Line);
    }

    #[test]
    fn column_and_winloss_have_zero_line() {
        assert!(!SparklineKind::Line.has_zero_line());
        assert!(SparklineKind::Column.has_zero_line());
        assert!(SparklineKind::WinLoss.has_zero_line());
    }

    #[test]
    fn serde_round_trip() {
        for k in [
            SparklineKind::Line,
            SparklineKind::Column,
            SparklineKind::WinLoss,
        ] {
            let s = serde_json::to_string(&k).unwrap();
            let back: SparklineKind = serde_json::from_str(&s).unwrap();
            assert_eq!(k, back);
        }
        // Lowercase, not the Rust default (PascalCase).
        assert_eq!(
            serde_json::to_string(&SparklineKind::Column).unwrap(),
            "\"column\""
        );
    }

    #[test]
    fn effective_color_accepts_canonical_hex() {
        let s = Sparkline {
            kind: SparklineKind::Line,
            range: "A1:A2".into(),
            anchor: "B1".into(),
            color: "#1e88e5".into(),
            width: 0.0,
            height: 0.0,
        };
        assert_eq!(s.effective_color(), "#1e88e5");
        // Uppercase is normalised.
        let s = Sparkline {
            color: "#FFFFFF".into(),
            ..s
        };
        assert_eq!(s.effective_color(), "#ffffff");
    }

    #[test]
    fn effective_color_falls_back_on_invalid() {
        let s = Sparkline {
            kind: SparklineKind::Line,
            range: "A1:A2".into(),
            anchor: "B1".into(),
            color: String::new(),
            width: 0.0,
            height: 0.0,
        };
        assert_eq!(s.effective_color(), "#1e88e5");
        let s = Sparkline {
            color: "not-a-color".into(),
            ..s
        };
        assert_eq!(s.effective_color(), "#1e88e5");
        let s = Sparkline {
            color: "#abc".into(),
            ..s
        };
        assert_eq!(s.effective_color(), "#1e88e5");
    }

    #[test]
    fn sparkline_serde_round_trip() {
        let s = Sparkline {
            kind: SparklineKind::Column,
            range: "A1:A12".into(),
            anchor: "B1".into(),
            color: "#e53935".into(),
            width: 120.0,
            height: 20.0,
        };
        let json = serde_json::to_value(&s).unwrap();
        let back: Sparkline = serde_json::from_value(json).unwrap();
        assert_eq!(s, back);
    }
}
