//! IndexedDB-backed persistence (Phase 5.7 / BACKLOG §4).
//!
//! localStorage is 5–10 MB per origin; IndexedDB gives GBs of
//! room for large workbooks. This module exposes a small
//! debounced IDB writer the host can opt into alongside (or
//! instead of) the existing localStorage path.
//!
//! The Rust side doesn't implement the actual IDB write —
//! `web-sys` 0.3 doesn't expose IndexedDB types and a
//! purpose-built crate (`idb`, `indexed_db_futures`) would
//! be a new dep. Instead the Rust side just:
//!
//! - Tracks the per-mount debounce config (db / store / delay
//!   / last-saved value) in a thread-local map.
//! - On every `note_change`, compares the new JSON against
//!   the last-saved value; if they differ, schedules a
//!   `setTimeout` via the JS API that calls a host-supplied
//!   callback. The host's callback does the actual `indexedDB`
//!   work and reports back via `idb_persist_done`.
//!
//! The host wires it up like this:
//!
//! ```js
//! import { enableIdbPersist } from "./your-glue.js";
//! enableIdbPersist({
//!     selector: "#my-grid",
//!     dbName: "zedsheet",
//!     storeName: "workbooks",
//!     debounceMs: 500,
//!     save: async (key, value) => {
//!         // Use any IDB wrapper you like (idb, native, …).
//!         await openMyDb().put(storeName, value, key);
//!     },
//!     load: async (key) => openMyDb().get(storeName, key),
//! });
//! zedsheet.mount("#my-grid", {});
//! ```
//!
//! From then on, every workbook change is debounced + flushed
//! through the host's `save` callback, and the same callback
//! reads via `load` on mount.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use wasm_bindgen::prelude::wasm_bindgen;
use wasm_bindgen::{JsCast, JsValue};

/// Per-mount IDB config the host registers.
#[derive(Default)]
struct MountIdbConfig {
    /// DB name (e.g. "zedsheet").
    db_name: String,
    /// Object store name (e.g. "workbooks").
    store_name: String,
    /// Debounce window in milliseconds.
    debounce_ms: u32,
    /// Last value that was flushed to the host. The debounce
    /// timer is skipped when the new JSON matches this — so
    /// repeated no-op changes don't trigger saves.
    last_json: Option<String>,
    /// The active debounce timer (a JS `setTimeout` return).
    /// Reset every time a new change comes in within the
    /// debounce window.
    pending: Option<JsValue>,
    /// The host's `save` function. Signature:
    /// `(selector, dbName, storeName, key, value) => Promise`.
    save_fn: Option<js_sys::Function>,
    /// The host's `load` function. Signature:
    /// `(selector, dbName, storeName, key) => Promise<value>`.
    load_fn: Option<js_sys::Function>,
}

thread_local! {
    static IDB: RefCell<HashMap<String, MountIdbConfig>> = RefCell::new(HashMap::new());
}

/// Host wiring entry point. Registers the per-mount debounced
/// IDB writer. `save_fn` / `load_fn` are the host's IDB
/// callbacks; both are passed the `selector`, the db + store
/// names, and a `key` (the mount selector). They return
/// Promises.
#[wasm_bindgen]
pub fn enable_idb_persist(
    selector: &str,
    db_name: &str,
    store_name: &str,
    debounce_ms: u32,
    save_fn: js_sys::Function,
    load_fn: js_sys::Function,
) {
    IDB.with(|m| {
        m.borrow_mut().insert(
            selector.to_string(),
            MountIdbConfig {
                db_name: db_name.to_string(),
                store_name: store_name.to_string(),
                debounce_ms,
                save_fn: Some(save_fn),
                load_fn: Some(load_fn),
                ..Default::default()
            },
        );
    });
}

/// Host tells us the most recent save completed. Lets the
/// debounce loop reset `last_json` so the next real change
/// fires a save instead of being skipped as a no-op.
#[wasm_bindgen]
pub fn idb_persist_done(selector: &str, json: &str) {
    IDB.with(|m| {
        if let Some(cfg) = m.borrow_mut().get_mut(selector) {
            cfg.last_json = Some(json.to_string());
        }
    });
}

