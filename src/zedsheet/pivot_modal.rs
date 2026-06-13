//! Insert-PivotTable dialog (issue #35).
//!
//! Excel-lite field list: source range (prefilled from selection), an
//! "available fields" list of chips built from the source's first row, and
//! three target zones (Rows / Columns / Values). A click on a zone header
//! makes it the active target; the next click on an available-field chip
//! moves it to the target zone. Each zone chip has a small `×` to remove
//! it. The Values zone has an aggregation select next to the value field.
//!
//! Drag-and-drop is deferred (issue #35 MVP); the click-to-move model
//! matches the modal-driven style of every other dialog in the app.

#[allow(unused_imports)]
use std::cell::RefCell;

use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlSelectElement};

use crate::component::element::Element;
use crate::core::data_proxy::SheetsRegistry;
use crate::core::cell_range::CellRange;
use crate::core::pivot::{Agg, PivotTable};
use crate::renderer::alphabets::xy2expr;
#[allow(unused_imports)]
use super::*;

/// Mutable state for the modal: tracks the current target zone and which
/// field indexes are in each zone. Persisted across re-renders of the
/// "available fields" list.
#[derive(Default, Clone)]
struct PivotState {
    target_zone: Zone,
    rows: Vec<usize>,
    cols: Vec<usize>,
    /// Multi-value (issue #59): one entry per value field, each holding
    /// the source field index and the aggregation. The chip UI shows one
    /// chip per entry, each with its own `<select>` for the aggregation.
    /// The save handler maps this list to `PivotTable::value_fields`.
    values: Vec<(usize, Agg)>,
    /// Page-level filters (issue #58). Each entry is a field index with
    /// the set of values currently checked (all-checked = "All", which
    /// is modeled as an empty list at the engine layer). The chip UI
    /// is identical to the other zones; the save handler inverts the
    /// "checked" set into the engine's `selected_values` form.
    filters: Vec<usize>,
    /// Last set of source unique values per filter field, so the
    /// checkbox list is stable across re-renders. Cached lazily by
    /// reading the source's header row + the field's column.
    filter_uniques: std::collections::HashMap<usize, Vec<String>>,
}

#[derive(Default, Clone, Copy, PartialEq)]
enum Zone {
    #[default]
    Rows,
    Columns,
    Values,
    /// Page-level Filters zone (issue #58).
    Filters,
}

impl Zone {
    fn label(self) -> &'static str {
        match self {
            Zone::Rows => "Rows",
            Zone::Columns => "Columns",
            Zone::Values => "Values",
            Zone::Filters => "Filters",
        }
    }
    fn attr(self) -> &'static str {
        match self {
            Zone::Rows => "rows",
            Zone::Columns => "cols",
            Zone::Values => "values",
            Zone::Filters => "filters",
        }
    }
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn pivot_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    let label = "width:110px;flex:none;";
    format!(
        r##"<div class="zedsheet-modal zs-pivot-root" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:600px;max-height:80vh;overflow-y:auto;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Insert PivotTable</span>
                <span class="zs-pivot-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
                <div style="{row}">
                    <label style="{label}">Source range</label>
                    <input class="zs-pivot-range" style="flex:1;padding:3px;" placeholder="A1:D12"/>
                </div>
                <div style="{row}">
                    <label style="{label}">Output sheet</label>
                    <input class="zs-pivot-output" style="flex:1;padding:3px;" placeholder="Pivot1"/>
                </div>
                <div style="display:flex;align-items:flex-start;gap:12px;margin-top:8px;">
                    <div style="flex:1;min-width:0;">
                        <div style="font-weight:600;margin-bottom:4px;">Available Fields</div>
                        <div class="zs-pivot-available" style="border:1px solid #e6e6e6;border-radius:4px;padding:6px;min-height:120px;"></div>
                    </div>
                    <div style="flex:1;min-width:0;">
                        <div style="display:flex;gap:6px;margin-bottom:4px;">
                            <button class="zs-pivot-zone" data-zone="rows" style="flex:1;padding:3px;cursor:pointer;">Rows</button>
                            <button class="zs-pivot-zone" data-zone="cols" style="flex:1;padding:3px;cursor:pointer;">Columns</button>
                            <button class="zs-pivot-zone" data-zone="values" style="flex:1;padding:3px;cursor:pointer;">Values</button>
                            <button class="zs-pivot-zone" data-zone="filters" style="flex:1;padding:3px;cursor:pointer;">Filters</button>
                        </div>
                        <div class="zs-pivot-zones" style="border:1px solid #e6e6e6;border-radius:4px;padding:6px;min-height:120px;"></div>
                        <div style="margin-top:6px;">
                            <span style="color:#999;font-size:11px;">Click a zone, then click a field chip to move it. Click × to remove.</span>
                        </div>
                    </div>
                </div>
                <div style="display:flex;justify-content:flex-end;gap:8px;margin-top:14px;">
                    <button class="zs-pivot-save" style="padding:4px 12px;cursor:pointer;">Create pivot</button>
                    <button class="zs-pivot-cancel" style="padding:4px 12px;cursor:pointer;">Cancel</button>
                </div>
            </div>
        </div>"##,
        row = row,
        label = label
    )
}

