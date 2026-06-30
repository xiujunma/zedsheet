//! Image cache + loader (Phase 4.2).
//!
//! A single thread-local `HashMap<String, Option<HtmlImageElement>>`
//! keys on the image URL. The first call to `ensure_loaded` for a
//! given URL starts a fetch via a synthetic `<img>` element; the
//! `onload` / `onerror` handler updates the entry. Subsequent
//! `ensure_loaded` calls are no-ops. `get` returns the cached image
//! (`Some` when loaded, `None` when failed or in-flight).
//!
//! The cache is intentionally thread-local because:
//! - the renderer is the only consumer (no need to share across
//!   threads in a single-threaded wasm app), and
//! - the closure captures for `onload` / `onerror` would otherwise
//!   need a `Send + Sync` wrapper for `Rc<RefCell<…>>`.

use std::cell::RefCell;
use std::collections::HashMap;

use gloo::utils::document;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlImageElement};

thread_local! {
    static CACHE: RefCell<HashMap<String, Option<HtmlImageElement>>> =
        RefCell::new(HashMap::new());
    /// Per-thread set of URLs whose load is currently in flight
    /// (we don't have the image yet). Prevents re-kicking the same
    /// `onload` listener on every frame.
    static PENDING: RefCell<std::collections::HashSet<String>> =
        RefCell::new(std::collections::HashSet::new());
}

/// Return the cached image for `url`, if any. The image is
/// available only after `ensure_loaded` has fired the
/// `onload` handler for the URL — until then, the entry is
/// `None` (in-flight) or absent.
pub(crate) fn get(url: &str) -> Option<HtmlImageElement> {
    CACHE.with(|c| c.borrow().get(url).and_then(|o| o.clone()))
}

/// Kick off a load for `url` if one isn't already pending or
/// cached. Idempotent across multiple calls for the same URL.
/// The load runs in the background; `get` returns the decoded
/// image once `onload` fires.
pub(crate) fn ensure_loaded(url: &str) {
    let already_loaded = CACHE.with(|c| c.borrow().contains_key(url));
    if already_loaded {
        return;
    }
    let already_pending = PENDING.with(|p| p.borrow().contains(url));
    if already_pending {
        return;
    }
    PENDING.with(|p| p.borrow_mut().insert(url.to_string()));

    let img = match document().create_element("img") {
        Ok(el) => el,
        Err(_) => {
            // create_element failed — record the failure and bail.
            CACHE.with(|c| c.borrow_mut().insert(url.to_string(), None));
            PENDING.with(|p| p.borrow_mut().remove(url));
            return;
        }
    };
    let html_img: HtmlImageElement = match img.dyn_into() {
        Ok(i) => i,
        Err(_) => {
            CACHE.with(|c| c.borrow_mut().insert(url.to_string(), None));
            PENDING.with(|p| p.borrow_mut().remove(url));
            return;
        }
    };

    // onload → cache the decoded image.
    let url_owned = url.to_string();
    let img_for_load = html_img.clone();
    let onload2 = Closure::wrap(Box::new(move || {
        CACHE.with(|c| {
            c.borrow_mut()
                .insert(url_owned.clone(), Some(img_for_load.clone()));
        });
        PENDING.with(|p| {
            p.borrow_mut().remove(&url_owned);
        });
    }) as Box<dyn FnMut()>);
    HtmlElement::set_onload(&html_img, Some(onload2.as_ref().unchecked_ref()));
    onload2.forget();

    let url_for_err = url.to_string();
    let onerror = Closure::wrap(Box::new(move || {
        CACHE.with(|c| {
            c.borrow_mut().insert(url_for_err.clone(), None);
        });
        PENDING.with(|p| {
            p.borrow_mut().remove(&url_for_err);
        });
    }) as Box<dyn FnMut()>);
    HtmlElement::set_onerror(&html_img, Some(onerror.as_ref().unchecked_ref()));
    onerror.forget();

    html_img.set_src(url);
}
