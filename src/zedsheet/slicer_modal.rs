//! Insert-Slicer dialog + floating slicer panels (issue #61).
//!
//! A slicer is a floating visual filter bound to a single source field;
//! the engine reads it from `DataProxy.slicers` and applies it as an
//! additional row predicate for every pivot on the source. This module
//! owns the *UI* half: the "Insert Slicer" dialog (a list of existing
//! slicers with delete + a field picker + Create button) and one
//! floating panel per slicer, appended to the sheet container so it
//! overlays the canvas.
//!
//! The wire function attaches a single delegated click handler to the
//! sheet container — chip clicks, Clear, close are all routed through
//! it, so re-rendering the panel (which wipes and rebuilds the chips)
//! doesn't have to re-wire anything.

use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlSelectElement};

use crate::component::element::Element;
use crate::core::pivot::Slicer;
#[allow(unused_imports)]
use super::*;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// CSS class prefix used to find our own DOM nodes inside the sheet
/// container (slicer panels and chips). Centralized so a rename here
/// is the only edit needed if the convention changes.
const PANEL_ATTR: &str = "data-slicer-panel";
const CHIP_ATTR: &str = "data-slicer-chip";
const CLEAR_ATTR: &str = "data-slicer-clear";
const CLOSE_ATTR: &str = "data-slicer-close";

pub(crate) fn slicer_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    let label = "width:90px;flex:none;";
    format!(
        r##"<div class="zedsheet-modal zs-slicer-root" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:480px;max-height:80vh;overflow-y:auto;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Insert Slicer</span>
                <span class="zs-slicer-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
                <div class="zs-slicer-list" style="max-height:140px;overflow-y:auto;margin-bottom:10px;"></div>
                <div style="border-top:1px solid #e6e6e6;margin-bottom:10px;"></div>
                <div style="{row}">
                    <label style="{label}">Field</label>
                    <select class="zs-slicer-field" style="flex:1;padding:3px;"></select>
                </div>
                <div style="{row}">
                    <label style="{label}">X (px)</label>
                    <input class="zs-slicer-x" type="number" min="0" value="200" style="flex:1;padding:3px;"/>
                </div>
                <div style="{row}">
                    <label style="{label}">Y (px)</label>
                    <input class="zs-slicer-y" type="number" min="0" value="80" style="flex:1;padding:3px;"/>
                </div>
                <div style="color:#999;font-size:11px;margin-bottom:8px;">
                    Filter pivots by clicking chips in the floating panel. Empty selection = "All".
                </div>
                <div style="display:flex;justify-content:flex-end;gap:8px;">
                    <button class="zs-slicer-create" style="padding:4px 12px;cursor:pointer;">Create slicer</button>
                    <button class="zs-slicer-done" style="padding:4px 12px;cursor:pointer;">Close</button>
                </div>
            </div>
        </div>"##,
        row = row,
        label = label
    )
}

/// Render the existing-slicers list inside the modal. Empty list shows a
/// "No slicers yet" placeholder. Each row has the field name and a
/// Delete button keyed by index.
fn render_slicer_list(modal: &web_sys::Element, sheets: &Sheets, active: &ActiveSheet) {
    let Ok(Some(list)) = modal.query_selector(".zs-slicer-list") else { return };
    let src = sheets.borrow()[*active.borrow()].clone();
    if src.slicers.is_empty() {
        list.set_inner_html("<div style=\"color:#999;padding:2px 0;\">No slicers yet.</div>");
        return;
    }
    // Read header row labels so the list shows "Slicer on Region" rather
    // than "Slicer on field 0".
    let headers = read_header_labels(&src, src.row_count.max(1));
    let mut html = String::new();
    for (i, s) in src.slicers.iter().enumerate() {
        let field_label = headers.get(s.field_idx).cloned().unwrap_or_default();
        let field_label = if field_label.is_empty() {
            format!("column {}", s.field_idx + 1)
        } else {
            field_label
        };
        let sel = if s.selected_values.is_empty() {
            "All".to_string()
        } else {
            format!("{} selected", s.selected_values.len())
        };
        html.push_str(&format!(
            "<div style=\"display:flex;align-items:center;gap:8px;padding:3px 0;\">\
               <span style=\"flex:1;\">Slicer on <b>{}</b> · {}</span>\
               <button {}=\"{}\" style=\"padding:1px 8px;cursor:pointer;\">Delete</button>\
             </div>",
            esc(&field_label),
            esc(&sel),
            "data-slicer-del",
            i,
        ));
    }
    list.set_inner_html(&html);
}

