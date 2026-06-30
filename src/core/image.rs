//! Image insert (Phase 4.2): floating images anchored to a single
//! cell. The model is minimal — a URL, an anchor cell, and a
//! size — and the renderer fetches the URL once on the first
//! frame, caches the decoded `HtmlImageElement`, and blits it
//! to the canvas every subsequent frame.
//!
//! **Out of scope for the first cut** (deferred to follow-ups):
//! - Clipboard paste of an image from the system clipboard
//!   (`paste` event → `clipboardData.items`).
//! - Crop / rotation / opacity / z-order.
//! - Resize handles (a drag-to-resize would mirror the slicer
//!   drag/resize work in Phase 1.1).
//!
//! Pre-4.2 workbooks load with `images: Vec::new()` via the
//! `#[serde(default)]` on the `DataProxy` field, so the new
//! feature is fully backward-compatible.

use serde::{Deserialize, Serialize};

/// Width / height defaults match the cell rectangle in the
/// default `table_renderer` config (column width 110, row
/// height 24) — a single-cell-sized image is the natural
/// starting point.
fn default_image_w() -> f64 {
    220.0
}
fn default_image_h() -> f64 {
    160.0
}

/// One floating image anchored to a single cell.
///
/// `src` is an `http(s)://` URL (or a `data:` URL for embedded
/// images; the canvas image element supports both). The renderer
/// caches the decoded `HtmlImageElement` keyed by `src`, so a
/// workbook with 10 images pointing at the same URL only
/// fetches once.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Image {
    /// URL or data URL of the image to display. Persisted
    /// verbatim in the workbook JSON.
    pub src: String,
    /// Anchor cell — the image's top-left corner sits at this
    /// cell's screen rect. Excel convention.
    pub anchor: String,
    /// Image width in CSS px. Defaults to a 2×1-cell starting
    /// size (220) so the user sees a visible block on insert.
    #[serde(default = "default_image_w")]
    pub width: f64,
    /// Image height in CSS px. Same default.
    #[serde(default = "default_image_h")]
    pub height: f64,
    /// Optional alt text. Not yet surfaced in the UI; reserved
    /// for the accessibility pass.
    #[serde(default)]
    pub alt: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_dimensions_are_2x1_cells() {
        // Pin the default so a future tweak is a deliberate
        // change rather than a silent regression. The numbers
        // match the cell-rect defaults in the renderer config
        // (column width 110, row height 24).
        assert_eq!(default_image_w(), 220.0);
        assert_eq!(default_image_h(), 160.0);
    }

    #[test]
    fn serde_round_trip() {
        let img = Image {
            src: "https://example.com/cat.png".into(),
            anchor: "F2".into(),
            width: 320.0,
            height: 240.0,
            alt: "cat".into(),
        };
        let json = serde_json::to_value(&img).unwrap();
        let back: Image = serde_json::from_value(json).unwrap();
        assert_eq!(img, back);
    }

    #[test]
    fn serde_default_works_when_alt_missing() {
        // Old workbooks (pre-Phase 4.2) won't have the `alt` key.
        // The `#[serde(default)]` on `alt` keeps them loadable.
        let json = serde_json::json!({
            "src": "https://example.com/cat.png",
            "anchor": "F2",
            "width": 320.0,
            "height": 240.0,
        });
        let img: Image = serde_json::from_value(json).unwrap();
        assert_eq!(img.alt, "");
    }

    #[test]
    fn serde_default_works_when_dimensions_missing() {
        // Pre-Phase 4.2 workbooks have neither `alt` nor the
        // dimension overrides. Both `default_image_w` /
        // `default_image_h` (per-field defaults) and the type's
        // own fallbacks need to agree.
        let json = serde_json::json!({
            "src": "https://example.com/cat.png",
            "anchor": "F2",
        });
        let img: Image = serde_json::from_value(json).unwrap();
        assert_eq!(img.width, 220.0);
        assert_eq!(img.height, 160.0);
        assert_eq!(img.alt, "");
    }
}
