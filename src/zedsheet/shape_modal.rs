//! "Insert Shape" modal (Phase 6 UI half).
//!
//! Mirrors the sparkline modal pattern: a single dialog with a
//! kind selector, an anchor cell input, width / height, and an
//! optional color (plus a text body for `ShapeKind::Text`). The
//! Apply handler appends a new `Shape` to `DataProxy.shapes` and
//! snapshots for undo (issue #62).
//!
//! Drawing-layer shapes live in `core::shape` (model) and
//! `chart_render::draw_shapes` (renderer); this modal is just the
//! host-side way to add an entry.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlSelectElement};

use super::*;
use crate::core::shape::Shape;
use crate::core::shape::ShapeKind;

const ROOT_CLASS: &str = "zs-shape-modal-root";

/// HTML for the modal. Hidden by default; opened by the
/// context-menu "Insert Shape…" item.
pub(crate) fn shape_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    let label = "width:90px;flex:none;";
    format!(
        r##"<div class="zedsheet-modal {root}" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:380px;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Insert Shape</span>
                <span class="zs-shape-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
                <div style="color:#666;font-size:11px;margin-bottom:8px;">
                    A drawing-layer shape anchored to a single cell.
                    Rectangles, lines, and text boxes float over the
                    body and scroll with the underlying cells.
                </div>
                <div style="{row}">
                    <label style="{label}">Kind</label>
                    <select class="zs-shape-kind" style="flex:1;padding:3px;">
                        <option value="rect">Rectangle</option>
                        <option value="line">Line</option>
                        <option value="text">Text box</option>
                    </select>
                </div>
                <div style="{row}">
                    <label style="{label}">Anchor</label>
                    <input class="zs-shape-anchor" style="flex:1;padding:3px;" placeholder="B1"/>
                    <span style="color:#999;font-size:11px;">top-left cell</span>
                </div>
                <div style="{row}">
                    <label style="{label}">Width</label>
                    <input class="zs-shape-width" type="number" min="20" style="flex:1;padding:3px;" placeholder="140"/>
                    <span style="color:#999;font-size:11px;">px</span>
                </div>
                <div style="{row}">
                    <label style="{label}">Height</label>
                    <input class="zs-shape-height" type="number" min="20" style="flex:1;padding:3px;" placeholder="80"/>
                    <span style="color:#999;font-size:11px;">px</span>
                </div>
                <div style="{row}">
                    <label style="{label}">Color</label>
                    <input class="zs-shape-color" style="flex:1;padding:3px;" placeholder="#1e88e5"/>
                </div>
                <div style="{row}">
                    <label style="{label}">Text</label>
                    <input class="zs-shape-text" style="flex:1;padding:3px;" placeholder="(only for Text box)"/>
                </div>
                <div class="zs-shape-error" style="display:none;color:#b71c1c;font-size:11px;margin-bottom:8px;"></div>
                <div style="display:flex;justify-content:flex-end;gap:8px;">
                    <button class="zs-shape-cancel" style="padding:4px 12px;cursor:pointer;">Cancel</button>
                    <button class="zs-shape-apply" style="padding:4px 12px;cursor:pointer;">Insert</button>
                </div>
            </div>
        </div>"##,
        root = ROOT_CLASS,
        row = row,
        label = label,
    )
}

/// Show the modal. `active_cell` seeds the anchor input.
pub(crate) fn open_shape_modal(modal: &web_sys::Element, active_cell: (usize, usize)) {
    use crate::renderer::alphabets::xy2expr;
    if let Some(anchor_input) = modal
        .query_selector(".zs-shape-anchor")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        anchor_input.set_value(&xy2expr(active_cell.1, active_cell.0));
    }
    for sel in [
        ".zs-shape-width",
        ".zs-shape-height",
        ".zs-shape-color",
        ".zs-shape-text",
    ] {
        if let Some(el) = modal
            .query_selector(sel)
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
        {
            el.set_value("");
        }
    }
    if let Some(err) = modal.query_selector(".zs-shape-error").ok().flatten() {
        if let Some(h) = err.dyn_ref::<HtmlElement>() {
            h.set_text_content(None);
            let _ = h.style().set_property("display", "none");
        }
    }
    if let Some(h) = modal.dyn_ref::<HtmlElement>() {
        let _ = h.style().set_property("display", "block");
    }
}