/// Disable IDB persistence for a mount (the host calls this
/// on teardown). Internal helper — the `#[wasm_bindgen]`
/// wrapper lives in `lib.rs` so we get one JS export, not two.
pub(crate) fn disable_idb_persist(selector: &str) {
    IDB.with(|m| {
        m.borrow_mut().remove(selector);
    });
}

/// Schedule a debounced save. Called from the persist
/// pipeline when armed and the JSON differs. Returns `true`
/// when a save was scheduled, `false` when the mount has no
/// IDB config (or the value is unchanged). Fire-and-forget:
/// any error in the host's `save_fn` shows up in the console
/// but never panics the engine.
pub(crate) fn maybe_save_to_idb(selector: &str, json: &str) -> bool {
    let (to_call, key, db, store) = IDB.with(|m| {
        let mut map = m.borrow_mut();
        let Some(cfg) = map.get_mut(selector) else {
            return (None, String::new(), String::new(), String::new());
        };
        if cfg.last_json.as_deref() == Some(json) {
            return (None, String::new(), String::new(), String::new());
        }
        cfg.last_json = Some(json.to_string());
        let Some(save_fn) = cfg.save_fn.clone() else {
            return (None, String::new(), String::new(), String::new());
        };
        (
            Some(Rc::new(save_fn)),
            selector.to_string(),
            cfg.db_name.clone(),
            cfg.store_name.clone(),
        )
    });
    let Some(save_fn) = to_call else {
        return false;
    };
    // Schedule via the JS setTimeout. We pass the callback as
    // a `Function` and let `spawn_local` drive the promise
    // it returns.
    let key_js = JsValue::from_str(&key);
    let db_js = JsValue::from_str(&db);
    let store_js = JsValue::from_str(&store);
    let json_js = JsValue::from_str(json);
    let save_fn_for_call = save_fn.clone();
    let cb = js_sys::Function::new_with_args(
        "selector, db, store, key, value",
        r#"
            const p = saveFn(selector, db, store, key, value);
            if (p && typeof p.then === 'function') {
                p.then(() => {
                    // Tell the engine the save completed so the
                    // next real change fires a save instead of
                    // being deduped.
                    if (typeof globalThis.__zedsheet_idb_done === 'function') {
                        globalThis.__zedsheet_idb_done(selector, value);
                    }
                }).catch((e) => {
                    console.warn('zedsheet: IDB save failed', e);
                });
            }
        "#,
    );
    let _ = js_sys::Reflect::set(&cb, &JsValue::from_str("saveFn"), save_fn_for_call.as_ref());
    let _ = cb.bind0(&JsValue::NULL);
    wasm_bindgen_futures::spawn_local(async move {
        let _ = cb.apply(
            &JsValue::NULL,
            &js_sys::Array::of5(&key_js, &db_js, &store_js, &key_js, &json_js),
        );
    });
    true
}

/// Load the saved JSON for this mount via the host's
/// `load_fn`. Returns `None` if the mount has no IDB config,
/// the load function is missing, or the load returns
/// nothing.
pub async fn load_via_idb(selector: &str) -> Option<String> {
    // Drop the borrow before the await point so the Ref<'_,
    // HashMap<…>>'s lifetime doesn't outlive the closure's
    // scope (rustc can't prove the lock is released when the
    // async block suspends on `.await`).
    let (load_fn, db, store) = IDB.with(|m| {
        let map = m.borrow();
        let cfg = map.get(selector)?;
        Some((
            cfg.load_fn.clone()?,
            cfg.db_name.clone(),
            cfg.store_name.clone(),
        ))
    })?;
    let key_js = JsValue::from_str(selector);
    let db_js = JsValue::from_str(&db);
    let store_js = JsValue::from_str(&store);
    let promise = load_fn
        .call4(
            &JsValue::NULL,
            &JsValue::from_str(selector),
            &db_js,
            &store_js,
            &key_js,
        )
        .ok()?;
    let promise: js_sys::Promise = promise
        .dyn_into()
        .map_err(|_| JsValue::from_str("load_fn must return a Promise"))
        .ok()?;
    let value = wasm_bindgen_futures::JsFuture::from(promise).await.ok()?;
    value.as_string()
}
