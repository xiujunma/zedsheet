//! Conditional-formatting dialog (issue #11): lists the sheet's rules and adds
//! new ones (comparisons, text-contains, 2-color scales). Mirrors the Data
//! Validation modal (issue #9): mounted hidden at root, opened from the
//! context menu via a shared open-handle.

use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlSelectElement};
use crate::component::element::Element;
use crate::core::cond_format::CondRule;
#[allow(unused_imports)]
use super::*;

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Human label for a rule's operator.
fn op_label(op: &str) -> &'static str {
    match op {
        "gt" => "greater than",
        "ge" => "≥",
        "lt" => "less than",
        "le" => "≤",
        "eq" => "equal to",
        "between" => "between",
        "contains" => "text contains",
        "scale2" => "2-color scale",
        _ => "?",
    }
}

pub(crate) fn cond_format_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    let label = "width:90px;flex:none;";
    format!(
        r##"<div class="x-spreadsheet-modal zs-cf-root" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:460px;">
            <div class="x-spreadsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Conditional formatting</span>
                <span class="zs-cf-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="x-spreadsheet-modal-content" style="padding:12px;">
                <div class="zs-cf-list" style="max-height:130px;overflow-y:auto;margin-bottom:10px;"></div>
                <div style="border-top:1px solid #e6e6e6;margin-bottom:10px;"></div>
                <div style="{row}">
                    <label style="{label}">Range</label>
                    <input class="zs-cf-range" style="flex:1;padding:3px;" placeholder="B2:B10"/>
                </div>
                <div style="{row}">
                    <label style="{label}">Rule</label>
                    <select class="zs-cf-op" style="flex:1;padding:3px;">
                        <option value="gt">Greater than</option>
                        <option value="ge">Greater or equal</option>
                        <option value="lt">Less than</option>
                        <option value="le">Less or equal</option>
                        <option value="eq">Equal to</option>
                        <option value="between">Between</option>
                        <option value="contains">Text contains</option>
                        <option value="scale2">2-color scale</option>
                    </select>
                </div>
                <div style="{row}">
                    <label class="zs-cf-v1-label" style="{label}">Value</label>
                    <input class="zs-cf-v1" style="flex:1;padding:3px;"/>
                </div>
                <div class="zs-cf-v2-row" style="{row}display:none;">
                    <label class="zs-cf-v2-label" style="{label}">and</label>
                    <input class="zs-cf-v2" style="flex:1;padding:3px;"/>
                </div>
                <div class="zs-cf-style-row" style="{row}">
                    <label style="{label}">Style</label>
                    <label>Fill <input class="zs-cf-bg" style="width:70px;padding:3px;" value="#ffc7ce"/></label>
                    <label>Text <input class="zs-cf-color" style="width:70px;padding:3px;" value="#9c0006"/></label>
                    <label><input type="checkbox" class="zs-cf-bold"/> Bold</label>
                </div>
                <div style="display:flex;justify-content:flex-end;gap:8px;margin-top:10px;">
                    <button class="zs-cf-add" style="padding:4px 12px;cursor:pointer;">Add rule</button>
                    <button class="zs-cf-done" style="padding:4px 12px;cursor:pointer;">Close</button>
                </div>
            </div>
        </div>"##,
        row = row,
        label = label
    )
}

/// Re-render the existing-rules list inside the dialog.
fn render_rules_list(modal: &web_sys::Element, renderer: &SharedRenderer) {
    let Ok(Some(list)) = modal.query_selector(".zs-cf-list") else { return };
    let rules: Vec<CondRule> = renderer.borrow().data.cond_formats.clone();
    if rules.is_empty() {
        list.set_inner_html("<div style=\"color:#999;padding:2px 0;\">No rules yet.</div>");
        return;
    }
    let mut html = String::new();
    for (i, r) in rules.iter().enumerate() {
        let what = if r.op == "between" {
            format!("{} {} and {}", op_label(&r.op), esc(&r.v1), esc(&r.v2))
        } else if r.op == "scale2" {
            format!("{} {} → {}", op_label(&r.op), esc(&r.v1), esc(&r.v2))
        } else {
            format!("{} {}", op_label(&r.op), esc(&r.v1))
        };
        let swatch = r
            .bgcolor
            .clone()
            .or_else(|| (r.op == "scale2").then(|| r.v2.clone()))
            .unwrap_or_else(|| "#ffffff".into());
        html.push_str(&format!(
            "<div style=\"display:flex;align-items:center;gap:8px;padding:3px 0;\">\
               <span style=\"display:inline-block;width:14px;height:14px;border:1px solid #ccc;background:{};flex:none;\"></span>\
               <span style=\"flex:1;\">{} · {}</span>\
               <button data-cfdel=\"{}\" style=\"padding:1px 8px;cursor:pointer;\">Delete</button>\
             </div>",
            esc(&swatch),
            esc(&r.range),
            what,
            i
        ));
    }
    list.set_inner_html(&html);
}

