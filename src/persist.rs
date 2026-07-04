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

use serde::{Deserialize, Serialize};
use wasm_bindgen::JsValue;

/// Per-mount persistence state.
#[derive(Default)]
struct MountState {
    /// Last workbook JSON we persisted/notified — used to suppress no-op syncs
    /// (e.g. pure selection moves, which don't alter the serialized data).
    last_json: Option<String>,
    /// Host-registered change callback, invoked with the workbook JSON.
    on_change: Option<js_sys::Function>,
    /// Host-registered custom keyboard shortcuts. The key is a
    /// normalised combo like "Ctrl+Shift+K" (case-insensitive on
    /// the key letter); the value is the JS callback to fire.
    custom_shortcuts: HashMap<String, js_sys::Function>,
    /// Persistence is disarmed until the initial mount + restore completes, so
    /// the first sync can't overwrite saved data before it has been read back.
    armed: bool,
}

/// Normalise a key-combo string for the registry: upper-case the
/// modifier list and the letter, trim whitespace, and join with
/// `+`. The host can pass `"ctrl+k"`, `"Ctrl+K"`, or
/// `"ctrl + K"` and they all collide on the same registry entry.
fn normalise_combo(combo: &str) -> String {
    let mut parts: Vec<String> = combo
        .split('+')
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();
    // Stable order: modifiers first (Ctrl, Alt, Shift, Meta) then the key.
    let order = |s: &str| match s.to_ascii_lowercase().as_str() {
        "ctrl" | "control" | "cmd" | "meta" => 0,
        "alt" | "option" => 1,
        "shift" => 2,
        _ => 3,
    };
    parts.sort_by_key(|p| order(p));
    parts
        .iter()
        .map(|p| {
            let lower = p.to_ascii_lowercase();
            if order(p) == 3 {
                // The actual key letter: keep the original case so
                // the host can distinguish Shift+1 from 1 if they
                // need to. The lookup is case-insensitive at the
                // call site, so "Ctrl+K" and "Ctrl+k" both match.
                p.clone()
            } else {
                lower
            }
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// Register (or clear, with `None`) a custom keyboard shortcut for
/// this mount (Phase 5.6). `combo` is matched case-insensitively on
/// the key letter; the modifier order is normalised (Ctrl, Alt,
/// Shift, Meta) so two equivalent strings collide on one entry.
pub fn set_custom_shortcut(selector: &str, combo: &str, cb: Option<js_sys::Function>) {
    let key = normalise_combo(combo);
    let selector_key = selector.to_string();
    STATE.with(|s| {
        let mut map = s.borrow_mut();
        let entry = map.entry(selector_key).or_default();
        match cb {
            Some(f) => {
                entry.custom_shortcuts.insert(key.clone(), f);
            }
            None => {
                entry.custom_shortcuts.remove(&key);
            }
        }
    });
}

/// Look up a custom shortcut for the given combo. Returns the JS
/// callback if the host registered one, otherwise `None`. Used by
/// the keyboard handler to short-circuit before the built-in
/// bindings.
pub fn get_custom_shortcut(selector: &str, combo: &str) -> Option<js_sys::Function> {
    let key = normalise_combo(combo);
    STATE.with(|s| {
        s.borrow()
            .get(selector)
            .and_then(|m| m.custom_shortcuts.get(&key).cloned())
    })
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

/// Maximum number of entries kept in the recent-files list per
/// mount selector. Older entries are dropped first.
const RECENT_FILES_MAX: usize = 10;
/// `localStorage` key for a mount's recent-files list.
fn recent_files_key(selector: &str) -> String {
    format!("zedsheet::recent::{selector}")
}

/// One entry in the recent-files list. The `json` is the full
/// workbook JSON; the host can `load_data(selector, json)` to
/// restore. `timestamp_ms` is the wall-clock ms at the time the
/// entry was added (use Date.now() on the JS side).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecentFile {
    pub name: String,
    pub json: String,
    pub timestamp_ms: u64,
}

/// Return the recent-files list for a mount, newest first. Empty
/// when nothing has been pushed or the host's localStorage is
/// unavailable. Each call parses the JSON, so the caller should
/// cache the result if they're rendering a dropdown.
pub fn get_recent_files(selector: &str) -> Vec<RecentFile> {
    let Some(ls) = local_storage() else {
        return Vec::new();
    };
    let Ok(Some(raw)) = ls.get_item(&recent_files_key(selector)) else {
        return Vec::new();
    };
    if raw.trim().is_empty() {
        return Vec::new();
    }
    serde_json::from_str(&raw).unwrap_or_default()
}

/// Append a file to the recent-files list (Phase 5.4). The newest
/// entry wins: if a file with the same name already exists, it is
/// moved to the front. The list is capped at `RECENT_FILES_MAX`
/// entries — older ones are dropped. No-op on an empty name or
/// when localStorage is unavailable. Pure side-effect; no return
/// value.
pub fn push_recent_file(selector: &str, name: &str, json: &str, timestamp_ms: u64) {
    if name.is_empty() {
        return;
    }
    let Some(ls) = local_storage() else {
        return;
    };
    let list = upsert_recent(get_recent_files(selector), name, json, timestamp_ms);
    if let Ok(serialized) = serde_json::to_string(&list) {
        let _ = ls.set_item(&recent_files_key(selector), &serialized);
    }
}

/// Pure helper: de-dup by name (newest entry wins) + cap at
/// `RECENT_FILES_MAX`. Extracted so the logic is host-testable
/// without needing a `web_sys::Storage`.
fn upsert_recent(
    mut list: Vec<RecentFile>,
    name: &str,
    json: &str,
    timestamp_ms: u64,
) -> Vec<RecentFile> {
    list.retain(|f| f.name != name);
    list.insert(
        0,
        RecentFile {
            name: name.to_string(),
            json: json.to_string(),
            timestamp_ms,
        },
    );
    list.truncate(RECENT_FILES_MAX);
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upsert_recent_inserts_at_front() {
        // An empty list becomes a one-entry list with the new entry first.
        let list = upsert_recent(Vec::new(), "Q1", "{}", 100);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].name, "Q1");
    }

    #[test]
    fn upsert_recent_dedupes_by_name() {
        // Pushing the same name twice produces one entry with the
        // newer timestamp + json.
        let mut list = Vec::new();
        list = upsert_recent(list, "Q1", "{\"a\":1}", 100);
        list = upsert_recent(list, "Q2", "{\"b\":1}", 200);
        list = upsert_recent(list, "Q1", "{\"a\":2}", 300);
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Q1");
        assert_eq!(list[0].timestamp_ms, 300);
        assert_eq!(list[0].json, "{\"a\":2}");
        assert_eq!(list[1].name, "Q2");
    }

    #[test]
    fn upsert_recent_caps_at_max() {
        // 12 pushes of distinct names → list stays at 10; oldest 2
        // dropped.
        let mut list = Vec::new();
        for i in 0..12 {
            list = upsert_recent(list, &format!("F{i}"), "{}", i as u64);
        }
        assert_eq!(list.len(), RECENT_FILES_MAX);
        // Newest first: F11 is at index 0, F2 is at index 9.
        assert_eq!(list[0].name, "F11");
        assert_eq!(list[9].name, "F2");
    }

    #[test]
    fn recent_file_serde_round_trip() {
        // The host-facing API hands JSON arrays back to JS, so
        // RecentFile must round-trip through serde without losing
        // the json string (which itself contains quotes).
        let f = RecentFile {
            name: "Budget 2026".into(),
            json: r#"{"sheets":[{"name":"A","cells":[]}]}"#.into(),
            timestamp_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&f).unwrap();
        let back: RecentFile = serde_json::from_str(&json).unwrap();
        assert_eq!(back, f);
    }

    #[test]
    fn normalise_combo_orders_modifiers_and_lowercases_them() {
        // Modifier order is fixed: Ctrl, Alt, Shift. The key letter
        // keeps its original case so the host can distinguish
        // Shift+1 from 1 if they need to. Lookup is
        // case-insensitive on the letter, so the host can write
        // any of these and they all collide.
        assert_eq!(normalise_combo("Ctrl+Shift+K"), "ctrl+shift+K");
        assert_eq!(normalise_combo("shift+ctrl+K"), "ctrl+shift+K");
        assert_eq!(normalise_combo("  Alt  +  Enter "), "alt+Enter");
        assert_eq!(normalise_combo("k"), "k");
        assert_eq!(normalise_combo(""), "");
    }

    #[cfg(target_arch = "wasm32")]
    #[test]
    fn custom_shortcut_register_and_lookup_round_trip() {
        // The thread-local map is keyed by selector + combo.
        // Setting twice with the same (case-different) combo
        // overwrites; setting with None removes. Lookup uses
        // case-insensitive match on the combo so "Ctrl+k" and
        // "Ctrl+K" both hit.
        let sel = "test-sel";
        let cb: js_sys::Function = js_sys::Function::new_no_args("return 1;");
        set_custom_shortcut(sel, "Ctrl+K", Some(cb.clone()));
        assert!(get_custom_shortcut(sel, "Ctrl+K").is_some());
        assert!(get_custom_shortcut(sel, "ctrl+k").is_some());
        // Re-register with a different combo: both stay.
        let cb2: js_sys::Function = js_sys::Function::new_no_args("return 2;");
        set_custom_shortcut(sel, "Ctrl+Shift+L", Some(cb2.clone()));
        assert!(get_custom_shortcut(sel, "Ctrl+Shift+L").is_some());
        assert!(get_custom_shortcut(sel, "Ctrl+K").is_some());
        // Clear one combo: the other remains.
        set_custom_shortcut(sel, "Ctrl+K", None);
        assert!(get_custom_shortcut(sel, "Ctrl+K").is_none());
        assert!(get_custom_shortcut(sel, "Ctrl+Shift+L").is_some());
        // Different selector doesn't see the entry.
        assert!(get_custom_shortcut("other-sel", "Ctrl+Shift+L").is_none());
    }
}