/// Re-render the available-fields list and the three zones from `state`.
/// `headers` is the source range's first row (column labels).
fn render_chips(
    modal: &web_sys::Element,
    state: &PivotState,
    headers: &[String],
) {
    let Ok(Some(avail)) = modal.query_selector(".zs-pivot-available") else { return };
    let Ok(Some(zones)) = modal.query_selector(".zs-pivot-zones") else { return };

    // Highlight the active target zone button.
    if let Ok(btns) = modal.query_selector_all(".zs-pivot-zone") {
        for i in 0..btns.length() {
            if let Some(b) = btns.item(i) {
                let bel: Option<&web_sys::Element> = b.dyn_ref();
                if let Some(bel) = bel {
                    if let Some(zone_attr) = bel.get_attribute("data-zone") {
                        let active = match (zone_attr.as_str(), state.target_zone) {
                            ("rows", Zone::Rows)
                            | ("cols", Zone::Columns)
                            | ("values", Zone::Values)
                            | ("filters", Zone::Filters) => true,
                            _ => false,
                        };
                        let _ = bel.unchecked_ref::<HtmlElement>().style().set_property(
                            "background",
                            if active { "#e8eef7" } else { "" },
                        );
                    }
                }
            }
        }
    }

    // Available: any field not currently in a zone.
    let mut used = vec![false; headers.len()];
    for &ci in state.rows.iter().chain(state.cols.iter()).chain(state.filters.iter()) {
        if ci < used.len() {
            used[ci] = true;
        }
    }
    // Every value field consumes its source column from the "available"
    // list (issue #59): a field is in exactly one zone at a time.
    for &(ci, _) in &state.values {
        if ci < used.len() {
            used[ci] = true;
        }
    }
    let mut html = String::new();
    let mut any = false;
    for (i, h) in headers.iter().enumerate() {
        if used[i] {
            continue;
        }
        any = true;
        html.push_str(&format!(
            "<span class=\"zs-pivot-chip zs-pivot-available-chip\" data-field=\"{}\" \
              draggable=\"true\" \
              style=\"display:inline-block;padding:2px 8px;margin:2px;border:1px solid #ccc;\
              border-radius:10px;background:#f7f7f7;cursor:grab;\">{}</span>",
            i,
            esc(h),
        ));
    }
    if !any {
        html.push_str(
            "<span style=\"color:#999;font-size:11px;\">All fields placed</span>",
        );
    }
    avail.set_inner_html(&html);

    // Zones
    let mut zh = String::new();
    zh.push_str(&format!("<div style=\"margin-bottom:4px;color:#999;font-size:11px;\">Rows</div>"));
    if state.rows.is_empty() {
        zh.push_str("<div style=\"color:#bbb;font-size:11px;margin-bottom:8px;\">(empty)</div>");
    } else {
        for &ci in &state.rows {
            zh.push_str(&chip_in_zone(ci, "rows", &headers[ci]));
        }
    }
    zh.push_str(&format!("<div style=\"margin:6px 0 4px;color:#999;font-size:11px;\">Columns</div>"));
    if state.cols.is_empty() {
        zh.push_str("<div style=\"color:#bbb;font-size:11px;margin-bottom:8px;\">(empty)</div>");
    } else {
        for &ci in &state.cols {
            zh.push_str(&chip_in_zone(ci, "cols", &headers[ci]));
        }
    }
    // Values zone (issue #59): one chip per value field, each with its
    // own agg `<select>` and a `data-value-index` so the agg-change and
    // remove handlers can target the right entry.
    zh.push_str(&format!("<div style=\"margin:6px 0 4px;color:#999;font-size:11px;\">Values</div>"));
    if state.values.is_empty() {
        zh.push_str("<div style=\"color:#bbb;font-size:11px;\">(empty)</div>");
    } else {
        for (v_idx, &(ci, agg)) in state.values.iter().enumerate() {
            let label = headers.get(ci).cloned().unwrap_or_default();
            zh.push_str(&format!(
                "<div data-field=\"{}\" data-zone=\"values\" data-value-index=\"{}\" \
                  class=\"zs-pivot-chip zs-pivot-zone-chip zs-pivot-value-chip\" \
                  style=\"display:inline-flex;align-items:center;gap:4px;padding:2px 6px;\
                  margin:2px;border:1px solid #ccc;border-radius:10px;background:#f7f7f7;\">\
                  <span>{} of {}</span>\
                  <select class=\"zs-pivot-agg\" data-value-index=\"{}\" \
                    style=\"font-size:11px;padding:1px;\">\
                    <option value=\"sum\"{}>Sum</option>\
                    <option value=\"count\"{}>Count</option>\
                    <option value=\"avg\"{}>Average</option>\
                    <option value=\"min\"{}>Min</option>\
                    <option value=\"max\"{}>Max</option>\
                  </select>\
                  <span class=\"zs-pivot-remove\" data-value-index=\"{}\" data-zone=\"values\" \
                    style=\"cursor:pointer;color:#999;font-weight:bold;\">×</span>\
                </div>",
                ci,
                v_idx,
                esc(&agg.label()),
                esc(&label),
                v_idx,
                sel(agg, Agg::Sum),
                sel(agg, Agg::Count),
                sel(agg, Agg::Avg),
                sel(agg, Agg::Min),
                sel(agg, Agg::Max),
                v_idx,
            ));
        }
    }
    // Filters zone (issue #58) — same chip model as Rows/Columns. The
    // saved_values is intentionally not shown in v1; an empty list at the
    // engine layer means "All", and the default is empty.
    zh.push_str(&format!(
        "<div style=\"margin:6px 0 4px;color:#999;font-size:11px;\">Filters</div>"
    ));
    if state.filters.is_empty() {
        zh.push_str(
            "<div style=\"color:#bbb;font-size:11px;margin-bottom:4px;\">(empty)</div>",
        );
    } else {
        for &ci in &state.filters {
            zh.push_str(&chip_in_zone(ci, "filters", &headers[ci]));
        }
    }
    zones.set_inner_html(&zh);
}