/// Read the first row of the active sheet as header labels, one per
/// column up to `max_col`. Used to populate the field `<select>` and
/// to label existing slicers in the list.
fn read_header_labels(src: &crate::core::data_proxy::DataProxy, max_col: usize) -> Vec<String> {
    (0..max_col)
        .map(|ci| src.cell_raw_value(0, ci).trim().to_string())
        .collect()
}

/// Populate the field `<select>` from the active sheet's row 0.
fn populate_field_select(modal: &web_sys::Element, sheets: &Sheets, active: &ActiveSheet) {
    let Ok(Some(sel_el)) = modal.query_selector(".zs-slicer-field") else { return };
    let Ok(sel) = sel_el.dyn_into::<HtmlSelectElement>() else { return };
    let src = sheets.borrow()[*active.borrow()].clone();
    let max_col = src.row_count.max(1).min(64);
    let headers = read_header_labels(&src, max_col);
    let mut html = String::new();
    for (i, h) in headers.iter().enumerate() {
        let label = if h.is_empty() {
            format!("Column {}", i + 1)
        } else {
            h.clone()
        };
        html.push_str(&format!("<option value=\"{}\">{}</option>", i, esc(&label)));
    }
    sel.set_inner_html(&html);
}

/// Prefill X/Y from the first existing slicer (so consecutive creates
/// don't stack on top of each other) and show the modal.
pub(crate) fn open_slicer_modal(
    modal: &web_sys::Element,
    renderer: &SharedRenderer,
    sheets: &Sheets,
    active: &ActiveSheet,
) {
    populate_field_select(modal, sheets, active);
    render_slicer_list(modal, sheets, active);
    // Default X/Y offset so a second slicer doesn't land on top of the
    // first — each new slicer shifts right/down a bit.
    let dx = (sheets.borrow()[*active.borrow()].slicers.len() as f64) * 24.0;
    if let Ok(Some(e)) = modal.query_selector(".zs-slicer-x") {
        if let Ok(i) = e.dyn_into::<HtmlInputElement>() {
            i.set_value(&format!("{}", 200.0 + dx));
        }
    }
    if let Ok(Some(e)) = modal.query_selector(".zs-slicer-y") {
        if let Ok(i) = e.dyn_into::<HtmlInputElement>() {
            i.set_value(&format!("{}", 80.0 + dx));
        }
    }
    // Re-render the floating panels too — the user might have toggled
    // a chip while the modal was hidden, and the list is the only
    // surface that reflects the count.
    let _ = renderer; // (currently unused, kept for future pre-fill from selection)
    let _ = modal
        .unchecked_ref::<HtmlElement>()
        .style()
        .set_property("display", "block");
}

