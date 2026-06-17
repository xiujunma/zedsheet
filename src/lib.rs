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

use gloo::utils::document;
use wasm_bindgen::prelude::*;

mod component;
mod config;
pub mod core;
mod formula;
mod persist;
mod renderer;
mod zedsheet;

use component::options::Options;
use core::cell_range::CellRange;
use core::data_proxy::{DataProxy, SheetsRegistry, Style};
use zedsheet::{ActiveSheetFn, GetDataFn, LoadDataFn, ZedSheet};

/// Everything `lib` needs to keep about a mounted workbook: the get/load
/// closures backing the public JS API (issue #20) and the sheet registry used
/// to toggle per-sheet read-only mode (issue #24).
struct MountHandle {
    get_data: GetDataFn,
    load_data: LoadDataFn,
    active_sheet: ActiveSheetFn,
    sheets: Option<SheetsRegistry>,
}

// Every mounted workbook, keyed by mount selector. Keyed (rather than a single
// Option) so a second `mount()` doesn't clobber the first; re-mounting the same
// selector replaces its entry.
thread_local! {
    static MOUNTS: RefCell<HashMap<String, MountHandle>> = RefCell::new(HashMap::new());
}

/// Module init. Installs the panic hook. For the standalone Trunk demo it also
/// auto-mounts sample data into `#zedsheet` if that element is present; in a
/// host app (React, etc.) there is no such element, so nothing auto-mounts and
/// the host calls `mount` explicitly.
#[wasm_bindgen(start)]
pub fn start() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

    if document()
        .query_selector("#zedsheet")
        .ok()
        .flatten()
        .is_some()
    {
        // Demo: restore the user's saved edits if present, else seed the sample.
        finish_mount("#zedsheet", demo_data(), None);
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
    let explicit = data_json.filter(|j| !j.trim().is_empty());
    finish_mount(selector, DataProxy::new("sheet1"), explicit);
}

/// Mount `initial` into `selector`, then restore the workbook from host-provided
/// `explicit` JSON, or failing that a previous `localStorage` snapshot — all
/// before arming persistence, so the initial render can't overwrite saved data
/// before it has been read back (issue #20).
fn finish_mount(selector: &str, initial: DataProxy, explicit: Option<String>) {
    // Capture any restore payload BEFORE building: the initial render's sync
    // runs while persistence is still disarmed.
    let restore = explicit.or_else(|| persist::load_saved(selector));
    mount_into(selector, initial);
    if let Some(json) = &restore {
        MOUNTS.with(|m| {
            if let Some(h) = m.borrow().get(selector) {
                (h.load_data)(json);
            }
        });
    }
    // Baseline = whatever is now displayed; arm so future edits persist.
    if let Some(current) = MOUNTS.with(|m| m.borrow().get(selector).map(|h| (h.get_data)())) {
        persist::seed_baseline(selector, &current);
    }
    persist::arm(selector);
}

fn mount_into(selector: &str, data: DataProxy) {
    let sheet = ZedSheet::new(selector, Options::default(), data);
    // Stash the get/load closures and the sheet registry (for read-only, issue
    // #24) so JS callers can drive the workbook after `ZedSheet` is forgotten.
    let handle = MountHandle {
        get_data: sheet.get_data_fn(),
        load_data: sheet.load_data_fn(),
        active_sheet: sheet.active_sheet_fn(),
        sheets: sheet.sheets_registry(),
    };
    MOUNTS.with(|m| {
        m.borrow_mut().insert(selector.to_string(), handle);
    });
    std::mem::forget(sheet);
}

