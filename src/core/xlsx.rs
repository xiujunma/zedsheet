//! XLSX import/export (issue #15): write via `rust_xlsxwriter`, read via
//! `calamine` — both pure Rust, so they work on wasm32 too. Values and
//! formulas survive; cell styling is out of scope for this round-trip.

use std::io::Cursor;

use calamine::{Data, Reader, Xlsx};
use rust_xlsxwriter::Workbook;

use crate::core::data_proxy::DataProxy;

/// Serialize every sheet to an `.xlsx` workbook. Numbers are written as
/// numbers, formulas as formulas (Excel recomputes them on open), everything
/// else as strings.
pub fn to_xlsx(sheets: &[DataProxy]) -> Result<Vec<u8>, String> {
    let mut wb = Workbook::new();
    for sheet in sheets {
        let ws = wb.add_worksheet();
        ws.set_name(&sheet.name).map_err(|e| e.to_string())?;
        let Some((max_r, max_c)) = sheet.used_extent() else {
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
}