/// Wire the modal: close, create, delete. The floating-panel chip
/// click handler is installed separately in `wire_slicer_panel_events`
/// so re-rendering the panel doesn't have to re-attach anything.
pub(crate) fn wire_slicer_modal(
    modal: web_sys::Element,
    renderer: &SharedRenderer,
    sheets: &Sheets,
    active: &ActiveSheet,
    sheet_el: web_sys::Element,
    sync: SyncFn,
) {
    // Clone the shared handles into owned values for the modal's
    // `move` closure. The trailing `wire_slicer_panel_events` call
    // pulls fresh clones from the original `&` args below — the
    // `_local` names here are intentionally distinct so the function
    // args (`renderer`, `sheets`, `active`, `sync`) stay accessible
    // after the closure moves the locals.
    let renderer_local = renderer.clone();
    let sheets_local = sheets.clone();
    let active_local = active.clone();
    let sheet_el_local = sheet_el.clone();
    let modal_node = modal.clone();
    let sync_local = sync.clone();
    let mut el: Element = modal.into();
    el.add_event_listener("click", move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };

        // Close: any click on the header × or the bottom "Close" button.
        if elx.closest(".zs-slicer-close, .zs-slicer-done").ok().flatten().is_some() {
            let _ = modal_node
                .unchecked_ref::<HtmlElement>()
                .style()
                .set_property("display", "none");
            return;
        }

        // Delete: a row's Delete button, identified by data-slicer-del="i".
        if let Some(del) = elx.closest(&format!("[{}]", "data-slicer-del")).ok().flatten() {
            if let Some(idx) = del
                .get_attribute("data-slicer-del")
                .and_then(|v| v.parse::<usize>().ok())
            {
                let mut s = sheets_local.borrow_mut();
                let ai = *active_local.borrow();
                if ai < s.len() && idx < s[ai].slicers.len() {
                    s[ai].slicers.remove(idx);
                }
                drop(s);
                render_slicer_list(&modal_node, &sheets_local, &active_local);
                {
                    let mut r = renderer_local.borrow_mut();
                    r.refresh_pivots_on_source(&sheets_local, &active_local, *active_local.borrow());
                }
                render_slicer_panels(&sheet_el_local, &sheets_local, &active_local, &renderer_local);
                sync_local();
            }
            return;
        }

        // Create: build a Slicer from the form, push to the active source,
        // re-render the list and the floating panels, and recompute any
        // pivots on the source so they pick up the new (empty = "All")
        // slicer state immediately.
        if elx.closest(".zs-slicer-create").ok().flatten().is_some() {
            let field_idx = modal_node
                .query_selector(".zs-slicer-field")
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<HtmlSelectElement>().ok())
                .and_then(|s| s.value().parse::<usize>().ok())
                .unwrap_or(0);
            let x = read_num(&modal_node, ".zs-slicer-x", 200.0);
            let y = read_num(&modal_node, ".zs-slicer-y", 80.0);
            // Generate a stable id so re-renders are idempotent and a
            // workbook reload can find the same panel.
            let id = {
                let s = sheets_local.borrow();
                format!("slicer_{}", s[*active_local.borrow()].slicers.len())
            };
            {
                let mut s = sheets_local.borrow_mut();
                let ai = *active_local.borrow();
                if ai < s.len() {
                    s[ai].slicers.push(Slicer {
                        id,
                        field_idx,
                        selected_values: vec![],
                        x,
                        y,
                        width: 200.0,
                        height: 180.0,
                    });
                }
            }
            render_slicer_list(&modal_node, &sheets_local, &active_local);
            render_slicer_panels(&sheet_el_local, &sheets_local, &active_local, &renderer_local);
            {
                let mut r = renderer_local.borrow_mut();
                r.refresh_pivots_on_source(&sheets_local, &active_local, *active_local.borrow());
            }
            sync_local();
        }
    });

    // Install the floating-panel click handler on the sheet container.
    // Delegated on the parent so re-rendering the panel (which wipes
    // its inner HTML) doesn't have to re-attach anything. Fresh clones
    // from the original function args (the modal's click closure
    // already moved the `_local` versions above).
    wire_slicer_panel_events(
        sheet_el,
        renderer.clone(),
        sheets.clone(),
        active.clone(),
        sync.clone(),
    );
}

fn read_num(modal: &web_sys::Element, sel: &str, default: f64) -> f64 {
    modal
        .query_selector(sel)
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
        .and_then(|i| i.value().parse::<f64>().ok())
        .unwrap_or(default)
}