/// Put the sheet named `name` into read-only mode (`true`) or unlock it
/// (`false`). Applies to every mounted workbook that has a sheet with this
/// name; unknown names are silently ignored (issue #24).
#[wasm_bindgen]
#[allow(non_snake_case)]
pub fn setSheetReadOnly(name: &str, read_only: bool) {
    let upper = name.to_uppercase();
    MOUNTS.with(|m| {
        for h in m.borrow().values() {
            if let Some(sheets) = &h.sheets {
                for d in sheets.borrow_mut().iter_mut() {
                    if d.name.to_uppercase() == upper {
                        d.set_read_only(read_only);
                    }
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
    MOUNTS.with(|m| {
        m.borrow().values().any(|h| {
            h.sheets.as_ref().is_some_and(|sheets| {
                sheets
                    .borrow()
                    .iter()
                    .any(|d| d.name.to_uppercase() == upper && d.is_read_only())
            })
        })
    })
}

/// Serialize the mounted workbook (every sheet) as an x-spreadsheet JSON array
/// string. Returns `None` for an unmounted selector (issue #20).
///
/// ```js
/// const json = get_data("#my-grid");
/// localStorage.setItem("backup", json);   // or POST it to a server
/// ```
#[wasm_bindgen]
pub fn get_data(selector: &str) -> Option<String> {
    MOUNTS.with(|m| m.borrow().get(selector).map(|h| (h.get_data)()))
}

/// Replace the mounted workbook's contents from `json` — either a single sheet
/// object or an array of them — re-rendering and refreshing the sheet tabs.
/// No-op for an unmounted selector (issue #20).
#[wasm_bindgen]
pub fn load_data(selector: &str, json: &str) {
    MOUNTS.with(|m| {
        if let Some(h) = m.borrow().get(selector) {
            (h.load_data)(json);
        }
    });
}

/// Register a callback invoked with the workbook JSON whenever the data changes
/// — edits, formatting, and structural changes, but not selection moves. Passing
/// a new callback replaces the previous one (issue #20).
///
/// ```js
/// on_change("#my-grid", (json) => console.log("changed", json.length));
/// ```
#[wasm_bindgen]
pub fn on_change(selector: &str, callback: js_sys::Function) {
    persist::set_on_change(selector, Some(callback));
}

/// Export the mounted workbook's ACTIVE sheet as CSV (CSV is single-sheet).
/// Formula cells export their computed values. `None` for an unmounted
/// selector (issue #15).
#[wasm_bindgen]
pub fn export_csv(selector: &str) -> Option<String> {
    MOUNTS.with(|m| {
        m.borrow()
            .get(selector)
            .map(|h| core::csv::to_csv(&(h.active_sheet)()))
    })
}

/// Replace the mounted workbook with the parsed CSV as a single sheet —
/// "opening" semantics, like Excel opening a .csv file (issue #15).
#[wasm_bindgen]
pub fn import_csv(selector: &str, text: &str) {
    let sheet = core::csv::from_csv("sheet1", text);
    let json = core::workbook::serialize(&[sheet]);
    load_data(selector, &json);
}

/// Export the whole mounted workbook (every sheet, values + live formulas)
/// as `.xlsx` bytes, ready for a Blob download. `None` for an unmounted
/// selector or a write failure (issue #15).
#[wasm_bindgen]
pub fn export_xlsx(selector: &str) -> Option<Vec<u8>> {
    MOUNTS.with(|m| {
        m.borrow().get(selector).and_then(|h| {
            let sheets = core::workbook::deserialize(&(h.get_data)());
            core::xlsx::to_xlsx(&sheets).ok()
        })
    })
}

/// Replace the mounted workbook with the parsed `.xlsx` (all sheets; stored
/// formulas stay live). Returns `false` when the bytes don't parse
/// (issue #15).
#[wasm_bindgen]
pub fn import_xlsx(selector: &str, bytes: &[u8]) -> bool {
    match core::xlsx::from_xlsx(bytes) {
        Ok(sheets) => {
            let json = core::workbook::serialize(&sheets);
            load_data(selector, &json);
            true
        }
        Err(_) => false,
    }
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
    data.set_cell_text(
        5,
        0,
        "This is a long sentence that wraps across multiple lines inside the cell.",
    );
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
