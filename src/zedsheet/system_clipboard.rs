//! System-clipboard glue for cross-app copy/paste (zedsheet ↔ Excel, Google
//! Sheets, …). Installs document-level `copy`/`cut`/`paste` listeners that
//! read and write the `text/plain` (TSV) and `text/html` (`<table>`) clipboard
//! flavors. The pure serialization/parsing lives in `core::clipboard_io`; this
//! module is the thin browser boundary.
//!
//! We only act on a clipboard event when our grid canvas is the focused
//! element, so we never hijack a copy/paste meant for the cell editor, the
//! formula bar, or text elsewhere on a host page. To make the canvas focusable
//! it is given `tabindex="-1"` and focused on mousedown.

use std::cell::Cell as StdCell;
use std::rc::Rc;

use gloo::utils::document;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{ClipboardEvent, Element as WebElement, HtmlElement};

use super::{EditingCell, SharedRenderer, SyncFn};
use crate::component::element::Element;
use crate::core::clipboard_io::{
    grid_from_rows, nonce_in_html, parse_tsv, to_html, to_tsv, ParsedGrid, RawCell,
};

/// Install the clipboard listeners and make the canvas focusable.
pub(crate) fn install(
    canvas_el: &mut Element,
    renderer: &SharedRenderer,
    editing: &EditingCell,
    sync: &SyncFn,
) {
    let Some(canvas) = canvas_el.el.clone() else {
        return;
    };

    // Make the canvas focusable (without a visible focus ring) so it can be the
    // target of native clipboard events.
    let _ = canvas.set_attribute("tabindex", "-1");
    if let Some(he) = canvas.dyn_ref::<HtmlElement>() {
        let _ = he.style().set_property("outline", "none");
    }

    // Focus the canvas on pointer-down so Ctrl/Cmd+C/V land on the grid.
    {
        let canvas = canvas.clone();
        canvas_el.add_event_listener("mousedown", move |_e: web_sys::Event| {
            if let Some(he) = canvas.dyn_ref::<HtmlElement>() {
                let _ = he.focus();
            }
        });
    }

    // A per-page-load nonce base so a paste can tell our own clipboard payload
    // apart from another tab's or another app's.
    let nonce = Rc::new(StdCell::new((js_sys::Math::random() * 1.0e9) as u64));

    install_copy(&canvas, renderer, editing, &nonce, false);
    install_copy(&canvas, renderer, editing, &nonce, true);
    install_paste(&canvas, renderer, editing, sync, &nonce);
}

/// Register a `copy` or `cut` listener (`is_cut` selects which).
fn install_copy(
    canvas: &WebElement,
    renderer: &SharedRenderer,
    editing: &EditingCell,
    nonce: &Rc<StdCell<u64>>,
    is_cut: bool,
) {
    let canvas = canvas.clone();
    let renderer = renderer.clone();
    let editing = editing.clone();
    let nonce = nonce.clone();
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        if !grid_is_focused(&canvas, &editing) {
            return; // let the browser handle native copy (editor, host page, …)
        }
        let Ok(ce) = event.dyn_into::<ClipboardEvent>() else {
            return;
        };
        let Some(dt) = ce.clipboard_data() else {
            return;
        };

        let payload = {
            let mut r = renderer.borrow_mut();
            let Some(range) = r.contiguous_selection() else {
                return;
            };
            let n = nonce.get().wrapping_add(1);
            nonce.set(n);
            if is_cut {
                r.cut_selection();
            } else {
                r.copy_selection();
            }
            (to_tsv(&r.data, &range), to_html(&r.data, &range, n))
        };
        let _ = dt.set_data("text/plain", &payload.0);
        let _ = dt.set_data("text/html", &payload.1);
        ce.prevent_default();
    });
    let _ = document().add_event_listener_with_callback(
        if is_cut { "cut" } else { "copy" },
        cb.as_ref().unchecked_ref(),
    );
    cb.forget();
}