fn chip_in_zone(ci: usize, zone: &str, header: &str) -> String {
    format!(
        "<span class=\"zs-pivot-chip zs-pivot-zone-chip\" data-field=\"{}\" data-zone=\"{}\" \
          draggable=\"true\" \
          style=\"display:inline-flex;align-items:center;gap:4px;padding:2px 8px;margin:2px;\
          border:1px solid #ccc;border-radius:10px;background:#f7f7f7;cursor:grab;\">\
          <span>{}</span>\
          <span class=\"zs-pivot-remove\" data-field=\"{}\" data-zone=\"{}\" \
            style=\"cursor:pointer;color:#999;font-weight:bold;\">×</span>\
        </span>",
        ci, zone, esc(header), ci, zone,
    )
}

fn sel(actual: Agg, want: Agg) -> &'static str {
    if actual == want { " selected" } else { "" }
}

/// Prefill range from the live selection, default output name to "Pivot1"
/// (or "Pivot2" etc. first unused), and re-render the chips.
pub(crate) fn open_pivot_modal(
    modal: &web_sys::Element,
    renderer: &SharedRenderer,
    sheets: &SheetsRegistry,
) {
    let (r0, c0, r1, c1) = renderer.borrow().selection_bounds();
    if let Ok(Some(e)) = modal.query_selector(".zs-pivot-range") {
        if let Ok(i) = e.dyn_into::<HtmlInputElement>() {
            i.set_value(&CellRange::new(r0, c0, r1, c1).to_string());
        }
    }
    // Default output name: "Pivot1", or "Pivot2", … first unused.
    let used: Vec<String> = sheets.borrow().iter().map(|d| d.name.clone()).collect();
    let mut n = 1;
    loop {
        let candidate = format!("Pivot{}", n);
        if !used.contains(&candidate) {
            if let Ok(Some(e)) = modal.query_selector(".zs-pivot-output") {
                if let Ok(i) = e.dyn_into::<HtmlInputElement>() {
                    i.set_value(&candidate);
                }
            }
            break;
        }
        n += 1;
    }
    // Initial chip render with the current state.
    let state = STATE.with(|s| s.borrow().clone());
    let headers = read_source_headers(&renderer.borrow().data, &state);
    render_chips(modal, &state, &headers);
    let _ = modal
        .unchecked_ref::<HtmlElement>()
        .style()
        .set_property("display", "block");
}