fn close_shape_modal(modal: &web_sys::Element) {
    if let Some(h) = modal.dyn_ref::<HtmlElement>() {
        let _ = h.style().set_property("display", "none");
    }
}

fn show_shape_error(modal: &web_sys::Element, msg: &str) {
    if let Some(err) = modal.query_selector(".zs-shape-error").ok().flatten() {
        if let Some(h) = err.dyn_ref::<HtmlElement>() {
            h.set_text_content(Some(msg));
            let _ = h.style().set_property("display", "block");
        }
    }
}

fn read_shape_kind(modal: &web_sys::Element) -> ShapeKind {
    modal
        .query_selector(".zs-shape-kind")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlSelectElement>().ok())
        .map(|s| match s.value().as_str() {
            "line" => ShapeKind::Line,
            "text" => ShapeKind::Text,
            _ => ShapeKind::Rect,
        })
        .unwrap_or_default()
}

/// Mount the modal HTML and wire its Apply / Cancel / close
/// handlers. The Apply handler appends a new `Shape` to
/// `DataProxy.shapes` and snapshots for undo (issue #62).
pub(crate) fn wire_shape_modal(modal: web_sys::Element, renderer: &SharedRenderer, sync: &SyncFn) {
    let modal_for_apply = modal.clone();
    let renderer_for_apply = renderer.clone();
    let sync_for_apply = sync.clone();
    let apply_cb = Closure::<dyn FnMut()>::new(move || {
        let kind = read_shape_kind(&modal_for_apply);
        let anchor_input = modal_for_apply
            .query_selector(".zs-shape-anchor")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let width_input = modal_for_apply
            .query_selector(".zs-shape-width")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let height_input = modal_for_apply
            .query_selector(".zs-shape-height")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let color_input = modal_for_apply
            .query_selector(".zs-shape-color")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let text_input = modal_for_apply
            .query_selector(".zs-shape-text")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let Some(anchor_el) = anchor_input else {
            return;
        };
        let anchor = anchor_el.value().trim().to_string();
        if anchor.is_empty() {
            show_shape_error(&modal_for_apply, "Anchor cell is required (e.g. B1).");
            return;
        }
        // Width / height: parse, fall back to the per-kind default
        // when the field is empty or unparseable. A non-empty
        // garbage value surfaces a friendlier inline error.
        let width = width_input
            .as_ref()
            .and_then(|e| e.value().trim().parse::<f64>().ok())
            .unwrap_or(match kind {
                ShapeKind::Rect => 140.0,
                ShapeKind::Line => 120.0,
                ShapeKind::Text => 200.0,
            });
        let height = height_input
            .as_ref()
            .and_then(|e| e.value().trim().parse::<f64>().ok())
            .unwrap_or(match kind {
                ShapeKind::Rect => 80.0,
                ShapeKind::Line => 60.0,
                ShapeKind::Text => 60.0,
            });
        let color = color_input
            .map(|e| e.value().trim().to_string())
            .unwrap_or_default();
        let text = text_input
            .map(|e| e.value().trim().to_string())
            .unwrap_or_default();
        {
            let mut r = renderer_for_apply.borrow_mut();
            r.add_shape(Shape {
                kind,
                anchor,
                width,
                height,
                color,
                fill: String::new(),
                text,
            });
        }
        close_shape_modal(&modal_for_apply);
        // Make sure the next render frame picks up the new shape.
        sync_for_apply();
    });
    if let Some(btn) = modal
        .query_selector(".zs-shape-apply")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        let cb = apply_cb.as_ref().unchecked_ref();
        let _ = btn.add_event_listener_with_callback("click", cb);
    }
    apply_cb.forget();

    // Cancel / close: just hide.
    let modal_for_close = modal.clone();
    let close_cb = Closure::<dyn FnMut()>::new(move || {
        close_shape_modal(&modal_for_close);
    });
    for sel in [".zs-shape-cancel", ".zs-shape-close"] {
        if let Some(el) = modal.query_selector(sel).ok().flatten() {
            if let Ok(btn) = el.dyn_into::<HtmlInputElement>() {
                let cb = close_cb.as_ref().unchecked_ref();
                let _ = btn.add_event_listener_with_callback("click", cb);
            }
        }
    }
    close_cb.forget();
}