/// Register the `paste` listener.
fn install_paste(
    canvas: &WebElement,
    renderer: &SharedRenderer,
    editing: &EditingCell,
    sync: &SyncFn,
    nonce: &Rc<StdCell<u64>>,
) {
    let canvas = canvas.clone();
    let renderer = renderer.clone();
    let editing = editing.clone();
    let sync = sync.clone();
    let nonce = nonce.clone();
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        if !grid_is_focused(&canvas, &editing) {
            return;
        }
        let Ok(ce) = event.dyn_into::<ClipboardEvent>() else {
            return;
        };
        let Some(dt) = ce.clipboard_data() else {
            return;
        };
        let html = dt.get_data("text/html").unwrap_or_default();
        let plain = dt.get_data("text/plain").unwrap_or_default();

        // Our own payload (nonce matches and the in-app clipboard is live) →
        // lossless internal paste, preserving styles and formulas exactly.
        if let Some(n) = nonce_in_html(&html) {
            let has_clip = renderer.borrow().has_clipboard();
            if n == nonce.get() && has_clip {
                {
                    let mut r = renderer.borrow_mut();
                    r.paste();
                    r.render();
                }
                ce.prevent_default();
                sync();
                return;
            }
        }

        // External content: prefer the HTML table (recovers merges), else TSV.
        let grid = if !html.is_empty() {
            parse_html_table(&html).unwrap_or_else(|| parse_tsv(&plain))
        } else if !plain.is_empty() {
            parse_tsv(&plain)
        } else {
            return;
        };
        if grid.is_empty() {
            return;
        }
        {
            let mut r = renderer.borrow_mut();
            r.paste_external(grid);
            r.render();
        }
        ce.prevent_default();
        sync();
    });
    let _ = document().add_event_listener_with_callback("paste", cb.as_ref().unchecked_ref());
    cb.forget();
}

/// Whether the grid canvas currently owns focus (and no cell editor is open),
/// meaning a clipboard event is ours to handle rather than the browser's.
fn grid_is_focused(canvas: &WebElement, editing: &EditingCell) -> bool {
    if editing.borrow().is_some() {
        return false;
    }
    match document().active_element() {
        Some(active) => active.is_same_node(Some(canvas.as_ref())),
        None => false,
    }
}

/// Parse pasted clipboard HTML into a grid by walking the first `<table>` with a
/// detached element (never inserted into the document, so no resources load and
/// no scripts run). Returns `None` when there is no usable table.
fn parse_html_table(html: &str) -> Option<ParsedGrid> {
    let div = document().create_element("div").ok()?;
    div.set_inner_html(html);
    // Clipboard HTML is often wrapped (`<html><body><table>…`). Take the first
    // table and restrict to ITS direct rows/cells (`:scope >`) so a nested
    // table inside a cell can't leak spurious rows. No table → caller falls
    // back to plain-text TSV.
    let table = div.query_selector("table").ok().flatten()?;
    let trs = table
        .query_selector_all(
            ":scope > tr, :scope > thead > tr, :scope > tbody > tr, :scope > tfoot > tr",
        )
        .ok()?;
    if trs.length() == 0 {
        return None;
    }
    let mut rows: Vec<Vec<RawCell>> = Vec::new();
    for i in 0..trs.length() {
        let Some(tr_node) = trs.item(i) else { continue };
        let Ok(tr) = tr_node.dyn_into::<WebElement>() else {
            continue;
        };
        let Ok(cells) = tr.query_selector_all(":scope > td, :scope > th") else {
            continue;
        };
        let mut row = Vec::with_capacity(cells.length() as usize);
        for j in 0..cells.length() {
            let Some(cell_node) = cells.item(j) else {
                continue;
            };
            let text = cell_node.text_content().unwrap_or_default();
            let (rs, cs) = match cell_node.dyn_into::<WebElement>() {
                Ok(cell) => (span_attr(&cell, "rowspan"), span_attr(&cell, "colspan")),
                Err(_) => (1, 1),
            };
            row.push(RawCell::new(text, rs, cs));
        }
        rows.push(row);
    }
    if rows.iter().all(|r| r.is_empty()) {
        return None;
    }
    Some(grid_from_rows(&rows))
}

/// Read a `rowspan`/`colspan` attribute, defaulting to 1.
fn span_attr(cell: &WebElement, name: &str) -> usize {
    cell.get_attribute(name)
        .and_then(|s| s.trim().parse::<usize>().ok())
        .filter(|&n| n >= 1)
        .unwrap_or(1)
}
