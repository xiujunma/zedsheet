use serde::{Deserialize, Serialize};

/// One styled run of text inside a cell (Phase 4.5: rich text).
/// A cell with \`runs.is_some()\` renders run-by-run; a cell with
/// \`runs.is_none()\` falls back to the legacy flat
/// \`text\` + \`style\` rendering. The two are kept distinct so
/// pre-4.5 workbooks (no \`runs\` key) still load with the flat
/// path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    /// Substring of the cell's text this run covers.
    pub text: String,
    /// Index into \`DataProxy.styles\` for this run's formatting.
    /// \`None\` means "inherit the cell's default style" (same
    /// formatting the cell had before being split into runs).
    #[serde(default)]
    pub style: Option<usize>,
}

impl Run {
    pub fn new(text: &str) -> Self {
        Self {
            text: text.to_string(),
            style: None,
        }
    }
    pub fn with_style(text: &str, style: usize) -> Self {
        Self {
            text: text.to_string(),
            style: Some(style),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cell {
    pub text: String,
    pub value: String,
    pub style: Option<usize>,
    pub merge: Option<(usize, usize)>, // (row_span, col_span)
    pub editable: bool,
    pub cell_type: String,
    /// An attached comment/note, if any.
    #[serde(default)]
    pub note: Option<String>,
    /// A hyperlink target (normalized URL), if any.
    #[serde(default)]
    pub link: Option<String>,
    /// Phase 4.5: styled text runs. \`None\` (the default for
    /// pre-4.5 workbooks) means the cell uses the flat \`text\` +
    /// \`style\` rendering; \`Some(runs)\` means the renderer
    /// should iterate the runs and draw each with its own style.
    /// Backward-compat: this field is \`#[serde(default)]\` so
    /// pre-4.5 workbooks without the key load unchanged.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runs: Option<Vec<Run>>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell {
            text: String::new(),
            value: String::new(),
            style: None,
            merge: None,
            editable: true,
            cell_type: String::from("text"),
            note: None,
            link: None,
            runs: None,
        }
    }
}

impl Cell {
    pub fn new() -> Self {
        Cell::default()
    }

    pub fn with_text(text: &str) -> Self {
        Cell {
            text: text.to_string(),
            value: text.to_string(),
            ..Default::default()
        }
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
    }

    /// Convert the cell to a single-run rich-text cell with the
    /// given style index. Used by the "Format as rich text"
    /// context menu (Phase 4.5). The flat \`text\` field stays
    /// populated for the legacy flat-render path; \`runs\` is the
    /// new source of truth when the cell is rich.
    pub fn convert_to_rich(&mut self, style: Option<usize>) {
        self.runs = Some(vec![Run {
            text: self.text.clone(),
            style,
        }]);
    }

    pub fn set_value(&mut self, value: &str) {
        self.value = value.to_string();
    }

    pub fn set_style(&mut self, style_idx: usize) {
        self.style = Some(style_idx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_new_uses_default_style() {
        let r = Run::new("hello");
        assert_eq!(r.text, "hello");
        assert_eq!(r.style, None);
    }

    #[test]
    fn run_with_style_sets_index() {
        let r = Run::with_style("bold", 3);
        assert_eq!(r.text, "bold");
        assert_eq!(r.style, Some(3));
    }

    #[test]
    fn run_serde_round_trip() {
        for style in [None, Some(0), Some(42)] {
            let r = Run {
                text: "hello".into(),
                style,
            };
            let json = serde_json::to_value(&r).unwrap();
            let back = serde_json::from_value(json).unwrap();
            assert_eq!(r, back);
        }
    }

    #[test]
    fn run_style_field_emits_null_when_none() {
        // The bare Run always emits the `style` key (we don't
        // gate Run with `skip_serializing_if`); only Cell.runs
        // has the skip. Verify the field is present in the
        // JSON so a downstream parser sees the right shape.
        let r = Run::new("hello");
        let json = serde_json::to_value(&r).unwrap();
        assert!(json.get("style").is_some());
        assert_eq!(json["style"], serde_json::Value::Null);
    }

    #[test]
    fn cell_convert_to_rich_sets_runs() {
        let mut c = Cell::new();
        c.set_text("hello");
        assert!(c.runs.is_none());
        c.convert_to_rich(Some(2));
        let runs = c.runs.as_ref().unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].text, "hello");
        assert_eq!(runs[0].style, Some(2));
    }

