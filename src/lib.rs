// This crate is a from-scratch port of x-spreadsheet: many structs, helpers,
// and whole modules were ported ahead of being wired into the active UI/engine
// and are filled in feature-by-feature. Allow dead code crate-wide so that
// not-yet-wired scaffolding doesn't drown out real warnings; individual modules
// are removed once confirmed to be superseded rather than pending.
#![allow(dead_code)]

extern crate console_error_panic_hook;
use std::cell::RefCell;
use std::collections::HashMap;
use std::panic;

use wasm_bindgen::prelude::*;
use gloo::utils::document;

mod renderer;
mod component;
mod zedsheet;
mod config;
mod core;
mod formula;

use core::data_proxy::{DataProxy, Style, SheetsRegistry};
use core::cell_range::CellRange;
use component::options::Options;
use zedsheet::ZedSheet;

// Every mounted workbook's sheet registry, keyed by mount selector, exposed so
// JS can toggle read-only on a named sheet (`setSheetReadOnly`, issue #24).
// Keyed (rather than a single Option) so a second `mount()` doesn't clobber the
// first; re-mounting the same selector replaces its entry.
thread_local! {
    static ACTIVE_SHEETS: RefCell<HashMap<String, SheetsRegistry>> =
        RefCell::new(HashMap::new());
}

/// Module init. Installs the panic hook. For the standalone Trunk demo it also
/// auto-mounts sample data into `#zedsheet` if that element is present; in a
/// host app (React, etc.) there is no such element, so nothing auto-mounts and
/// the host calls `mount` explicitly.
#[wasm_bindgen(start)]
pub fn start() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

    if document().query_selector("#zedsheet").ok().flatten().is_some() {
        mount_into("#zedsheet", demo_data());
    }
}

/// Mount a spreadsheet into the element matching `selector`. Optionally seed it
/// with x-spreadsheet-format JSON; pass `undefined`/empty for a blank sheet.
///
/// ```js
/// import init, { mount } from "zedsheet";
/// await init();
/// mount("#my-grid", JSON.stringify(data)); // data optional
/// ```
#[wasm_bindgen]
pub fn mount(selector: &str, data_json: Option<String>) {
    let mut data = DataProxy::new("sheet1");
    if let Some(json) = data_json {
        if !json.trim().is_empty() {
            data.set_data_json(&json);
        }
    }
    mount_into(selector, data);
}

fn mount_into(selector: &str, data: DataProxy) {
    let sheet = ZedSheet::new(selector, Options::default(), data);
    // Stash the registry built inside `ZedSheet::new` so JS callers can
    // toggle read-only by name (issue #24), keyed by selector so multiple
    // mounts coexist.
    if let Some(sheets) = sheet.sheets_registry() {
        ACTIVE_SHEETS.with(|a| {
            a.borrow_mut().insert(selector.to_string(), sheets);
        });
    }
    std::mem::forget(sheet);
}

/// Put the sheet named `name` into read-only mode (`true`) or unlock it
/// (`false`). Applies to every mounted workbook that has a sheet with this
/// name; unknown names are silently ignored (issue #24).
#[wasm_bindgen]
#[allow(non_snake_case)]
pub fn setSheetReadOnly(name: &str, read_only: bool) {
    let upper = name.to_uppercase();
    ACTIVE_SHEETS.with(|a| {
        for sheets in a.borrow().values() {
            for d in sheets.borrow_mut().iter_mut() {
                if d.name.to_uppercase() == upper {
                    d.set_read_only(read_only);
                }
            }
        }
    });
}

/// Read whether a sheet named `name` is in read-only mode (in any mounted
/// workbook). Returns `false` for unknown names (issue #24).
#[wasm_bindgen]
#[allow(non_snake_case)]
pub fn isSheetReadOnly(name: &str) -> bool {
    let upper = name.to_uppercase();
    ACTIVE_SHEETS.with(|a| {
        a.borrow().values().any(|sheets| {
            sheets
                .borrow()
                .iter()
                .any(|d| d.name.to_uppercase() == upper && d.is_read_only())
        })
    })
}

/// Sample data for the standalone demo.
fn demo_data() -> DataProxy {
    let mut data = DataProxy::new("sheet1");

    // Bold, centered, shaded header row.
    let mut header = Style::default();
    header.bold = true;
    header.align = "center".to_string();
    header.bgcolor = Some("#e8eef7".to_string());
    let header_idx = data.add_style(header);

    data.set_cell_text(0, 0, "Name");
    data.set_cell_style(0, 0, header_idx);
    data.set_cell_text(0, 1, "Score");
    data.set_cell_style(0, 1, header_idx);

    data.set_cell_text(1, 0, "Alice");
    data.set_cell_text(1, 1, "100");
    data.set_cell_text(2, 0, "Bob");
    data.set_cell_text(2, 1, "200");

    // A formula cell with a highlight style.
    data.set_cell_text(3, 0, "Total");
    data.set_cell_text(3, 1, "=SUM(B2:B3)");
    let mut hl = Style::default();
    hl.bgcolor = Some("#fff3cd".to_string());
    hl.color = "#9a6700".to_string();
    hl.bold = true;
    let hl_idx = data.add_style(hl);
    data.set_cell_style(3, 1, hl_idx);

    // A currency-formatted column.
    data.set_cell_text(0, 2, "Price");
    data.set_cell_style(0, 2, header_idx);
    let mut usd = Style::default();
    usd.format = "usd".to_string();
    usd.align = "right".to_string();
    let usd_idx = data.add_style(usd);
    data.set_cell_text(1, 2, "1234.5");
    data.set_cell_style(1, 2, usd_idx);
    data.set_cell_text(2, 2, "49.99");
    data.set_cell_style(2, 2, usd_idx);

    // A wrapped long-text cell (row given extra height to show the wrap).
    let mut wrap = Style::default();
    wrap.text_wrap = true;
    wrap.valign = "top".to_string();
    let wrap_idx = data.add_style(wrap);
    data.set_cell_text(5, 0, "This is a long sentence that wraps across multiple lines inside the cell.");
    data.set_cell_style(5, 0, wrap_idx);
    data.set_row_height(5, 60.0);

    // A merged cell spanning D1:E2.
    data.set_cell_text(0, 3, "Merged region");
    let mut mstyle = Style::default();
    mstyle.align = "center".to_string();
    mstyle.valign = "middle".to_string();
    mstyle.bgcolor = Some("#d1e7dd".to_string());
    let m_idx = data.add_style(mstyle);
    data.set_cell_style(0, 3, m_idx);
    data.merges.add(CellRange::new(0, 3, 1, 4));

    data
}
