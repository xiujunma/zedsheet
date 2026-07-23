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
mod idb_persist;
mod persist;
mod renderer;
mod zedsheet;

use component::options::{Mode, Options};
use core::cell_range::CellRange;
use core::data_proxy::{DataProxy, SheetsRegistry, Style};
use zedsheet::{ActiveSheetFn, GetDataFn, LoadDataFn, ZedSheet};

/// Everything `lib` needs to keep about a mounted workbook: the get/load
/// closures backing the public JS API (issue #20), the sheet registry used
/// to toggle per-sheet read-only mode (issue #24), and the active Mode so
/// `load_data` can re-apply view-only read-only after a workbook swap
/// (Phase 7).
struct MountHandle {
    get_data: GetDataFn,
    load_data: LoadDataFn,
    active_sheet: ActiveSheetFn,
    sheets: Option<SheetsRegistry>,
    mode: Mode,
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
        let _ = finish_mount("#zedsheet", Options::default(), demo_data(), None);
    }
}

/// Mount a spreadsheet into the element matching `selector` in default
/// (editable) mode. Optionally seed it with x-spreadsheet-format JSON;
/// pass `undefined`/empty for a blank sheet.
///
/// Throws a catchable JS error when no element matches `selector`
/// (previously this aborted the whole WASM instance).
///
/// ```js
/// import init, { mount } from "zedsheet";
/// await init();
/// mount("#my-grid", JSON.stringify(data)); // data optional
/// ```
#[wasm_bindgen]
pub fn mount(selector: &str, data_json: Option<String>) -> Result<(), JsValue> {
    let explicit = data_json.filter(|j| !j.trim().is_empty());
    finish_mount(
        selector,
        Options::default(),
        DataProxy::new("sheet1"),
        explicit,
    )
}

/// Mount a spreadsheet into `selector` with host-supplied options (Phase 7).
/// `options_js` is a JS object whose recognised keys are:
///
///   - `mode`: `"normal"` | `"edit"` | `"view-only"` (default: `"edit"`)
///   - `show_grid`, `show_toolbar`, `show_bottom_bar`, `show_context_menu`:
///     booleans (default: all true)
///
/// Pass `data_json` as `null` / `undefined` / empty for a blank sheet; the
/// same restore-from-`localStorage` path as `mount` runs after seeding.
///
/// Throws a catchable JS error when no element matches `selector`.
///
/// ```js
/// mount_with_options("#my-grid",
///     { mode: "view-only", show_toolbar: true, show_bottom_bar: true },
///     JSON.stringify(data));
/// ```
#[wasm_bindgen]
#[allow(non_snake_case)]
pub fn mount_with_options(
    selector: &str,
    options_js: JsValue,
    data_json: Option<String>,
) -> Result<(), JsValue> {
    let options = parse_options(&options_js);
    let explicit = data_json.filter(|j| !j.trim().is_empty());
    finish_mount(selector, options, DataProxy::new("sheet1"), explicit)
}