thread_local! {
    static STATE: RefCell<PivotState> = RefCell::new(PivotState {
        target_zone: Zone::Rows,
        rows: vec![],
        cols: vec![],
        values: vec![],
        filters: vec![],
        filter_uniques: std::collections::HashMap::new(),
    });
}

/// Read the first row of the source range and return the column headers
/// (one per column). Used to label the available-field chips.
fn read_source_headers(data: &crate::core::data_proxy::DataProxy, _state: &PivotState) -> Vec<String> {
    // Best-effort: parse the source range from the input. If parsing fails
    // we return a single-column placeholder so the chips render (the user
    // will see no real labels until they fix the range).
    let range_str = || -> Option<String> {
        web_sys::window()
            .and_then(|w| w.document())
            .and_then(|d| {
                d.query_selector(".zs-pivot-range").ok().flatten()
            })
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
            .map(|i| i.value())
    };
    let raw = range_str().unwrap_or_default();
    let a1 = match raw.split_once('!') {
        Some((_s, rest)) => rest.to_string(),
        None => raw,
    };
    let cr = match CellRange::from_str(&a1) {
        Ok(c) => c,
        Err(()) => return vec!["?".to_string()],
    };
    let c0 = cr.sci.min(cr.eci);
    let c1 = cr.eci.max(cr.sci);
    let r0 = cr.sri.min(cr.eri);
    (c0..=c1)
        .map(|ci| {
            let s = data.cell_raw_value(r0, ci);
            let trimmed = s.trim();
            if trimmed.is_empty() {
                xy2expr(ci, r0) // cell ref like "B1"
            } else {
                trimmed.to_string()
            }
        })
        .collect()
}