/// Build the floating slicer panels: one per `Slicer` on the active
/// sheet, appended to `sheet_el`. Idempotent — clears every existing
/// `[data-slicer-panel]` child first so the DOM is a clean reflection
/// of `sheets.borrow()[active].slicers`.
pub(crate) fn render_slicer_panels(
    sheet_el: &web_sys::Element,
    sheets: &Sheets,
    active: &ActiveSheet,
    _renderer: &SharedRenderer,
) {
    // Wipe existing panels. `query_selector_all` finds every descendant
    // with the attribute (panels are direct children of `sheet_el`, so
    // the result is the same as iterating direct children, and we don't
    // need a `dyn_into::<HtmlElement>()` to access `children()`).
    let to_remove: Vec<web_sys::Node> = {
        let mut out = Vec::new();
        if let Ok(all) = sheet_el.query_selector_all(&format!("[{}]", PANEL_ATTR)) {
            for i in 0..all.length() {
                if let Some(c) = all.item(i) {
                    // `query_selector_all` already filtered by the
                    // attribute, so every returned node is one of ours.
                    out.push(c);
                }
            }
        }
        out
    };
    for c in to_remove.iter() {
        let _ = sheet_el.remove_child(c);
    }

    let src = sheets.borrow()[*active.borrow()].clone();
    if src.slicers.is_empty() {
        return;
    }
    // Reuse the same headers we showed in the modal — they're the
    // source's row 0 cell text, used as field labels.
    let headers = read_header_labels(&src, src.row_count.max(1).min(64));

    for (i, slicer) in src.slicers.iter().enumerate() {
        let field_label = headers.get(slicer.field_idx).cloned().unwrap_or_default();
        let field_label = if field_label.is_empty() {
            format!("Column {}", slicer.field_idx + 1)
        } else {
            field_label
        };
        // Unique values in this column, sorted. The chip set is the
        // union of the source's actual values and the slicer's
        // selected_values — the union so a previously-selected value
        // that no longer appears in the source still shows as a chip
        // (and can be un-toggled), letting the user always reach the
        // "All" state.
        let mut all_values: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        for v in &slicer.selected_values {
            all_values.insert(v.clone());
        }
        // Walk the source column. Empty cells are dropped — the user
        // would only see them as noise, and the engine's "value not
        // in selected_values" predicate handles them either way (an
        // empty cell never matches a non-empty selection).
        for ri in 1..src.row_count {
            let v = src.cell_raw_value(ri, slicer.field_idx).trim().to_string();
            if !v.is_empty() {
                all_values.insert(v);
            }
        }
        let uniques: Vec<String> = all_values.into_iter().collect();

        let mut chips_html = String::new();
        if uniques.is_empty() {
            chips_html.push_str(
                "<div style=\"color:#999;font-size:11px;padding:4px 0;\">No values in this column.</div>",
            );
        }
        for v in &uniques {
            let selected = slicer.selected_values.contains(v);
            let bg = if selected { "#e8eef7" } else { "#fafafa" };
            let border = if selected { "#5577cc" } else { "#ccc" };
            let weight = if selected { "600" } else { "400" };
            // Encode the (slicer index, value) pair on the chip so the
            // delegated click handler can find it without keeping
            // per-chip state in JS.
            let encoded_value = html_attr_encode(v);
            chips_html.push_str(&format!(
                "<span {}=\"{}\" {}=\"{}\" \
                  style=\"display:inline-block;padding:2px 8px;margin:2px;\
                  border:1px solid {};border-radius:10px;background:{};\
                  cursor:pointer;font-size:11px;font-weight:{};\
                  user-select:none;\">{}</span>",
                CHIP_ATTR,
                i,
                "data-value",
                encoded_value,
                border,
                bg,
                weight,
                esc(v),
            ));
        }
        let title = format!("Slicer: {}", field_label);
        let panel_html = format!(
            "<div {}=\"{}\" \
              style=\"position:absolute;left:{}px;top:{}px;width:{}px;\
              background:#fff;border:1px solid #888;border-radius:4px;\
              box-shadow:rgba(0,0,0,0.18) 0px 2px 6px;\
              font-size:12px;z-index:150;\">\
              <div style=\"display:flex;align-items:center;\
                padding:4px 8px;background:#f0f3f9;\
                border-bottom:1px solid #d0d4dd;font-weight:600;\">\
                <span style=\"flex:1;\">{}</span>\
                <span {}=\"{}\" \
                  style=\"cursor:pointer;color:#666;\
                  font-size:11px;padding:1px 6px;margin-right:4px;\
                  border:1px solid #ccc;border-radius:3px;\
                  background:#fff;\" \
                  title=\"Clear selection (back to All)\">Clear</span>\
                <span {}=\"{}\" \
                  style=\"cursor:pointer;color:#999;font-weight:bold;\">✕</span>\
              </div>\
              <div style=\"max-height:{}px;overflow-y:auto;padding:4px 6px;\">{}</div>\
            </div>",
            PANEL_ATTR,
            slicer.id,
            slicer.x,
            slicer.y,
            slicer.width,
            esc(&title),
            CLEAR_ATTR,
            i,
            CLOSE_ATTR,
            i,
            // Chip list inner area: 180 - header(~30) = ~150.
            150.0_f64.max(slicer.height - 30.0),
            chips_html,
        );
        // Append the panel as a child of `sheet_el` without disturbing
        // its existing children (canvas, editor, contextmenu, prior
        // panels). `insert_adjacent_html` is the standard "append
        // parsed HTML" primitive in DOM.
        let _ = sheet_el.insert_adjacent_html("beforeend", &panel_html);
    }
}