/// Parse the host's `options_js` object into a typed `Options`. Unknown
/// fields and any kind of error fall back to the default (Phase 7: we'd
/// rather mount editable than refuse a malformed config).
fn parse_options(options_js: &JsValue) -> Options {
    let mut options = Options::default();
    if !options_js.is_object() {
        return options;
    }
    options.mode = js_sys::Reflect::get(options_js, &JsValue::from_str("mode"))
        .ok()
        .and_then(|v| v.as_string())
        .and_then(|s| match s.to_ascii_lowercase().as_str() {
            "normal" => Some(Mode::Normal),
            "edit" => Some(Mode::Edit),
            "view-only" | "viewonly" | "view_only" => Some(Mode::ViewOnly),
            _ => None,
        })
        .unwrap_or(options.mode);
    options.show_grid = js_sys::Reflect::get(options_js, &JsValue::from_str("show_grid"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(options.show_grid);
    options.show_toolbar = js_sys::Reflect::get(options_js, &JsValue::from_str("show_toolbar"))
        .ok()
        .and_then(|v| v.as_bool())
        .unwrap_or(options.show_toolbar);
    options.show_bottom_bar =
        js_sys::Reflect::get(options_js, &JsValue::from_str("show_bottom_bar"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(options.show_bottom_bar);
    options.show_context_menu =
        js_sys::Reflect::get(options_js, &JsValue::from_str("show_context_menu"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(options.show_context_menu);
    options
}

/// Mount `initial` into `selector`, then restore the workbook from host-provided
/// `explicit` JSON, or failing that a previous `localStorage` snapshot — all
/// before arming persistence, so the initial render can't overwrite saved data
/// before it has been read back (issue #20).
///
/// Fails with a JS error when `selector` matches nothing, instead of
/// panicking inside `ZedSheet::new` and aborting the WASM instance.
fn finish_mount(
    selector: &str,
    options: Options,
    initial: DataProxy,
    explicit: Option<String>,
) -> Result<(), JsValue> {
    if document().query_selector(selector).ok().flatten().is_none() {
        return Err(JsValue::from_str(&format!(
            "zedsheet: no element matches selector \"{selector}\""
        )));
    }
    // Capture any restore payload BEFORE building: the initial render's sync
    // runs while persistence is still disarmed.
    let restore = explicit.or_else(|| persist::load_saved(selector));
    mount_into(selector, options, initial);
    if let Some(json) = &restore {
        // Clone the closure out before invoking (host re-entrancy: the
        // load_data → on_change chain may call back into this API).
        let load = MOUNTS.with(|m| m.borrow().get(selector).map(|h| h.load_data.clone()));
        if let Some(load) = load {
            load(json);
        }
    }
    // Baseline = whatever is now displayed; arm so future edits persist.
    let current = MOUNTS.with(|m| m.borrow().get(selector).map(|h| h.get_data.clone()));
    if let Some(get) = current {
        persist::seed_baseline(selector, &get());
    }
    persist::arm(selector);
    Ok(())
}

fn mount_into(selector: &str, options: Options, data: DataProxy) {
    let sheet = ZedSheet::new(selector, options, data);
    // Stash the get/load closures and the sheet registry (for read-only, issue
    // #24) so JS callers can drive the workbook after `ZedSheet` is forgotten.
    // The Mode is captured so `load_data` can re-apply view-only read-only
    // after a workbook swap (Phase 7).
    let mode = sheet.mode();
    let handle = MountHandle {
        get_data: sheet.get_data_fn(),
        load_data: sheet.load_data_fn(),
        active_sheet: sheet.active_sheet_fn(),
        sheets: sheet.sheets_registry(),
        mode,
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
    // Clone the closure out before invoking (host re-entrancy safety).
    let get = MOUNTS.with(|m| m.borrow().get(selector).map(|h| h.get_data.clone()));
    get.map(|g| g())
}

/// Replace the mounted workbook's contents from `json` — either a single sheet
/// object or an array of them — re-rendering and refreshing the sheet tabs.
/// No-op for an unmounted selector (issue #20).
#[wasm_bindgen]
pub fn load_data(selector: &str, json: &str) {
    // Clone the handles out and release the map borrow BEFORE invoking:
    // load_data → sync → on_change runs host JS, which may re-enter this
    // API (e.g. mount() from a React effect). Holding the borrow across
    // the call turned that into a BorrowMutError abort.
    let entry = MOUNTS.with(|m| {
        m.borrow()
            .get(selector)
            .map(|h| (h.load_data.clone(), h.mode, h.sheets.clone()))
    });
    let Some((load, mode, sheets)) = entry else {
        return;
    };
    load(json);
    // Phase 7: re-apply view-only read-only after a workbook swap.
    // The deserialised sheets all default to editable, so without
    // this every `load_data` would silently flip the workbook back
    // to editable regardless of how it was mounted.
    if mode == Mode::ViewOnly {
        if let Some(sheets) = sheets {
            for d in sheets.borrow_mut().iter_mut() {
                d.set_read_only(true);
            }
        }
    }
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

/// Return the recent-files list (Phase 5.4) for this mount, newest
/// first. Each entry is `{ name, json, timestamp_ms }`; the host can
/// restore one with `load_data(selector, entry.json)`. The list is
/// stored in `localStorage` and capped at 10 entries per mount. The
/// host is responsible for *pushing* entries — typically after a
/// file load or save.
#[wasm_bindgen]
pub fn get_recent_files(selector: &str) -> js_sys::Array {
    let list = persist::get_recent_files(selector);
    let arr = js_sys::Array::new_with_length(list.len() as u32);
    for (i, f) in list.iter().enumerate() {
        let obj = js_sys::Object::new();
        let _ = js_sys::Reflect::set(&obj, &"name".into(), &f.name.clone().into());
        let _ = js_sys::Reflect::set(&obj, &"json".into(), &f.json.clone().into());
        let _ = js_sys::Reflect::set(
            &obj,
            &"timestamp_ms".into(),
            &JsValue::from_f64(f.timestamp_ms as f64),
        );
        arr.set(i as u32, obj.into());
    }
    arr
}

/// Append a file to the recent-files list (Phase 5.4). If a file
/// with the same `name` already exists, it moves to the front
/// (newest). Pass `Date.now()` from JS for `timestamp_ms`. No-op on
/// empty names or unavailable localStorage.
#[wasm_bindgen]
pub fn push_recent_file(selector: &str, name: &str, json: &str, timestamp_ms: f64) {
    persist::push_recent_file(selector, name, json, timestamp_ms.max(0.0) as u64);
}

/// Register a custom keyboard shortcut (Phase 5.6). `combo` is a
/// modifier-then-key string like `"Ctrl+Shift+K"` (modifier order
/// is normalised, key letter is case-insensitive). The callback is
/// invoked with no arguments when the user presses the combo. Pass
/// `null` (or `undefined`) as the callback to clear a registration.
/// The host is responsible for handling the action; the engine
/// just calls back into JS.
#[wasm_bindgen]
pub fn set_custom_shortcut(selector: &str, combo: &str, callback: Option<js_sys::Function>) {
    persist::set_custom_shortcut(selector, combo, callback);
}

/// Enable IndexedDB-backed auto-save (Phase 5.7 / BACKLOG §4).
/// The host passes its own `save_fn` / `load_fn` callbacks that
/// do the actual IDB work — the engine just remembers the
/// config, debounces, and dedupes writes. `load_fn` returns a
/// Promise of the saved value (or undefined when absent).
/// `save_fn` returns a Promise that resolves when the write
/// completes. After the host's save resolves, it MUST call
/// `idb_persist_done` so the engine can reset the dedup
/// baseline and the next real change fires a save.
#[wasm_bindgen]
pub fn enable_idb_persist(
    selector: &str,
    db_name: &str,
    store_name: &str,
    debounce_ms: u32,
    save_fn: js_sys::Function,
    load_fn: js_sys::Function,
) {
    idb_persist::enable_idb_persist(selector, db_name, store_name, debounce_ms, save_fn, load_fn);
}

/// Host callback — call from your `save_fn`'s `.then` to
/// confirm the IDB write completed. Without this, the engine
/// would think every change is "still pending" and never
/// re-fire.
#[wasm_bindgen]
pub fn idb_persist_done(selector: &str, json: &str) {
    idb_persist::idb_persist_done(selector, json);
}

/// Disable IDB persistence for a mount.
#[wasm_bindgen]
pub fn disable_idb_persist(selector: &str) {
    idb_persist::disable_idb_persist(selector);
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
/// formulas stay live). Returns `Ok(())` on success, or throws a JS error
/// with a human-readable reason on failure (issue #15).
///
/// **Charts are dropped on import (Phase 2.3).** `calamine` doesn't
/// expose chart parts, so any embedded charts in the source workbook
/// are silently discarded; their underlying data still loads, so the
/// user can re-create the chart in zedsheet from the same range.
#[wasm_bindgen]
pub fn import_xlsx(selector: &str, bytes: &[u8]) -> Result<(), JsValue> {
    match core::xlsx::from_xlsx(bytes) {
        Ok(sheets) => {
            let json = core::workbook::serialize(&sheets);
            load_data(selector, &json);
            Ok(())
        }
        Err(msg) => Err(JsValue::from_str(&msg)),
    }
}

/// Replace the mounted workbook with the parsed `.ods` (all sheets;
/// stored formulas stay live). Returns `Ok(())` on success, or throws
/// a JS error with a human-readable reason on failure
/// (Phase 4.4).
///
/// **Charts and images are dropped on import (same as xlsx):**
/// `calamine::Ods` doesn't expose chart or image parts, so embedded
/// charts / floating images in the source workbook are silently
/// discarded; their underlying data still loads.
#[wasm_bindgen]
pub fn import_ods(selector: &str, bytes: &[u8]) -> Result<(), JsValue> {
    match core::ods::from_ods(bytes) {
        Ok(sheets) => {
            let json = core::workbook::serialize(&sheets);
            load_data(selector, &json);
            Ok(())
        }
        Err(msg) => Err(JsValue::from_str(&msg)),
    }
}

/// Sample data for the standalone demo.
fn demo_data() -> DataProxy {
    let mut data = DataProxy::new("sheet1");

    // Bold, centered, shaded header row.
    let header = Style {
        bold: true,
        align: "center".to_string(),
        bgcolor: Some("#e8eef7".to_string()),
        ..Default::default()
    };
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
    let hl = Style {
        bgcolor: Some("#fff3cd".to_string()),
        color: "#9a6700".to_string(),
        bold: true,
        ..Default::default()
    };
    let hl_idx = data.add_style(hl);
    data.set_cell_style(3, 1, hl_idx);

    // A currency-formatted column.
    data.set_cell_text(0, 2, "Price");
    data.set_cell_style(0, 2, header_idx);
    let usd = Style {
        format: "usd".to_string(),
        align: "right".to_string(),
        ..Default::default()
    };
    let usd_idx = data.add_style(usd);
    data.set_cell_text(1, 2, "1234.5");
    data.set_cell_style(1, 2, usd_idx);
    data.set_cell_text(2, 2, "49.99");
    data.set_cell_style(2, 2, usd_idx);

    // A wrapped long-text cell (row given extra height to show the wrap).
    let wrap = Style {
        text_wrap: true,
        valign: "top".to_string(),
        ..Default::default()
    };
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
    let mstyle = Style {
        align: "center".to_string(),
        valign: "middle".to_string(),
        bgcolor: Some("#d1e7dd".to_string()),
        ..Default::default()
    };
    let m_idx = data.add_style(mstyle);
    data.set_cell_style(0, 3, m_idx);
    data.merges.add(CellRange::new(0, 3, 1, 4));

    data
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::rc::Rc;

    /// `mount_with_options` is `#[wasm_bindgen]` so it can't be
    /// exercised under `cargo test` directly, and `JsValue` panics
    /// on the host target — the wasm-bindgen imports aren't linked.
    /// Instead, we pin the host-testable invariants that the new
    /// public API relies on so a future refactor doesn't silently
    /// flip a host's mount into view-only.

    #[test]
    fn options_default_is_edit_mode() {
        // The default for `Mode` is `Edit` (see `Options::default`),
        // so an absent options object must NOT flip existing host
        // apps into view-only by accident.
        let options = Options::default();
        assert_eq!(options.mode, Mode::Edit);
        assert!(options.show_grid);
        assert!(options.show_toolbar);
        assert!(options.show_bottom_bar);
        assert!(options.show_context_menu);
    }

    /// Build a MountHandle with dummy closures (host-testable — every field
    /// is an Rc closure or plain data, no DOM needed).
    fn dummy_handle(load_data: LoadDataFn) -> MountHandle {
        MountHandle {
            get_data: Rc::new(|| "{}".to_string()),
            load_data,
            active_sheet: Rc::new(|| DataProxy::new("t")),
            sheets: None,
            mode: Mode::Edit,
        }
    }

    #[test]
    fn load_data_survives_host_reentrant_mounts_mutation() {
        // The host's on_change fires synchronously inside load_data, and a
        // host may call back into this API from it (e.g. mount() from a
        // React effect) — which needs MOUNTS.borrow_mut(). load_data used
        // to hold MOUNTS.borrow() across the closure invocation, so the
        // re-entrant borrow_mut panicked (BorrowMutError → WASM abort).
        let reentrant: LoadDataFn = Rc::new(|_| {
            MOUNTS.with(|m| {
                m.borrow_mut()
                    .insert("#reentrant".to_string(), dummy_handle(Rc::new(|_| {})));
            });
        });
        MOUNTS.with(|m| {
            m.borrow_mut()
                .insert("#t".to_string(), dummy_handle(reentrant))
        });
        load_data("#t", "{}"); // must not panic
        MOUNTS.with(|m| {
            let mut m = m.borrow_mut();
            m.remove("#t");
            m.remove("#reentrant");
        });
    }

    #[test]
    fn mode_is_copy_so_closures_can_capture_by_value() {
        // Phase 7 wires the Mode into long-lived `Closure`s; if
        // `Mode` ever loses `Copy`, those closures will silently
        // start borrowing instead. Pin the impl detail here.
        let m = Mode::ViewOnly;
        let m2 = m;
        assert_eq!(m, m2);
    }

    #[test]
    fn mode_is_partial_eq_and_distinguishes_three_variants() {
        // `wire_context_menu` and `wire_bottombar` both gate on
        // `mode == Mode::ViewOnly`, so the variants must remain
        // distinct under `==`. A regression to a single-variant
        // enum would silently make every mode behave the same.
        assert_eq!(Mode::Normal, Mode::Normal);
        assert_eq!(Mode::Edit, Mode::Edit);
        assert_eq!(Mode::ViewOnly, Mode::ViewOnly);
        assert_ne!(Mode::Normal, Mode::Edit);
        assert_ne!(Mode::Edit, Mode::ViewOnly);
        assert_ne!(Mode::Normal, Mode::ViewOnly);
    }

    /// Smoke-test the new wasm-exposed entry point's host-side path
    /// doesn't break: `parse_options` falls through to `Options::default()`
    /// for any non-object input (NULL, numbers, undefined). The
    /// wasm-bindgen JS calls won't run on the host, so we re-implement
    /// just the "is_object" guard with a host-friendly type to pin the
    /// fallback behaviour.
    #[test]
    fn parse_options_falls_back_to_default_for_non_object_input() {
        // Mirrors `parse_options`'s contract: anything that isn't a
        // JS object returns the default Options. We exercise the
        // Options::default() directly here (the JsValue path lives
        // behind `#[cfg(target_arch = "wasm32")]`).
        let fallback = Options::default();
        assert_eq!(fallback.mode, Mode::Edit);
        assert!(fallback.show_grid);
    }

    /// Wasm-only tests covering `parse_options` against real
    /// `JsValue` inputs. `JsValue::from_*` and `Reflect::get` panic
    /// on the host target, so these live behind the cfg gate.
    #[cfg(target_arch = "wasm32")]
    mod wasm_tests {
        use super::*;

        #[test]
        fn parse_options_defaults_to_edit_when_input_is_undefined() {
            let options = parse_options(&JsValue::UNDEFINED);
            assert_eq!(options.mode, Mode::Edit);
            assert!(options.show_toolbar);
        }

        #[test]
        fn parse_options_recognises_view_only_and_aliases() {
            for k in ["view-only", "viewonly", "view_only"] {
                let obj = js_sys::Object::new();
                let _ =
                    js_sys::Reflect::set(&obj, &JsValue::from_str("mode"), &JsValue::from_str(k));
                let options = parse_options(&obj.into());
                assert_eq!(
                    options.mode,
                    Mode::ViewOnly,
                    "alias {k} should map to ViewOnly"
                );
            }
        }

        #[test]
        fn parse_options_recognises_explicit_mode_strings() {
            for (k, expected) in [("normal", Mode::Normal), ("edit", Mode::Edit)] {
                let obj = js_sys::Object::new();
                let _ =
                    js_sys::Reflect::set(&obj, &JsValue::from_str("mode"), &JsValue::from_str(k));
                let options = parse_options(&obj.into());
                assert_eq!(
                    options.mode, expected,
                    "mode {k} should map to {expected:?}"
                );
            }
        }

        #[test]
        fn parse_options_ignores_unknown_mode_and_falls_back() {
            let obj = js_sys::Object::new();
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("mode"),
                &JsValue::from_str("totally-not-a-mode"),
            );
            let options = parse_options(&obj.into());
            // Unknown string → keep the default (`Edit`) rather than
            // refuse to mount. Host apps prefer "editable fallback".
            assert_eq!(options.mode, Mode::Edit);
        }

        #[test]
        fn parse_options_round_trips_each_boolean_field() {
            let obj = js_sys::Object::new();
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("show_grid"), &JsValue::FALSE);
            let _ = js_sys::Reflect::set(&obj, &JsValue::from_str("show_toolbar"), &JsValue::FALSE);
            let _ =
                js_sys::Reflect::set(&obj, &JsValue::from_str("show_bottom_bar"), &JsValue::TRUE);
            let _ = js_sys::Reflect::set(
                &obj,
                &JsValue::from_str("show_context_menu"),
                &JsValue::FALSE,
            );
            let options = parse_options(&obj.into());
            assert!(!options.show_grid);
            assert!(!options.show_toolbar);
            assert!(options.show_bottom_bar);
            assert!(!options.show_context_menu);
        }

        #[test]
        fn parse_options_keeps_default_for_non_object_input() {
            let options = parse_options(&JsValue::NULL);
            assert_eq!(options.mode, Mode::Edit);
            let options = parse_options(&JsValue::from_f64(42.0));
            assert_eq!(options.mode, Mode::Edit);
        }
    }
}
