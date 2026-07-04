//! Conditional-formatting dialog (issue #11): lists the sheet's rules and adds
//! new ones (comparisons, text-contains, 2-color scales). Mirrors the Data
//! Validation modal (issue #9): mounted hidden at root, opened from the
//! context menu via a shared open-handle.

#[allow(unused_imports)]
use super::*;
use crate::component::element::Element;
use crate::core::cond_format::CondRule;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlSelectElement};

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
        "scale3" => "3-color scale",
        "databar" => "data bar",
        "icons" => "icon set",
        "top" => "top",
        "bottom" => "bottom",
        "above-avg" => "above average",
        "below-avg" => "below average",
        "dup" => "duplicate values",
        "unique" => "unique values",
        "formula" => "formula",
        _ => "?",
    }
}

pub(crate) fn cond_format_modal_html() -> String {
    let row = "display:flex;align-items:center;gap:8px;margin-bottom:8px;";
    let label = "width:90px;flex:none;";
    format!(
        r##"<div class="zedsheet-modal zs-cf-root" role="dialog" aria-modal="true" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:460px;">
            <div class="zedsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Conditional formatting</span>
                <span class="zs-cf-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="zedsheet-modal-content" style="padding:12px;">
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
                        <option value="top">Top N</option>
                        <option value="bottom">Bottom N</option>
                        <option value="above-avg">Above average</option>
                        <option value="below-avg">Below average</option>
                        <option value="dup">Duplicate values</option>
                        <option value="unique">Unique values</option>
                        <option value="formula">Custom formula</option>
                        <option value="scale2">2-color scale</option>
                        <option value="scale3">3-color scale</option>
                        <option value="databar">Data bar</option>
                        <option value="icons-arrows">Icon set: arrows</option>
                        <option value="icons-traffic">Icon set: traffic lights</option>
                    </select>
                </div>
                <div class="zs-cf-v1-row" style="{row}">
                    <label class="zs-cf-v1-label" style="{label}">Value</label>
                    <input class="zs-cf-v1" style="flex:1;padding:3px;"/>
                </div>
                <div class="zs-cf-v2-row" style="{row}display:none;">
                    <label class="zs-cf-v2-label" style="{label}">and</label>
                    <input class="zs-cf-v2" style="flex:1;padding:3px;"/>
                </div>
                <div class="zs-cf-v3-row" style="{row}display:none;">
                    <label class="zs-cf-v3-label" style="{label}">Max color</label>
                    <input class="zs-cf-v3" style="flex:1;padding:3px;"/>
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
    let Ok(Some(list)) = modal.query_selector(".zs-cf-list") else {
        return;
    };
    let rules: Vec<CondRule> = renderer.borrow().data.cond_formats.clone();
    if rules.is_empty() {
        list.set_inner_html("<div style=\"color:#999;padding:2px 0;\">No rules yet.</div>");
        return;
    }
    let mut html = String::new();
    for (i, r) in rules.iter().enumerate() {
        let what = match r.op.as_str() {
            "between" => format!("{} {} and {}", op_label(&r.op), esc(&r.v1), esc(&r.v2)),
            "scale2" => format!("{} {} → {}", op_label(&r.op), esc(&r.v1), esc(&r.v2)),
            "scale3" => format!(
                "{} {} → {} → {}",
                op_label(&r.op),
                esc(&r.v1),
                esc(&r.v2),
                esc(&r.v3)
            ),
            "icons" => format!("{} ({})", op_label(&r.op), esc(&r.v1)),
            "top" | "bottom" => format!("{} {}", op_label(&r.op), esc(&r.v1)),
            "databar" | "above-avg" | "below-avg" | "dup" | "unique" => op_label(&r.op).to_string(),
            _ => format!("{} {}", op_label(&r.op), esc(&r.v1)),
        };
        let swatch = r
            .bgcolor
            .clone()
            .or_else(|| (r.op == "scale2").then(|| r.v2.clone()))
            .or_else(|| (r.op == "scale3").then(|| r.v3.clone()))
            .unwrap_or_else(|| "#ffffff".into());
        html.push_str(&format!(
            "<div style=\"display:flex;align-items:center;gap:8px;padding:3px 0;\">\
               <span style=\"display:inline-block;width:14px;height:14px;border:1px solid #ccc;background:{};flex:none;\"></span>\
               <span style=\"flex:1;\">{} · {}</span>\
               <button data-cfmove=\"{}\" data-cfdir=\"up\" title=\"Move up\" style=\"padding:1px 6px;cursor:pointer;{};\">↑</button>\
               <button data-cfmove=\"{}\" data-cfdir=\"down\" title=\"Move down\" style=\"padding:1px 6px;cursor:pointer;{};\">↓</button>\
               <button data-cfdel=\"{}\" style=\"padding:1px 8px;cursor:pointer;\">Delete</button>\
             </div>",
            esc(&swatch),
            esc(&r.range),
            what,
            i,
            if i == 0 { "display:none;" } else { "" },
            i,
            if i + 1 == rules.len() { "display:none;" } else { "" },
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
                let set_val = |sel: &str, v: &str| {
                    if let Ok(Some(e)) = modal.query_selector(sel) {
                        if let Ok(i) = e.dyn_into::<HtmlInputElement>() {
                            i.set_value(v);
                        }
                    }
                };
                // Default layout: Value row only, style row on, no v2/v3.
                set_vis(".zs-cf-v1-row", true);
                set_vis(".zs-cf-v2-row", false);
                set_vis(".zs-cf-v3-row", false);
                set_vis(".zs-cf-style-row", true);
                set_text(".zs-cf-v1-label", "Value");
                // Re-seed the style defaults so a previous rule type's colors
                // (e.g. the data bar's blue fill) don't leak into this one.
                set_val(".zs-cf-bg", "#ffc7ce");
                set_val(".zs-cf-color", "#9c0006");
                match op.as_str() {
                    "between" => {
                        set_text(".zs-cf-v1-label", "From");
                        set_text(".zs-cf-v2-label", "To");
                        set_vis(".zs-cf-v2-row", true);
                    }
                    "scale2" => {
                        set_text(".zs-cf-v1-label", "Min color");
                        set_text(".zs-cf-v2-label", "Max color");
                        set_vis(".zs-cf-v2-row", true);
                        set_vis(".zs-cf-style-row", false);
                        set_val(".zs-cf-v1", "#ffffff");
                        set_val(".zs-cf-v2", "#1a73e8");
                    }
                    "scale3" => {
                        set_text(".zs-cf-v1-label", "Min color");
                        set_text(".zs-cf-v2-label", "Mid color");
                        set_vis(".zs-cf-v2-row", true);
                        set_vis(".zs-cf-v3-row", true);
                        set_vis(".zs-cf-style-row", false);
                        // Excel's red → yellow → green preset.
                        set_val(".zs-cf-v1", "#f8696b");
                        set_val(".zs-cf-v2", "#ffeb84");
                        set_val(".zs-cf-v3", "#63be7b");
                    }
                    // Data bar: only the Fill color applies (the bar color).
                    "databar" => {
                        set_vis(".zs-cf-v1-row", false);
                        set_val(".zs-cf-bg", "#638ec6");
                    }
                    // Icon sets need no operands or style at all.
                    "icons-arrows" | "icons-traffic" => {
                        set_vis(".zs-cf-v1-row", false);
                        set_vis(".zs-cf-style-row", false);
                    }
                    "top" | "bottom" => {
                        set_text(".zs-cf-v1-label", "N (or N%)");
                        set_val(".zs-cf-v1", "10");
                    }
                    "above-avg" | "below-avg" | "dup" | "unique" => {
                        set_vis(".zs-cf-v1-row", false);
                    }
                    "formula" => {
                        set_text(".zs-cf-v1-label", "Formula");
                        set_val(".zs-cf-v1", "");
                    }
                    _ => {}
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
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else {
            return;
        };
        let val = |sel: &str| -> String {
            modal_node
                .query_selector(sel)
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                .map(|i| i.value().trim().to_string())
                .unwrap_or_default()
        };

        if elx
            .closest(".zs-cf-close, .zs-cf-done")
            .ok()
            .flatten()
            .is_some()
        {
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

        // Per-rule ↑ / ↓ reorders (issue #29 follow-on). The first
        // matching rule wins during evaluation, so the order is
        // user-visible. Up arrow hides on row 0; down arrow hides on
        // the last row, but the boundary check is also in
        // `move_cond_rule` for safety.
        if let Some(mv) = elx.closest("[data-cfmove]").ok().flatten() {
            let idx = mv.get_attribute("data-cfmove").and_then(|v| v.parse().ok());
            let dir = mv.get_attribute("data-cfdir");
            if let (Some(i), Some(d)) = (idx, dir.as_deref()) {
                let signed = match d {
                    "up" => -1,
                    "down" => 1,
                    _ => 0,
                };
                if signed != 0 {
                    {
                        let mut r = renderer.borrow_mut();
                        r.move_cond_rule(i, signed);
                        r.render();
                    }
                    sync();
                    render_rules_list(&modal_node, &renderer);
                }
            }
            return;
        }

        if elx.closest(".zs-cf-add").ok().flatten().is_some() {
            let select_op = modal_node
                .query_selector(".zs-cf-op")
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<HtmlSelectElement>().ok())
                .map(|s| s.value())
                .unwrap_or_default();
            // The two icon-set menu entries share the `icons` op; the set
            // name travels in v1 (issue #29).
            let (op, mut v1) = match select_op.as_str() {
                "icons-arrows" => ("icons".to_string(), "arrows".to_string()),
                "icons-traffic" => ("icons".to_string(), "traffic".to_string()),
                _ => (select_op, val(".zs-cf-v1")),
            };
            let range = val(".zs-cf-range");
            // Scales and icon sets carry no cell style; a data bar uses the
            // Fill color as its bar color but no text style.
            let no_style = matches!(op.as_str(), "scale2" | "scale3" | "icons");
            let bar = op == "databar";
            if op == "formula" {
                v1 = v1.trim().to_string();
                if v1.trim_start_matches('=').trim().is_empty() {
                    if let Some(w) = web_sys::window() {
                        let _ = w.alert_with_message("Enter a formula like =B1>100.");
                    }
                    return;
                }
            }
            if matches!(op.as_str(), "top" | "bottom") {
                let n = v1.trim().trim_end_matches('%').trim();
                if n.parse::<f64>().map(|x| x <= 0.0).unwrap_or(true) {
                    if let Some(w) = web_sys::window() {
                        let _ = w.alert_with_message("Enter a positive N like 10 or 10%.");
                    }
                    return;
                }
            }
            let bold = modal_node
                .query_selector(".zs-cf-bold")
                .ok()
                .flatten()
                .and_then(|e| e.dyn_into::<HtmlInputElement>().ok())
                .map(|i| i.checked())
                .unwrap_or(false);
            // Only persist the operands the op actually uses, so hidden-field
            // leftovers from a previously selected rule type don't serialize.
            let uses_v1 = !matches!(
                op.as_str(),
                "databar" | "above-avg" | "below-avg" | "dup" | "unique"
            );
            let uses_v2 = matches!(op.as_str(), "between" | "scale2" | "scale3");
            let uses_v3 = op == "scale3";
            let rule = CondRule {
                range,
                op,
                v1: if uses_v1 { v1 } else { String::new() },
                v2: if uses_v2 {
                    val(".zs-cf-v2")
                } else {
                    String::new()
                },
                v3: if uses_v3 {
                    val(".zs-cf-v3")
                } else {
                    String::new()
                },
                bgcolor: (!no_style)
                    .then(|| val(".zs-cf-bg"))
                    .filter(|s| !s.is_empty()),
                color: (!no_style && !bar)
                    .then(|| val(".zs-cf-color"))
                    .filter(|s| !s.is_empty()),
                bold: bold && !no_style && !bar,
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