/// Minimal HTML-attribute encoding for the chip's `data-value`. The
/// value can be any user-entered text, including quotes and
/// ampersands; the chip's text label goes through `esc` separately,
/// but the data attribute needs its own encode because attribute
/// parsing tolerates less than inner-HTML does. The chip's *index*
/// lives in a separate `data-slicer-chip` attribute so values that
/// contain colons don't have to be re-escaped inside a single packed
/// attribute.
fn html_attr_encode(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Install the delegated click handler on the sheet container. The
/// handler reads the chip's `data-slicer-chip="i:value"` attribute,
/// toggles the value in `sheets.borrow_mut()[active].slicers[i]`,
/// re-renders the panels, and recomputes the pivots on the source.
fn wire_slicer_panel_events(
    sheet_el: web_sys::Element,
    renderer: SharedRenderer,
    sheets: Sheets,
    active: ActiveSheet,
    sync: SyncFn,
) {
    // Clone for the closure so the trailing `add_event_listener_with_callback`
    // can still borrow the original `sheet_el` (it's used both inside the
    // chip handler and as the event-target container for the listener).
    let sheet_el_for_closure = sheet_el.clone();
    let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };

        // Chip click: read the index from `data-slicer-chip` and the
        // value from `data-value` (the two are kept as separate
        // attributes so values containing colons don't have to be
        // re-escaped inside a single packed attribute).
        if let Some(chip) = elx.closest(&format!("[{}]", CHIP_ATTR)).ok().flatten() {
            let idx = chip
                .get_attribute(CHIP_ATTR)
                .and_then(|v| v.parse::<usize>().ok());
            let value = chip.get_attribute("data-value");
            if let (Some(idx), Some(value)) = (idx, value) {
                let mut s = sheets.borrow_mut();
                let ai = *active.borrow();
                if ai < s.len() && idx < s[ai].slicers.len() {
                    // Toggle: if present, remove; if absent, add.
                    // `Vec::retain` keeps order; `push` adds to
                    // the end. Order doesn't matter for the
                    // engine, but stable order keeps the JSON
                    // deterministic.
                    if let Some(pos) = s[ai].slicers[idx]
                        .selected_values
                        .iter()
                        .position(|v| v == &value)
                    {
                        s[ai].slicers[idx].selected_values.remove(pos);
                    } else {
                        s[ai].slicers[idx].selected_values.push(value);
                    }
                }
                drop(s);
                // Re-render the panels so the chip's class
                // updates (selected vs unselected), then
                // recompute the pivots so the output sheet
                // reflects the new selection.
                render_slicer_panels(&sheet_el_for_closure, &sheets, &active, &renderer);
                {
                    let mut r = renderer.borrow_mut();
                    r.refresh_pivots_on_source(&sheets, &active, *active.borrow());
                }
                sync();
            }
            return;
        }

        // Clear: empty the selection on the named slicer.
        if let Some(clear) = elx.closest(&format!("[{}]", CLEAR_ATTR)).ok().flatten() {
            if let Some(idx) = clear
                .get_attribute(CLEAR_ATTR)
                .and_then(|v| v.parse::<usize>().ok())
            {
                let mut s = sheets.borrow_mut();
                let ai = *active.borrow();
                if ai < s.len() && idx < s[ai].slicers.len() {
                    s[ai].slicers[idx].selected_values.clear();
                }
                drop(s);
                render_slicer_panels(&sheet_el_for_closure, &sheets, &active, &renderer);
                {
                    let mut r = renderer.borrow_mut();
                    r.refresh_pivots_on_source(&sheets, &active, *active.borrow());
                }
                sync();
            }
            return;
        }

        // Close: hide the panel (doesn't delete the slicer — the user
        // can re-open via the modal's "Slicer" list). Stash a hidden
        // flag on the panel via a class.
        if let Some(close) = elx.closest(&format!("[{}]", CLOSE_ATTR)).ok().flatten() {
            if let Some(panel) = close.closest(&format!("[{}]", PANEL_ATTR)).ok().flatten() {
                if let Some(p) = panel.dyn_ref::<HtmlElement>() {
                    let _ = p.style().set_property("display", "none");
                }
            }
        }
    });
    sheet_el
        .add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())
        .unwrap();
    cb.forget();
}
