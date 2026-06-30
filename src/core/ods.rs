//! ODS (OpenDocument Spreadsheet) import (Phase 4.4).
//!
//! Minimal-viable path: values + formulas. Cell styling,
//! column widths, row heights, and defined names are deferred —
//! ODS is "optional but common" per PLAN.md, and a 1-week project
//! per its file-format surface would dwarf the value of styling
//! parity alone.
//!
//! `calamine::Ods` is built into the same crate as the XLSX reader
//! (already a dependency), so no new crate is needed. The
//! `Read + Seek` bound is satisfied by `Cursor<&[u8]>` on both
//! native and wasm targets.

use std::collections::HashMap;
use std::io::Cursor;

use calamine::{Data, Ods, Reader};

use crate::core::data_proxy::DataProxy;

/// Convert a `.ods` byte slice into a list of sheets (one
/// `DataProxy` per sheet). Formulas are preserved with the leading
/// `=` so the engine re-evaluates them on first commit. Errors
/// from `calamine` are returned as a string for the JS surface.
pub(crate) fn from_ods(bytes: &[u8]) -> Result<Vec<DataProxy>, String> {
    let mut wb = Ods::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let names = wb.sheet_names();
    let mut out = Vec::with_capacity(names.len());
    for name in names {
        let range = wb.worksheet_range(&name).map_err(|e| e.to_string())?;
        // Formulas come from a separate range. calamine 0.26
        // doesn't have `Data::Formula`; the formula text is on a
        // parallel `worksheet_formula` call. We collect into a
        // `HashMap` and override the cached value with the formula
        // text (prefixed with `=` so the engine re-evaluates on
        // commit). Missing or empty formulas are skipped.
        let formulas: HashMap<(usize, usize), String> = wb
            .worksheet_formula(&name)
            .ok()
            .map(|fr| {
                fr.cells()
                    .filter(|(_, _, f)| !f.is_empty())
                    .map(|(r, c, f)| ((r, c), format!("={}", f)))
                    .collect()
            })
            .unwrap_or_default();
        let mut sheet = DataProxy::new(&name);
        for (row, col, data) in range.cells() {
            // Formula text wins over the cached value when present.
            if let Some(formula) = formulas.get(&(row, col)) {
                sheet.set_cell_text(row, col, formula);
                continue;
            }
            let text = match data {
                Data::Empty => continue,
                Data::String(s) => s.clone(),
                Data::Float(f) => format_number(*f),
                Data::Int(i) => i.to_string(),
                Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
                Data::DateTime(dt) => format_number(dt.as_f64()),
                Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
                Data::Error(e) => format!("{}", e),
                // Formula cells: the cached value lives in the
                // outer \`Data\` variant. Without a formula handle
                // we can't recover the formula text from the
                // current read, so skip the cell — the user can
                // re-enter the formula. (The same trade-off the
                // xlsx import makes.)
            };
            sheet.set_cell_text(row, col, &text);
        }
        out.push(sheet);
    }
    Ok(out)
}

/// `f64` → "12" / "12.5" matching the rest of the engine.
fn format_number(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

/// Pure mapping helper for tests: convert a single \`Data\`
/// value to the text that the engine's \`set_cell_text\` expects.
/// Mirrors the in-loop conversion in \`from_ods\`.
pub(crate) fn data_to_text(d: &Data) -> Option<String> {
    Some(match d {
        Data::Empty => return None,
        Data::String(s) => s.clone(),
        Data::Float(f) => format_number(*f),
        Data::Int(i) => i.to_string(),
        Data::Bool(b) => if *b { "TRUE" } else { "FALSE" }.to_string(),
        Data::DateTime(dt) => format_number(dt.as_f64()),
        Data::DateTimeIso(s) | Data::DurationIso(s) => s.clone(),
        Data::Error(e) => format!("{}", e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_to_text_handles_each_variant() {
        // Each Data variant should map to the engine's expected
        // text form. Formulas and Empty are skipped (return None).
        assert_eq!(data_to_text(&Data::Empty), None);
        assert_eq!(
            data_to_text(&Data::String("hello".into())),
            Some("hello".into())
        );
        assert_eq!(data_to_text(&Data::Float(12.0)), Some("12".into()));
        assert_eq!(data_to_text(&Data::Float(12.5)), Some("12.5".into()));
        assert_eq!(data_to_text(&Data::Int(-42)), Some("-42".into()));
        assert_eq!(data_to_text(&Data::Bool(true)), Some("TRUE".into()));
        assert_eq!(data_to_text(&Data::Bool(false)), Some("FALSE".into()));
        // Formula variant: skip (we can't recover the formula text
        // from the data alone in the current calamine API).
    }

    #[test]
    fn format_number_strips_integer_fractional_zero() {
        // Pin the formatting so a future tweak forces an explicit
        // test update — the engine reads back the same string the
        // import writes, and changing the format would silently
        // break that round-trip.
        assert_eq!(format_number(0.0), "0");
        assert_eq!(format_number(7.0), "7");
        assert_eq!(format_number(-7.0), "-7");
        assert_eq!(format_number(1.5), "1.5");
        assert_eq!(format_number(0.1 + 0.2), "0.30000000000000004");
    }

    #[test]
    fn from_ods_rejects_garbage_bytes() {
        // \`Ods::new\` validates the \`mimetype\` first; a non-zip
        // blob is rejected as \`InvalidMime\` which we surface as
        // an \`Err\`. This is the same shape the xlsx import uses.
        assert!(from_ods(b"not a zip file").is_err());
        assert!(from_ods(b"").is_err());
    }

    #[test]
    fn from_ods_handles_zip_without_mimetype() {
        // A zip that doesn't contain a \`mimetype\` member is
        // rejected as \`FileNotFound("mimetype")\`. Some non-spreadsheet
        // zips (e.g. docx) still report as an invalid mime type
        // since their \`mimetype\` doesn't match the spreadsheet
        // magic. We accept both as "this is not a spreadsheet" and
        // surface the error to the JS caller.
        // Minimal docx-style zip header bytes:
        // 4-byte local-file-header signature, then "PK\x03\x04".
        // We use a real PK-magic but no \`mimetype\` member; the
        // reader should error on \`mimetype\` not found.
        let mut zip_bytes: Vec<u8> = Vec::new();
        zip_bytes.extend_from_slice(b"PK\x03\x04");
        zip_bytes.extend_from_slice(&[0u8; 26]); // rest of LFH
        assert!(from_ods(&zip_bytes).is_err());
    }
}
