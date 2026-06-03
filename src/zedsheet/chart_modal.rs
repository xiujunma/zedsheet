//! Insert-chart dialog (issue #16): lists the sheet's charts and adds new
//! ones (bar / line / pie) bound to a data range and anchored at a cell.
//! Mirrors the conditional-formatting dialog (issue #11).

use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlSelectElement};
use crate::component::element::Element;
use crate::core::chart::Chart;
use crate::renderer::alphabets::xy2expr;
#[allow(unused_imports)]
use super::*;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

pub(crate) fn chart_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    let label = "width:90px;flex:none;";
    format!(
        r##"<div class="zedsheet-modal zs-chart-root" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:440px;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Charts</span>
                <span class="zs-chart-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
                <div class="zs-chart-list" style="max-height:120px;overflow-y:auto;margin-bottom:10px;"></div>
                <div style="border-top:1px solid #e6e6e6;margin-bottom:10px;"></div>
                <div style="{row}">
                    <label style="{label}">Type</label>
                    <select class="zs-chart-kind" style="flex:1;padding:3px;">
                        <option value="bar">Bar</option>
                        <option value="line">Line</option>
                        <option value="pie">Pie</option>
                    </select>
                </div>
                <div style="{row}">
                    <label style="{label}">Data range</label>
                    <input class="zs-chart-range" style="flex:1;padding:3px;" placeholder="A1:B4"/>
                </div>
                <div style="{row}">
                    <label style="{label}">Title</label>
                    <input class="zs-chart-title" style="flex:1;padding:3px;" placeholder="(optional)"/>
                </div>
                <div style="{row}">
                    <label style="{label}">Anchor cell</label>
                    <input class="zs-chart-anchor" style="width:80px;padding:3px;"/>
                    <span style="color:#999;">top-left corner of the chart</span>
                </div>
                <div style="display:flex;justify-content:flex-end;gap:8px;margin-top:10px;">
                    <button class="zs-chart-add" style="padding:4px 12px;cursor:pointer;">Insert chart</button>
                    <button class="zs-chart-done" style="padding:4px 12px;cursor:pointer;">Close</button>
                </div>
            </div>
        </div>"##,
        row = row,
        label = label
    )
}

/// Re-render the existing-charts list inside the dialog.
fn render_chart_list(modal: &web_sys::Element, renderer: &SharedRenderer) {
    let Ok(Some(list)) = modal.query_selector(".zs-chart-list") else { return };
    let charts: Vec<Chart> = renderer.borrow().data.charts.clone();
    if charts.is_empty() {
        list.set_inner_html("<div style=\"color:#999;padding:2px 0;\">No charts yet.</div>");
        return;
    }
    let mut html = String::new();
    for (i, ch) in charts.iter().enumerate() {
        let title = if ch.title.is_empty() { "(untitled)" } else { &ch.title };
        html.push_str(&format!(
            "<div style=\"display:flex;align-items:center;gap:8px;padding:3px 0;\">\
               <span style=\"flex:1;\">{} · {} · {} @ {}</span>\
               <button data-chartdel=\"{}\" style=\"padding:1px 8px;cursor:pointer;\">Delete</button>\
             </div>",
            esc(&ch.kind),
            esc(title),
            esc(&ch.range),
            esc(&ch.anchor),
            i
        ));
    }
    list.set_inner_html(&html);
}

/// Prefill range from the live selection and anchor just right of it, refresh
/// the list, and show.
pub(crate) fn open_chart_modal(modal: &web_sys::Element, renderer: &SharedRenderer) {
    let (r0, c0, r1, c1) = renderer.borrow().selection_bounds();
    let set = |sel: &str, v: &str| {
        if let Ok(Some(e)) = modal.query_selector(sel) {
            if let Ok(i) = e.dyn_into::<HtmlInputElement>() {
                i.set_value(v);
            }
        }
    };
    set(
        ".zs-chart-range",
        &crate::core::cell_range::CellRange::new(r0, c0, r1, c1).to_string(),
    );
    set(".zs-chart-anchor", &xy2expr(c1 + 2, r0));
    render_chart_list(modal, renderer);
    let _ = modal
        .unchecked_ref::<HtmlElement>()
        .style()
        .set_property("display", "block");
}

/// Wire the dialog once: close, insert, and per-chart delete.
pub(crate) fn wire_chart_modal(modal: web_sys::Element, renderer: &SharedRenderer, sync: &SyncFn) {
    let renderer = renderer.clone();
    let sync = sync.clone();
    let modal_node = modal.clone();
    let mut el: Element = modal.into();
    el.add_event_listener("click", move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
        let val = |sel: &str| -> String {
            modal_node
                .query_selector(sel)
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                .map(|i| i.value().trim().to_string())
                .unwrap_or_default()
        };

        if elx.closest(".zs-chart-close, .zs-chart-done").ok().flatten().is_some() {
            let _ = modal_node
                .unchecked_ref::<HtmlElement>()
                .style()
                .set_property("display", "none");
            return;
        }

        if let Some(del) = elx.closest("[data-chartdel]").ok().flatten() {
            if let Some(idx) = del.get_attribute("data-chartdel").and_then(|v| v.parse().ok()) {
                {
                    let mut r = renderer.borrow_mut();
                    r.remove_chart(idx);
                    r.render();
                }
                sync();
                render_chart_list(&modal_node, &renderer);
            }
            return;
        }

        if elx.closest(".zs-chart-add").ok().flatten().is_some() {
            let kind = modal_node
                .query_selector(".zs-chart-kind")
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<HtmlSelectElement>().ok())
                .map(|s| s.value())
                .unwrap_or_else(|| "bar".to_string());
            let chart = Chart {
                kind,
                range: val(".zs-chart-range"),
                title: val(".zs-chart-title"),
                anchor: val(".zs-chart-anchor"),
                width: 360.0,
                height: 220.0,
            };
            // Reject inputs that could never draw, so the dialog gives
            // immediate feedback.
            let range_ok = crate::core::cell_range::CellRange::from_str(&chart.range).is_ok();
            let anchor_ok =
                crate::formula::parser::looks_like_cell_ref(&chart.anchor);
            if !range_ok || !anchor_ok {
                if let Some(w) = web_sys::window() {
                    let _ = w.alert_with_message(
                        "Enter a valid data range (like A1:B4) and anchor cell (like F2).",
                    );
                }
                return;
            }
            {
                let mut r = renderer.borrow_mut();
                r.add_chart(chart);
                r.render();
            }
            sync();
            render_chart_list(&modal_node, &renderer);
        }
    });
}
