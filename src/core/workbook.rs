//! Workbook-level (multi-sheet) serialization for persistence (issue #20).
//!
//! A workbook is serialized as a JSON array of x-spreadsheet sheet objects —
//! the same per-sheet shape [`DataProxy::get_data`] already produces. Keeping
//! the pure (de)serialization here, independent of the renderer and the DOM,
//! makes it host-testable; the UI wiring in `zedsheet` and the JS API in `lib`
//! build on top of it.

use crate::core::data_proxy::DataProxy;

/// Serialize every sheet, in workbook order, to a JSON array string.
pub fn serialize(sheets: &[DataProxy]) -> String {
    let arr: Vec<serde_json::Value> = sheets.iter().map(|s| s.get_data()).collect();
    serde_json::to_string(&serde_json::Value::Array(arr)).unwrap_or_else(|_| "[]".to_string())
}

/// Parse a workbook JSON string into sheets.
///
/// Accepts either a JSON array of sheet objects (a workbook) or a single sheet
/// object (the legacy single-sheet `mount` payload). Always returns at least
/// one sheet: malformed or empty input yields a single blank `sheet1`, so
/// callers never have to handle an empty workbook.
pub fn deserialize(json: &str) -> Vec<DataProxy> {
    let parsed: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return vec![DataProxy::new("sheet1")],
    };
    let objs = match parsed {
        serde_json::Value::Array(a) => a,
        obj @ serde_json::Value::Object(_) => vec![obj],
        _ => return vec![DataProxy::new("sheet1")],
    };
    let mut out: Vec<DataProxy> = Vec::with_capacity(objs.len());
    for (i, obj) in objs.into_iter().enumerate() {
        // `set_data` already adopts the embedded "name"; seed a sensible default
        // for sheets that omit it so tabs never render blank.
        let name = obj
            .get("name")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("sheet{}", i + 1));
        let mut d = DataProxy::new(&name);
        d.set_data(obj);
        out.push(d);
    }
    if out.is_empty() {
        out.push(DataProxy::new("sheet1"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workbook_roundtrip_preserves_sheets_and_content() {
        let mut s1 = DataProxy::new("Alpha");
        s1.set_cell_text(0, 0, "hello");
        s1.set_freeze(1, 0);
        let mut s2 = DataProxy::new("Beta");
        s2.set_cell_text(2, 3, "world");

        let json = serialize(&[s1, s2]);
        let sheets = deserialize(&json);

        assert_eq!(sheets.len(), 2);
        assert_eq!(sheets[0].name, "Alpha");
        assert_eq!(sheets[1].name, "Beta");
        assert_eq!(sheets[0].get_cell_text(0, 0), "hello");
        assert_eq!(sheets[0].freeze, (1, 0));
        assert_eq!(sheets[1].get_cell_text(2, 3), "world");
    }

    #[test]
    fn deserialize_accepts_single_sheet_object() {
        // The legacy `mount` payload is one sheet object, not an array.
        let mut s = DataProxy::new("Solo");
        s.set_cell_text(0, 0, "x");
        let single = s.get_data_json();

        let sheets = deserialize(&single);

        assert_eq!(sheets.len(), 1);
        assert_eq!(sheets[0].name, "Solo");
        assert_eq!(sheets[0].get_cell_text(0, 0), "x");
    }

    #[test]
    fn deserialize_malformed_or_empty_yields_one_blank_sheet() {
        assert_eq!(deserialize("not json").len(), 1);
        assert_eq!(deserialize("[]").len(), 1);
        assert_eq!(deserialize("").len(), 1);
    }
}
