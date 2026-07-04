//! "Insert Sparkline" modal (Phase 4.1b/5.2).
//!
//! Mirrors the chart modal pattern: a single dialog with a kind
//! selector, a data-range input, and an anchor cell. The Apply
//! handler appends a new `Sparkline` to `DataProxy.sparklines` and
//! snapshots for undo (issue #62).
//!
//! Sparklines are inline mini-charts drawn inside a single cell;
//! see `core::sparkline` for the data model and
//! `chart_render::draw_sparklines` for the renderer.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlSelectElement};

use super::*;
use crate::core::sparkline::SparklineKind;

const ROOT_CLASS: &str = "zs-sparkline-modal-root";

/// HTML for the modal. Hidden by default; opened by the
/// context-menu "Insert Sparkline…" item.
pub(crate) fn sparkline_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    let label = "width:90px;flex:none;";
    format!(
        r##"<div class="zedsheet-modal {root}" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:380px;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Insert Sparkline</span>
                <span class="zs-sparkline-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
                <div style="color:#666;font-size:11px;margin-bottom:8px;">
                    A sparkline is an inline mini-chart drawn inside a
                    single cell. The data range is the source values;
                    the anchor is the cell that hosts the line.
                </div>
                <div style="{row}">
                    <label style="{label}">Kind</label>
                    <select class="zs-sparkline-kind" style="flex:1;padding:3px;">
                        <option value="line">Line</option>
                        <option value="column">Column</option>
                        <option value="winloss">Win / Loss</option>
                    </select>
                </div>
                <div style="{row}">
                    <label style="{label}">Data range</label>
                    <input class="zs-sparkline-range" style="flex:1;padding:3px;" placeholder="A1:A12"/>
                </div>
                <div style="{row}">
                    <label style="{label}">Anchor</label>
                    <input class="zs-sparkline-anchor" style="flex:1;padding:3px;" placeholder="B1"/>
                    <span style="color:#999;font-size:11px;">host cell</span>
                </div>
                <div style="{row}">
                    <label style="{label}">Color</label>
                    <input class="zs-sparkline-color" style="flex:1;padding:3px;" placeholder="#1e88e5"/>
                </div>
                <div class="zs-sparkline-error" style="display:none;color:#b71c1c;font-size:11px;margin-bottom:8px;"></div>
                <div style="display:flex;justify-content:flex-end;gap:8px;">
                    <button class="zs-sparkline-cancel" style="padding:4px 12px;cursor:pointer;">Cancel</button>
                    <button class="zs-sparkline-apply" style="padding:4px 12px;cursor:pointer;">Insert</button>
                </div>
            </div>
        </div>"##,
        root = ROOT_CLASS,
        row = row,
        label = label,
    )
}

/// Show the modal. `active_cell` seeds the anchor input.
pub(crate) fn open_sparkline_modal(modal: &web_sys::Element, active_cell: (usize, usize)) {
    use crate::renderer::alphabets::xy2expr;
    if let Some(anchor_input) = modal
        .query_selector(".zs-sparkline-anchor")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
    {
        anchor_input.set_value(&xy2expr(active_cell.1, active_cell.0));
    }
    for sel in [".zs-sparkline-range", ".zs-sparkline-color"] {
        if let Some(el) = modal
            .query_selector(sel)
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
        {
            el.set_value("");
        }
    }
    if let Some(err) = modal.query_selector(".zs-sparkline-error").ok().flatten() {
        if let Some(h) = err.dyn_ref::<HtmlElement>() {
            h.set_text_content(None);
            let _ = h.style().set_property("display", "none");
        }
    }
    if let Some(h) = modal.dyn_ref::<HtmlElement>() {
        let _ = h.style().set_property("display", "block");
    }
}

fn close_sparkline_modal(modal: &web_sys::Element) {
    if let Some(h) = modal.dyn_ref::<HtmlElement>() {
        let _ = h.style().set_property("display", "none");
    }
}

fn show_sparkline_error(modal: &web_sys::Element, msg: &str) {
    if let Some(err) = modal.query_selector(".zs-sparkline-error").ok().flatten() {
        if let Some(h) = err.dyn_ref::<HtmlElement>() {
            h.set_text_content(Some(msg));
            let _ = h.style().set_property("display", "block");
        }
    }
}

fn read_kind(modal: &web_sys::Element) -> SparklineKind {
    modal
        .query_selector(".zs-sparkline-kind")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlSelectElement>().ok())
        .map(|s| match s.value().as_str() {
            "column" => SparklineKind::Column,
            "winloss" => SparklineKind::WinLoss,
            _ => SparklineKind::Line,
        })
        .unwrap_or_default()
}

/// Mount the modal HTML and wire its Apply / Cancel / close
/// handlers. The Apply handler appends a new `Sparkline` to
/// `DataProxy.sparklines` and snapshots for undo (issue #62).
pub(crate) fn wire_sparkline_modal(
    modal: web_sys::Element,
    renderer: &SharedRenderer,
    sync: &SyncFn,
) {
    let modal_for_apply = modal.clone();
    let renderer_for_apply = renderer.clone();
    let sync_for_apply = sync.clone();
    let apply_cb = Closure::<dyn FnMut()>::new(move || {
        let kind = read_kind(&modal_for_apply);
        let range_input = modal_for_apply
            .query_selector(".zs-sparkline-range")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let anchor_input = modal_for_apply
            .query_selector(".zs-sparkline-anchor")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let color_input = modal_for_apply
            .query_selector(".zs-sparkline-color")
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let (Some(range_el), Some(anchor_el)) = (range_input, anchor_input) else {
            return;
        };
        let range = range_el.value().trim().to_string();
        let anchor = anchor_el.value().trim().to_string();
        let color = color_input
            .map(|e| e.value().trim().to_string())
            .unwrap_or_default();
        if range.is_empty() {
            show_sparkline_error(&modal_for_apply, "Data range is required.");
            return;
        }
        if anchor.is_empty() {
            show_sparkline_error(&modal_for_apply, "Anchor cell is required (e.g. B1).");
            return;
        }
        // Sanity-check the data range parses as a cell range; reject
        // garbage before pushing it onto the sheet.
        if crate::core::cell_range::CellRange::from_str(&range).is_err() {
            show_sparkline_error(
                &modal_for_apply,
                "Data range is not a valid A1-style range.",
            );
            return;
        }
        {
            let mut r = renderer_for_apply.borrow_mut();
            r.add_sparkline(kind, range, anchor, color);
        }
        close_sparkline_modal(&modal_for_apply);
        // Make sure the next render frame picks up the new sparkline.
        sync_for_apply();
    });
    if let Some(btn) = modal
        .query_selector(".zs-sparkline-apply")
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
        close_sparkline_modal(&modal_for_close);
    });
    for sel in [".zs-sparkline-cancel", ".zs-sparkline-close"] {
        if let Some(el) = modal.query_selector(sel).ok().flatten() {
            if let Ok(btn) = el.dyn_into::<HtmlInputElement>() {
                let cb = close_cb.as_ref().unchecked_ref();
                let _ = btn.add_event_listener_with_callback("click", cb);
            }
        }
    }
    close_cb.forget();
}
