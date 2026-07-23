//! "Insert Image" modal (Phase 4.2).
//!
//! Minimal URL-only flow: a single text input for the image URL, a
//! second input for the anchor cell, and Insert / Cancel buttons.
//! The Apply handler appends a new \`Image\` to
//! \`DataProxy.images\` and snapshots for undo (issue #62).
//!
//! Out of scope (deferred to follow-ups):
//! - Clipboard paste of an image (system clipboard → base64 data
//!   URL).
//! - Resize handles (the slicer drag/resize work in Phase 1.1 is
//!   the reference pattern).
//! - Z-order management (multiple images on overlapping cells).

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::spawn_local;
use web_sys::{HtmlElement, HtmlInputElement};

use super::*;

/// External bindings for the async clipboard API. The browser
/// only exposes `navigator.clipboard.readText()` behind a Promise
/// (Phase 4.2 follow-up: read a blob from the clipboard is a
/// larger lift using `read()` + ClipboardItem); `readText()` covers
/// the most common case where the user copies an image URL first
/// and then wants to paste it.
#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen::prelude::wasm_bindgen(js_namespace = navigator, catch)]
    async fn clipboardReadText() -> Result<JsValue, JsValue>;
}

const ROOT_CLASS: &str = "zs-image-modal-root";

/// HTML for the modal. Hidden by default; opened by the
/// context-menu "Insert Image…" item.
pub(crate) fn image_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    let label = "width:90px;flex:none;";
    format!(
        r##"<div class="zedsheet-modal {root}" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:420px;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Insert Image</span>
                <span class="zs-image-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
                <div style="color:#666;font-size:11px;margin-bottom:8px;">
                    Enter an image URL (http/https) or a data: URL. The
                    image is loaded once and cached for subsequent
                    frames. The anchor cell is the image's top-left
                    corner.
                </div>
                <div style="{row}">
                    <label style="{label}">URL</label>
                    <input class="zs-image-url" type="text" style="flex:1;padding:3px;" placeholder="https://example.com/cat.png"/>
                    <button class="zs-image-paste" style="padding:3px 8px;cursor:pointer;" title="Paste a URL from the clipboard">Paste</button>
                </div>
                <div style="{row}">
                    <label style="{label}">Anchor</label>
                    <input class="zs-image-anchor" type="text" style="flex:1;padding:3px;" placeholder="A1"/>
                    <span style="color:#999;font-size:11px;">top-left cell</span>
                </div>
                <div class="zs-image-error" style="display:none;color:#b71c1c;font-size:11px;margin-bottom:8px;"></div>
                <div style="display:flex;justify-content:flex-end;gap:8px;">
                    <button class="zs-image-cancel" style="padding:4px 12px;cursor:pointer;">Cancel</button>
                    <button class="zs-image-apply" style="padding:4px 12px;cursor:pointer;">Insert</button>
                </div>
            </div>
        </div>"##,
        root = ROOT_CLASS,
        row = row,
        label = label,
    )
}

/// Show the modal. \`active_cell\` seeds the anchor input.
pub(crate) fn open_image_modal(modal: &web_sys::Element, active_cell: (usize, usize)) {
    use crate::renderer::alphabets::xy2expr;
    if let Some(anchor_input) = modal
        .query_selector(".zs-image-anchor")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        anchor_input.set_value(&xy2expr(active_cell.1, active_cell.0));
    }
    if let Some(url_input) = modal
        .query_selector(".zs-image-url")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        url_input.set_value("");
    }
    if let Some(err) = modal.query_selector(".zs-image-error").ok().flatten() {
        if let Some(h) = err.dyn_ref::<HtmlElement>() {
            h.set_text_content(None);
            let _ = h.style().set_property("display", "none");
        }
    }
    if let Some(h) = modal.dyn_ref::<HtmlElement>() {
        let _ = h.style().set_property("display", "block");
    }
}

fn close_image_modal(modal: &web_sys::Element) {
    if let Some(h) = modal.dyn_ref::<HtmlElement>() {
        let _ = h.style().set_property("display", "none");
    }
}

fn show_image_error(modal: &web_sys::Element, msg: &str) {
    if let Some(err) = modal.query_selector(".zs-image-error").ok().flatten() {
        if let Some(h) = err.dyn_ref::<HtmlElement>() {
            h.set_text_content(Some(msg));
            let _ = h.style().set_property("display", "block");
        }
    }
}

