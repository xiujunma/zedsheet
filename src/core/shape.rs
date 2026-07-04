//! Shapes (Phase 6): floating drawing-layer rectangles, lines, and
//! text boxes anchored to a single cell (issue #10's empty
//! `editor/` placeholder pointed at this feature). The renderer
//! blits each shape on top of the body the same way images do,
//! scrolling with the underlying cells.
//!
//! Three kinds:
//! - [`ShapeKind::Rect`]: a stroked rectangle
//! - [`ShapeKind::Line`]: a straight line from the anchor to
//!   `(anchor_row + height, anchor_col + width)`
//! - [`ShapeKind::Text`]: a text box with a background fill
//!
//! A shape is `Serialize`/`Deserialize` so the workbook JSON
//! round-trips through `get_data` / `set_data` unchanged.

use serde::{Deserialize, Serialize};

/// What kind of shape to draw.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum ShapeKind {
    #[default]
    Rect,
    Line,
    Text,
}

/// One drawing-layer shape. Anchored to a single cell (`anchor`)
/// with a top-left position derived from that cell's screen
/// geometry; `width` / `height` extend in CSS px. `color` is the
/// stroke / text color (defaults to a neutral black when empty);
/// `fill` is only used for `Text` (the text-box background).
/// `#[serde(default)]` on every field keeps pre-#10 workbooks
/// loadable.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Shape {
    pub kind: ShapeKind,
    pub anchor: String,
    #[serde(default = "default_shape_w")]
    pub width: f64,
    #[serde(default = "default_shape_h")]
    pub height: f64,
    #[serde(default)]
    pub color: String,
    #[serde(default)]
    pub fill: String,
    /// The text body, used only for `ShapeKind::Text`.
    #[serde(default)]
    pub text: String,
}

fn default_shape_w() -> f64 {
    140.0
}
fn default_shape_h() -> f64 {
    80.0
}

impl Shape {
    /// Normalise `color` / `fill` to guaranteed-valid hex strings
    /// (matching the sparkline convention). Empty or invalid
    /// input falls back to a neutral colour.
    pub fn effective_color(&self) -> String {
        Self::normalise_hex(&self.color, "#1e88e5")
    }
    pub fn effective_fill(&self) -> String {
        Self::normalise_hex(&self.fill, "#fffbe6")
    }
    fn normalise_hex(s: &str, fallback: &str) -> String {
        let stripped = s.strip_prefix('#').unwrap_or(s);
        if stripped.len() == 6 && stripped.chars().all(|c| c.is_ascii_hexdigit()) {
            format!("#{}", stripped.to_lowercase())
        } else {
            fallback.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_kind_is_rect() {
        assert_eq!(ShapeKind::default(), ShapeKind::Rect);
    }

    #[test]
    fn serde_round_trip() {
        for k in [ShapeKind::Rect, ShapeKind::Line, ShapeKind::Text] {
            let s = serde_json::to_string(&k).unwrap();
            let back: ShapeKind = serde_json::from_str(&s).unwrap();
            assert_eq!(k, back);
        }
        assert_eq!(serde_json::to_string(&ShapeKind::Text).unwrap(), "\"text\"");
    }

    #[test]
    fn shape_round_trip() {
        let s = Shape {
            kind: ShapeKind::Text,
            anchor: "B2".into(),
            width: 200.0,
            height: 60.0,
            color: "#1e88e5".into(),
            fill: "#fffbe6".into(),
            text: "hello".into(),
        };
        let json = serde_json::to_value(&s).unwrap();
        let back: Shape = serde_json::from_value(json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn effective_color_falls_back_on_invalid() {
        let s = Shape {
            kind: ShapeKind::Rect,
            anchor: "A1".into(),
            width: 0.0,
            height: 0.0,
            color: String::new(),
            fill: String::new(),
            text: String::new(),
        };
        assert_eq!(s.effective_color(), "#1e88e5");
        assert_eq!(s.effective_fill(), "#fffbe6");
    }
}