/// Wire the dialog once: chip movement, zone selection, agg select, save,
/// close.
pub(crate) fn wire_pivot_modal(
    modal: web_sys::Element,
    renderer: &SharedRenderer,
    sheets: &SheetsRegistry,
    // `active` is shared by Rc so the save handler can read the *current*
    // active index at click time. If we captured a plain `usize` at wire
    // time instead, switching tabs between Open and Create pivot would
    // route the pivot against the wrong sheet and clobber the original
    // source (issue #52).
    active: ActiveSheet,
    sync: SyncFn,
    rerender_tabs: OpenHandle,
) {
    let modal_node = modal.clone();
    // Each listener below moves a clone of `modal_node`; keep the original
    // available for the multi-listener blocks further down.
    let modal_for_click = modal_node.clone();
    let renderer_click = renderer.clone(); // captured by the main click listener
    let sheets = sheets.clone();
    let mut el: Element = modal.into();
    el.add_event_listener("click", move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };

        // Close / Cancel
        if elx.closest(".zs-pivot-close, .zs-pivot-cancel").ok().flatten().is_some() {
            let _ = modal_for_click
                .unchecked_ref::<HtmlElement>()
                .style()
                .set_property("display", "none");
            return;
        }

        // Zone-button click: switch the target zone.
        if let Some(btn) = elx.closest(".zs-pivot-zone").ok().flatten() {
            if let Some(attr) = btn.get_attribute("data-zone") {
                STATE.with(|s| {
                    let mut st = s.borrow_mut();
                    st.target_zone = match attr.as_str() {
                        "rows" => Zone::Rows,
                        "cols" => Zone::Columns,
                        "values" => Zone::Values,
                        "filters" => Zone::Filters,
                        _ => Zone::Rows,
                    };
                });
                let state = STATE.with(|s| s.borrow().clone());
                let headers = read_source_headers(&renderer_click.borrow().data, &state);
                render_chips(&modal_for_click, &state, &headers);
            }
            return;
        }

        // Remove chip: return the field to "available". For value-zone
        // chips, `data-value-index` identifies which entry to drop; for
        // other zones, the chip's `data-field` is the field index. The
        // value-zone remove button is the only path that needs the
        // value-index attribute — all other zones are field-indexed.
        if let Some(rm) = elx.closest(".zs-pivot-remove").ok().flatten() {
            if let Some(vi_str) = rm.get_attribute("data-value-index") {
                if let Ok(v_idx) = vi_str.parse::<usize>() {
                    STATE.with(|s| {
                        let mut st = s.borrow_mut();
                        if v_idx < st.values.len() {
                            st.values.remove(v_idx);
                        }
                    });
                    let state = STATE.with(|s| s.borrow().clone());
                    let headers = read_source_headers(&renderer_click.borrow().data, &state);
                    render_chips(&modal_for_click, &state, &headers);
                }
                return;
            }
            if let Some(field_str) = rm.get_attribute("data-field") {
                if let Ok(ci) = field_str.parse::<usize>() {
                    STATE.with(|s| {
                        let mut st = s.borrow_mut();
                        st.rows.retain(|&x| x != ci);
                        st.cols.retain(|&x| x != ci);
                        st.filters.retain(|&x| x != ci);
                        st.values.retain(|&(v, _)| v != ci);
                    });
                    let state = STATE.with(|s| s.borrow().clone());
                    let headers = read_source_headers(&renderer_click.borrow().data, &state);
                    render_chips(&modal_for_click, &state, &headers);
                }
            }
            return;
        }

        // Available-field chip click: move to current target zone.
        if let Some(chip) = elx.closest(".zs-pivot-available-chip").ok().flatten() {
            if let Some(field_str) = chip.get_attribute("data-field") {
                if let Ok(ci) = field_str.parse::<usize>() {
                    let target = STATE.with(|s| s.borrow().target_zone);
                    STATE.with(|s| {
                        let mut st = s.borrow_mut();
                        // Make sure the field is only in one zone.
                        st.rows.retain(|&x| x != ci);
                        st.cols.retain(|&x| x != ci);
                        st.filters.retain(|&x| x != ci);
                        st.values.retain(|&(v, _)| v != ci);
                        match target {
                            Zone::Rows => {
                                if !st.rows.contains(&ci) { st.rows.push(ci); }
                            }
                            Zone::Columns => {
                                if !st.cols.contains(&ci) { st.cols.push(ci); }
                            }
                            Zone::Values => {
                                // Multi-value (issue #59): append another
                                // entry rather than replacing. The same
                                // field can appear once with each agg in
                                // v1; future iterations may want to
                                // dedup-with-update, but Excel's UI
                                // appends a new "Sum of X" chip per click.
                                st.values.push((ci, Agg::Sum));
                            }
                            // Filters (issue #58): every field in the
                            // Filters zone scopes which source rows are
                            // aggregated. The user's value selections
                            // default to "All" (empty engine-level list)
                            // — they aren't editable in v1.
                            Zone::Filters => {
                                if !st.filters.contains(&ci) { st.filters.push(ci); }
                            }
                        }
                    });
                    let state = STATE.with(|s| s.borrow().clone());
                    let headers = read_source_headers(&renderer_click.borrow().data, &state);
                    render_chips(&modal_for_click, &state, &headers);
                }
            }
            return;
        }
    });

    // Aggregation change (delegated `change`). The select carries a
    // `data-value-index` so the handler can update the right entry in
    // the multi-value list (issue #59). Without that attribute, the
    // change is ignored (no single-value state remains to update).
    let modal_for_agg = modal_node.clone();
    let el2_src = modal_node.clone();
    let mut el2: Element = Element::from(el2_src);
    el2.add_event_listener("change", move |event: web_sys::Event| {
        let _ = modal_for_agg; // captured
        let Some(target) = event.target() else { return };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
        if elx.closest(".zs-pivot-agg").ok().flatten().is_none() { return; }
        // Read the value-index attribute before we consume `elx` via
        // `dyn_into` (issue #59). Both reads target the same element —
        // `elx.clone()` keeps the original semantics of "use the
        // element-typed reference for closest lookups".
        let vi_str = elx.get_attribute("data-value-index");
        let Ok(sel) = elx.dyn_into::<HtmlSelectElement>() else { return };
        let agg = match sel.value().as_str() {
            "count" => Agg::Count,
            "avg" => Agg::Avg,
            "min" => Agg::Min,
            "max" => Agg::Max,
            _ => Agg::Sum,
        };
        if let Some(vi_str) = vi_str {
            if let Ok(v_idx) = vi_str.parse::<usize>() {
                STATE.with(|s| {
                    let mut st = s.borrow_mut();
                    if v_idx < st.values.len() {
                        st.values[v_idx].1 = agg;
                    }
                });
            }
        }
        let _ = modal_for_agg; // keep alive in closure
    });

    // Source range / output sheet change: re-render chips.
    {
        let modal_for_input = modal_node.clone();
        let renderer_for_input = renderer.clone();
        let mut el3: Element = Element::from(modal_node.clone());
        el3.add_event_listener("input", move |_event: web_sys::Event| {
            let state = STATE.with(|s| s.borrow().clone());
            let headers = read_source_headers(&renderer_for_input.borrow().data, &state);
            render_chips(&modal_for_input, &state, &headers);
        });
    }

    // HTML5 drag-and-drop (issue #57). A drop on `.zs-pivot-available`
    // returns the chip to the available list; a drop on `.zs-pivot-zones`
    // moves it to whichever zone is the current target. The
    // click-to-move path stays intact — drag is the upgrade.
    {
        let modal_for_drag = modal_node.clone();
        let renderer_for_drag = renderer.clone();
        let mut el4: Element = Element::from(modal_node.clone());
        el4.add_event_listener("dragstart", move |event: web_sys::Event| {
            let Some(target) = event.target() else { return };
            let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
            // The dragstart fires on the chip span (or its inner label);
            // climb to the closest chip.
            let Some(chip) = elx.closest(".zs-pivot-chip").ok().flatten() else { return };
            let Some(field_str) = chip.get_attribute("data-field") else { return };
            let de: web_sys::DataTransfer = event.unchecked_into();
            let _ = de.set_data("text/plain", &field_str);
        });
        el4.add_event_listener("dragover", move |event: web_sys::Event| {
            // dragover needs preventDefault to enable drop, on every
            // potential target.
            event.prevent_default();
        });
        el4.add_event_listener("drop", move |event: web_sys::Event| {
            event.prevent_default();
            let Some(target) = event.target() else { return };
            let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
            // The drop's dataTransfer carries the field_idx string.
            let de: web_sys::DataTransfer = event.unchecked_into();
            let field_str = de.get_data("text/plain").unwrap_or_default();
            let Ok(ci) = field_str.parse::<usize>() else { return };
            // The actual drop target — chip moved here. Find which
            // container the drop landed in.
            let in_available = elx.closest(".zs-pivot-available").ok().flatten().is_some();
            let in_zones = elx.closest(".zs-pivot-zones").ok().flatten().is_some();
            // Move the field:
            //   drop on available → remove from all zones
            //   drop on zones     → move to the current target zone
            //   (Values zone appends — issue #59 — so a single drag can
            //    add multiple aggregations of the same field.)
            STATE.with(|s| {
                let mut st = s.borrow_mut();
                st.rows.retain(|&x| x != ci);
                st.cols.retain(|&x| x != ci);
                st.filters.retain(|&x| x != ci);
                st.values.retain(|&(v, _)| v != ci);
                if in_zones && !in_available {
                    let target = st.target_zone;
                    match target {
                        Zone::Rows => {
                            if !st.rows.contains(&ci) { st.rows.push(ci); }
                        }
                        Zone::Columns => {
                            if !st.cols.contains(&ci) { st.cols.push(ci); }
                        }
                        Zone::Values => {
                            st.values.push((ci, Agg::Sum));
                        }
                        Zone::Filters => {
                            if !st.filters.contains(&ci) { st.filters.push(ci); }
                        }
                    }
                }
            });
            let state = STATE.with(|s| s.borrow().clone());
            let headers = read_source_headers(&renderer_for_drag.borrow().data, &state);
            render_chips(&modal_for_drag, &state, &headers);
        });
    }

    // Save.
    {
        let modal_for_save = modal_node.clone();
        let renderer_for_save = renderer.clone();
        let sheets_for_save = sheets.clone();
        let sync_for_save = sync.clone();
        let tabs_for_save = rerender_tabs.clone();
        let active_for_save = active.clone();
        let mut el4: Element = Element::from(modal_node.clone());
        el4.add_event_listener("click", move |event: web_sys::Event| {
            let Some(target) = event.target() else { return };
            let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
            if elx.closest(".zs-pivot-save").ok().flatten().is_none() { return; }

            let (range, output, state) = {
                let r = modal_for_save
                    .query_selector(".zs-pivot-range")
                    .ok()
                    .flatten()
                    .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                    .map(|i| i.value().trim().to_string())
                    .unwrap_or_default();
                let o = modal_for_save
                    .query_selector(".zs-pivot-output")
                    .ok()
                    .flatten()
                    .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                    .map(|i| i.value().trim().to_string())
                    .unwrap_or_default();
                let s = STATE.with(|st| st.borrow().clone());
                (r, o, s)
            };

            if range.is_empty() {
                pivot_alert("Source range is required.");
                return;
            }
            if output.is_empty() {
                pivot_alert("Output sheet name is required.");
                return;
            }
            // Multi-value (issue #59): require at least one value field.
            if state.values.is_empty() {
                pivot_alert("Pick at least one field for Values (click Values, then a chip).");
                return;
            }
            // Synthesize the legacy single-value pair from the first
            // value field so old readers (and the renderer's snapshot
            // path) still see a sensible `value_field`/`agg` even though
            // the engine always reads `effective_value_fields()`.
            let first_value = state.values[0];

            // Validate that the output name doesn't collide with the
            // source sheet (issue #51). The renderer also defensively
            // refuses this case, but the modal-level check gives a
            // cleaner error before any work happens.
            if output == renderer_for_save.borrow().data.name {
                pivot_alert(&format!(
                    "Output sheet name {:?} is the same as the source sheet — pick a different name.",
                    output
                ));
                return;
            }
            // Otherwise allow overwrite (e.g. re-running with the same
            // output name refreshes the pivot in place); add_pivot
            // handles the registry swap.

            // The live active index is read inside the save handler via
            // `active_for_save` (issue #52: a tab switch between Open and
            // Create pivot must not route against a stale sheet).
            let source_sheet = renderer_for_save.borrow().data.name.clone();
            // Filters zone (issue #58): every field in `state.filters`
            // becomes a `FilterField` with an empty `selected_values` —
            // the engine treats empty as "All", so v1 ships with the
            // non-filtering default. Editing the per-value checkboxes is
            // a follow-up.
            let filter_fields: Vec<crate::core::pivot::FilterField> = state
                .filters
                .iter()
                .map(|&field_idx| crate::core::pivot::FilterField {
                    field_idx,
                    selected_values: vec![],
                })
                .collect();
            // Multi-value (issue #59): the engine always reads
            // `effective_value_fields()`. When `state.values` is
            // non-empty (which the check above guarantees), the
            // synthesized list is authoritative; the legacy
            // `value_field`/`agg` pair is mirrored from the first entry
            // for backward compat with the snapshot/test code.
            let value_fields: Vec<crate::core::pivot::ValueField> = state
                .values
                .iter()
                .map(|&(field, agg)| crate::core::pivot::ValueField { field, agg })
                .collect();
            let pt = PivotTable {
                source_range: range,
                source_sheet,
                row_fields: state.rows.clone(),
                col_fields: state.cols.clone(),
                value_field: first_value.0,
                agg: first_value.1,
                value_fields,
                filter_fields,
                date_groups: std::collections::HashMap::new(),
                output_sheet: output,
            };

            {
                let mut r = renderer_for_save.borrow_mut();
                r.add_pivot(pt, &sheets_for_save, &active_for_save);
                r.render();
            }
            // Re-render the bottom-bar tabs so the new sheet appears.
            if let Some(f) = tabs_for_save.borrow().as_ref() {
                f();
            }
            sync_for_save();
            // Close the modal.
            let _ = modal_for_save
                .unchecked_ref::<HtmlElement>()
                .style()
                .set_property("display", "none");
            // Reset the per-mount state for next time.
            STATE.with(|s| {
                *s.borrow_mut() = PivotState {
                    target_zone: Zone::Rows,
                    rows: vec![],
                    cols: vec![],
                    values: vec![],
                    filters: vec![],
                    filter_uniques: std::collections::HashMap::new(),
                };
            });
        });
    }
}

fn pivot_alert(msg: &str) {
    if let Some(w) = web_sys::window() {
        let _ = w.alert_with_message(msg);
    }
}