/// Prefill the range from the selection, refresh the rules list, and show.
pub(crate) fn open_cf_modal(modal: &web_sys::Element, renderer: &SharedRenderer) {
    if let Ok(Some(range_input)) = modal.query_selector(".zs-cf-range") {
        if let Ok(input) = range_input.dyn_into::<HtmlInputElement>() {
            // The renderer's selection is the live one (the data selector can
            // lag it), so prefill from there.
            let (r0, c0, r1, c1) = renderer.borrow().selection_bounds();
            input.set_value(&crate::core::cell_range::CellRange::new(r0, c0, r1, c1).to_string());
        }
    }
    render_rules_list(modal, renderer);
    let _ = modal
        .unchecked_ref::<HtmlElement>()
        .style()
        .set_property("display", "block");
}

/// Wire the dialog once: close buttons, the value-label swap for
/// between/scale rules, Add, and per-rule Delete.
pub(crate) fn wire_cond_format_modal(
    modal: web_sys::Element,
    renderer: &SharedRenderer,
    sync: &SyncFn,
) {
    // Rule-type change retitles the value inputs and shows/hides Value 2.
    {
        let modal = modal.clone();
        if let Ok(Some(op_el)) = modal.clone().query_selector(".zs-cf-op") {
            let mut op_node: Element = op_el.into();
            op_node.add_event_listener("change", move |_e: web_sys::Event| {
                let op = modal
                    .query_selector(".zs-cf-op")
                    .ok()
                    .flatten()
                    .and_then(|e| e.dyn_into::<HtmlSelectElement>().ok())
                    .map(|s| s.value())
                    .unwrap_or_default();
                let set_text = |sel: &str, text: &str| {
                    if let Ok(Some(e)) = modal.query_selector(sel) {
                        e.set_text_content(Some(text));
                    }
                };
                let set_vis = |sel: &str, show: bool| {
                    if let Ok(Some(e)) = modal.query_selector(sel) {
                        let _ = e
                            .unchecked_ref::<HtmlElement>()
                            .style()
                            .set_property("display", if show { "flex" } else { "none" });
                    }
                };
                match op.as_str() {
                    "between" => {
                        set_text(".zs-cf-v1-label", "From");
                        set_text(".zs-cf-v2-label", "To");
                        set_vis(".zs-cf-v2-row", true);
                        set_vis(".zs-cf-style-row", true);
                    }
                    "scale2" => {
                        set_text(".zs-cf-v1-label", "Min color");
                        set_text(".zs-cf-v2-label", "Max color");
                        set_vis(".zs-cf-v2-row", true);
                        set_vis(".zs-cf-style-row", false);
                        for (sel, v) in [(".zs-cf-v1", "#ffffff"), (".zs-cf-v2", "#1a73e8")] {
                            if let Ok(Some(e)) = modal.query_selector(sel) {
                                if let Ok(i) = e.dyn_into::<HtmlInputElement>() {
                                    i.set_value(v);
                                }
                            }
                        }
                    }
                    _ => {
                        set_text(".zs-cf-v1-label", "Value");
                        set_vis(".zs-cf-v2-row", false);
                        set_vis(".zs-cf-style-row", true);
                    }
                }
            });
        }
    }

    // Delegated clicks: close, add, delete.
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

        if elx.closest(".zs-cf-close, .zs-cf-done").ok().flatten().is_some() {
            let _ = modal_node
                .unchecked_ref::<HtmlElement>()
                .style()
                .set_property("display", "none");
            return;
        }

        if let Some(del) = elx.closest("[data-cfdel]").ok().flatten() {
            if let Some(idx) = del.get_attribute("data-cfdel").and_then(|v| v.parse().ok()) {
                {
                    let mut r = renderer.borrow_mut();
                    r.remove_cond_rule(idx);
                    r.render();
                }
                sync();
                render_rules_list(&modal_node, &renderer);
            }
            return;
        }

        if elx.closest(".zs-cf-add").ok().flatten().is_some() {
            let op = modal_node
                .query_selector(".zs-cf-op")
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<HtmlSelectElement>().ok())
                .map(|s| s.value())
                .unwrap_or_default();
            let range = val(".zs-cf-range");
            let scale = op == "scale2";
            let bold = modal_node
                .query_selector(".zs-cf-bold")
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                .map(|i| i.checked())
                .unwrap_or(false);
            let rule = CondRule {
                range,
                op,
                v1: val(".zs-cf-v1"),
                v2: val(".zs-cf-v2"),
                bgcolor: (!scale).then(|| val(".zs-cf-bg")).filter(|s| !s.is_empty()),
                color: (!scale).then(|| val(".zs-cf-color")).filter(|s| !s.is_empty()),
                bold: bold && !scale,
            };
            // A rule with an unparsable range can never match — reject it
            // here so the dialog gives immediate feedback.
            if rule.bounds().is_none() {
                if let Some(w) = web_sys::window() {
                    let _ = w.alert_with_message("Enter a valid range like B2:B10.");
                }
                return;
            }
            {
                let mut r = renderer.borrow_mut();
                r.add_cond_rule(rule);
                r.render();
            }
            sync();
            render_rules_list(&modal_node, &renderer);
        }
    });
}
