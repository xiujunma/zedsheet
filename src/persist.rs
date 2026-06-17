//! Workbook persistence and change notification (issue #20).
//!
//! Edits used to be lost on reload and there was no way for a host app to read
//! or be notified of changes. This module keeps, per mount (keyed by the mount
//! selector):
//!   * the last-serialized workbook JSON, for change de-duplication, and
//!   * an optional JS `on_change` callback.
//!
//! On a genuine change it writes the workbook to `localStorage` and invokes the
//! callback. The public JS API (`get_data`, `load_data`, `on_change`) lives in
//! `lib.rs`; the actual (de)serialization is `core::workbook`.

use std::cell::RefCell;
use std::collections::HashMap;

use wasm_bindgen::JsValue;

/// Per-mount persistence state.
#[derive(Default)]
struct MountState {
    /// Last workbook JSON we persisted/notified — used to suppress no-op syncs
    /// (e.g. pure selection moves, which don't alter the serialized data).
    last_json: Option<String>,
    /// Host-registered change callback, invoked with the workbook JSON.
    on_change: Option<js_sys::Function>,
    /// Persistence is disarmed until the initial mount + restore completes, so
    /// the first sync can't overwrite saved data before it has been read back.
    armed: bool,
}

thread_local! {
    static STATE: RefCell<HashMap<String, MountState>> = RefCell::new(HashMap::new());
}

/// `localStorage` key for a mount selector.
fn storage_key(selector: &str) -> String {
    format!("zedsheet::{selector}")
}

/// The browser `localStorage`, if available (absent in non-browser hosts, and
/// access can fail in private-mode / sandboxed contexts — treated as "no
/// persistence" rather than an error).
fn local_storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

/// The previously-saved workbook snapshot for this mount, if any. Empty strings
/// are treated as absent.
pub fn load_saved(selector: &str) -> Option<String> {
    local_storage()?
        .get_item(&storage_key(selector))
        .ok()
        .flatten()
        .filter(|s| !s.trim().is_empty())
}

fn write_storage(selector: &str, json: &str) {
    if let Some(ls) = local_storage() {
        let _ = ls.set_item(&storage_key(selector), json);
    }
}

/// Register (or clear, with `None`) the host's change callback for a mount.
pub fn set_on_change(selector: &str, cb: Option<js_sys::Function>) {
    STATE.with(|s| {
        s.borrow_mut()
            .entry(selector.to_string())
            .or_default()
            .on_change = cb;
    });
}

/// Seed the de-dup baseline without saving or notifying. Used right after mount
/// + restore so the next genuine edit is what triggers persistence.
pub fn seed_baseline(selector: &str, json: &str) {
    STATE.with(|s| {
        s.borrow_mut()
            .entry(selector.to_string())
            .or_default()
            .last_json = Some(json.to_string());
    });
}

/// Enable persistence for a mount (called once mount + restore has finished).
pub fn arm(selector: &str) {
    STATE.with(|s| {
        s.borrow_mut()
            .entry(selector.to_string())
            .or_default()
            .armed = true;
    });
}

/// Record the current workbook JSON. When armed and the JSON differs from the
/// last recorded value, persist it to `localStorage` and invoke the host's
/// `on_change` callback. Called from the UI sync path on every change.
pub fn note_change(selector: &str, json: &str) {
    // Decide + update state under the borrow, then do I/O and the JS call after
    // releasing it (the callback may re-enter this module). `proceed` is kept
    // separate from the (optional) callback so a change still persists even
    // when no `on_change` handler is registered.
    let (proceed, callback) = STATE.with(|s| {
        let mut map = s.borrow_mut();
        let st = map.entry(selector.to_string()).or_default();
        if !st.armed || st.last_json.as_deref() == Some(json) {
            (false, None)
        } else {
            st.last_json = Some(json.to_string());
            (true, st.on_change.clone())
        }
    });
    if !proceed {
        return;
    }
    write_storage(selector, json);
    if let Some(cb) = callback {
        let _ = cb.call1(&JsValue::NULL, &JsValue::from_str(json));
    }
}