/// Mount the modal HTML and wire its Apply / Cancel / close
/// handlers. The Apply handler appends a new \`Image\` to
/// \`DataProxy.images\` and snapshots for undo (issue #62).
pub(crate) fn wire_image_modal(
    modal: web_sys::Element,
    renderer: &SharedRenderer,
    sheets: &Sheets,
    active: &ActiveSheet,
    sync: &SyncFn,
) {
    let modal_for_apply = modal.clone();
    let renderer_for_apply = renderer.clone();
    let sheets_for_apply = sheets.clone();
    let active_for_apply = active.clone();
    let sync_for_apply = sync.clone();
    let apply_cb = Closure::<dyn FnMut()>::new(move || {
        let ai = *active_for_apply.borrow();
        let url_input = modal_for_apply
            .query_selector(".zs-image-url")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let anchor_input = modal_for_apply
            .query_selector(".zs-image-anchor")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let (Some(url_el), Some(anchor_el)) = (url_input, anchor_input) else {
            return;
        };
        let url = url_el.value().trim().to_string();
        let anchor = anchor_el.value().trim().to_string();
        if url.is_empty() {
            show_image_error(&modal_for_apply, "URL is required.");
            return;
        }
        if anchor.is_empty() {
            show_image_error(&modal_for_apply, "Anchor cell is required (e.g. A1).");
            return;
        }
        // Sanity-check the anchor via the same alphabetic parser the
        // engine uses for cell references; reject anything that doesn't
        // decode to a real cell rather than failing silently in the renderer.
        use crate::renderer::alphabets::exp2xy;
        if exp2xy(&anchor).is_none() {
            show_image_error(&modal_for_apply, "Anchor must be a valid cell (e.g. A1).");
            return;
        }
        {
            // Snapshot before mutating (undo, issue #62).
            let mut r = renderer_for_apply.borrow_mut();
            r.snapshot();
        }
        {
            let mut s = sheets_for_apply.borrow_mut();
            s[ai].images.push(crate::core::image::Image {
                src: url,
                anchor,
                width: 220.0,
                height: 160.0,
                alt: String::new(),
            });
        }
        close_image_modal(&modal_for_apply);
        // Make sure the next render frame picks up the new image.
        sync_for_apply();
    });
    if let Some(btn) = modal
        .query_selector(".zs-image-apply")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        let cb = apply_cb.as_ref().unchecked_ref();
        let _ = btn.add_event_listener_with_callback("click", cb);
    }
    apply_cb.forget();

    // Paste from clipboard: read the clipboard as text and drop it
    // into the URL input (Phase 4.2 follow-up). The user copies
    // an image URL, then opens the dialog and clicks Paste — no
    // manual typing. The async read lives in `spawn_local` so the
    // click handler returns immediately; the future writes back
    // into the same input element. Errors (permission denied, no
    // text on the clipboard) surface as inline error text.
    let paste_modal = modal.clone();
    let paste_cb = Closure::<dyn FnMut()>::new(move || {
        let modal_for_paste = paste_modal.clone();
        spawn_local(async move {
            match clipboardReadText().await {
                Ok(value) => {
                    let s = value.as_string().unwrap_or_default();
                    let trimmed = s.trim().to_string();
                    if let Some(input) = modal_for_paste
                        .query_selector(".zs-image-url")
                        .ok()
                        .flatten()
                        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                    {
                        if !trimmed.is_empty() {
                            input.set_value(&trimmed);
                        }
                    }
                }
                Err(_) => {
                    show_image_error(
                        &modal_for_paste,
                        "Couldn't read the clipboard. Copy an image URL first.",
                    );
                }
            }
        });
    });
    if let Some(btn) = modal
        .query_selector(".zs-image-paste")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        let cb = paste_cb.as_ref().unchecked_ref();
        let _ = btn.add_event_listener_with_callback("click", cb);
    }
    paste_cb.forget();

    // Cancel / close: just hide.
    let modal_for_close = modal.clone();
    let close_cb = Closure::<dyn FnMut()>::new(move || {
        close_image_modal(&modal_for_close);
    });
    for sel in [".zs-image-cancel", ".zs-image-close"] {
        if let Some(el) = modal.query_selector(sel).ok().flatten() {
            if let Ok(btn) = el.dyn_into::<HtmlInputElement>() {
                let cb = close_cb.as_ref().unchecked_ref();
                let _ = btn.add_event_listener_with_callback("click", cb);
            }
        }
    }
    close_cb.forget();
}