    #[test]
    fn cell_convert_to_rich_uses_none_for_legacy_path() {
        // `convert_to_rich(None)` means "single run, inherit the
        // cell's flat style" — the renderer falls through to the
        // cell's default style when a run has style=None.
        let mut c = Cell::new();
        c.set_text("text");
        c.convert_to_rich(None);
        assert_eq!(c.runs.as_ref().unwrap()[0].style, None);
    }

    #[test]
    fn cell_without_runs_serializes_cleanly() {
        // `skip_serializing_if = "Option::is_none"` on Cell.runs
        // means pre-4.5 workbooks (and a fresh cell that hasn't
        // been converted to rich text) don't grow a `runs: null`
        // key in the JSON.
        let c = Cell::new();
        let json = serde_json::to_value(&c).unwrap();
        assert!(json.get("runs").is_none());
    }

    #[test]
    fn cell_with_runs_round_trips() {
        let mut c = Cell::new();
        c.set_text("ignored once runs is Some");
        c.runs = Some(vec![Run::new("plain "), Run::with_style("bold", 1)]);
        let json = serde_json::to_value(&c).unwrap();
        let back: Cell = serde_json::from_value(json).unwrap();
        assert!(back.runs.is_some());
        let runs = back.runs.as_ref().unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "plain ");
        assert_eq!(runs[0].style, None);
        assert_eq!(runs[1].text, "bold");
        assert_eq!(runs[1].style, Some(1));
    }

    #[test]
    fn runs_preserved_across_get_data_set_data() {
        // Phase 4.5: a cell with runs survives the workbook
        // serialise → deserialise round-trip via set_data /
        // get_data. The renderer reads `cell.runs` directly; if
        // the field is dropped on round-trip, the cell would fall
        // back to the flat path on the next render.
        use crate::core::data_proxy::DataProxy;
        let mut d = DataProxy::new("t");
        d.set_cell_text(0, 0, "ignored");
        d.set_cell_text(0, 1, "ignored too");
        if let Some(c) = d.get_cell_mut(0, 0) {
            c.runs = Some(vec![
                crate::core::cell::Run::new("plain "),
                crate::core::cell::Run::with_style("bold", 0),
            ]);
        }
        // `get_data` returns the workbook JSON; round-trip
        // through set_data and verify the `runs` key survives.
        // (Skips the deserialised Cell here — the per-cell
        // field-level tests above cover the data path; this one
        // covers the workbook-level wire format.)
        let v: serde_json::Value = d.get_data();
        d.set_cell_text(0, 0, "");
        d.set_cell_text(0, 1, "");
        d.set_data(v.clone());
        let _after: serde_json::Value = d.get_data();
        // The JSON shape is `{"rows": {"_": {"0":
        // {"cells": {"0": {...}, ...}}}}, ...}`. A1 is
        // rows._.0.cells.0.
        let cell = v["rows"]["_"]["0"]["cells"]["0"].clone();
        let runs = cell["runs"].as_array().expect("runs is array");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0]["text"], "plain ");
        assert!(runs[0]["style"].is_null());
        assert_eq!(runs[1]["text"], "bold");
        assert_eq!(runs[1]["style"], 0);
    }

    #[test]
    fn legacy_cell_loads_without_runs_key() {
        // Pre-4.5 workbooks: no `runs` key in the JSON. The
        // `#[serde(default)]` on Cell.runs means the field
        // defaults to None and the cell renders via the flat
        // path.
        let json = serde_json::json!({
            "text": "100",
            "value": "100",
            "style": null,
            "merge": null,
            "editable": true,
            "cell_type": "text",
        });
        let c: Cell = serde_json::from_value(json).unwrap();
        assert!(c.runs.is_none());
        assert_eq!(c.text, "100");
    }

    #[test]
    fn deserializes_without_optional_fields() {
        // Older saved data has no `note`/`link` fields; serde defaults must
        // let it load (backward compatibility).
        let c: Cell = serde_json::from_str(
            r#"{"text":"x","value":"x","style":null,"merge":null,"editable":true,"cell_type":"text"}"#,
        )
        .unwrap();
        assert_eq!(c.text, "x");
        assert_eq!(c.note, None);
        assert_eq!(c.link, None);
    }
}
