use std::cell::RefCell;
use std::rc::Rc;
use gloo::utils::window;
use wasm_bindgen::closure::Closure;
use web_sys::KeyboardEvent;
#[allow(unused_imports)]
use super::*;

pub(crate) fn show_list_popover(
    popover: Option<&web_sys::Element>,
    renderer: &SharedRenderer,
    ri: usize,
    ci: usize,
    x: f64,
    y: f64,
    visible_flag: &Rc<RefCell<bool>>,
) {
    use wasm_bindgen::JsCast;
    let Some(pop) = popover else { return };
    let values = renderer.borrow().list_values_for_cell(ri, ci);
    let Some(values) = values else { return };
    // Build the <li> items.
    let mut html = String::new();
    for v in &values {
        let escaped = v
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;");
        html.push_str(&format!(
            "<li data-value=\"{}\" style=\"padding:4px 14px;cursor:pointer;\">{}</li>",
            escaped, escaped
        ));
    }
    pop.set_inner_html(&html);
    let _ = pop.set_attribute(
        "data-cell",
        &format!("{}_{}", ri, ci),
    );
    // Position the popover at (x, y). If it would overflow the viewport
    // bottom, flip it above the click point.
    let vh = web_sys::window()
        .and_then(|w| w.inner_height().ok().and_then(|v| v.as_f64()))
        .unwrap_or(800.0);
    let top = if y + 200.0 > vh { (y - 24.0).max(0.0) } else { y };
    let style = pop.unchecked_ref::<web_sys::HtmlElement>().style();
    let _ = style.set_property("left", &format!("{}px", x));
    let _ = style.set_property("top", &format!("{}px", top));
    let _ = style.set_property("display", "block");
    *visible_flag.borrow_mut() = true;
}

pub(crate) fn data_validation_modal_html() -> String {
    format!(
        r#"<div class="x-spreadsheet-modal zs-dv-root" role="dialog" aria-modal="true" aria-labelledby="zs-dv-title" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:420px;">
            <div class="x-spreadsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span id="zs-dv-title">Data Validation</span>
                <span class="x-spreadsheet-icon zs-dv-close" role="button" tabindex="0" aria-label="Close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="x-spreadsheet-modal-content" style="padding:12px;">
                <div style="display:flex;align-items:center;margin-bottom:8px;">
                    <label for="zs-dv-type-sel" style="width:90px;">Allow</label>
                    <select id="zs-dv-type-sel" class="zs-dv-type" style="flex:1;padding:3px;">
                        <option value="">Any value</option>
                        <option value="list">List</option>
                        <option value="number">Number</option>
                        <option value="text-length">Text length</option>
                        <option value="email">Email</option>
                        <option value="phone">Phone</option>
                    </select>
                </div>
                <div class="zs-dv-op-row" style="display:none;align-items:center;margin-bottom:8px;">
                    <label for="zs-dv-op-sel" style="width:90px;">Operator</label>
                    <select id="zs-dv-op-sel" class="zs-dv-op" style="flex:1;padding:3px;">
                        <option value="be">between</option>
                        <option value="nbe">not between</option>
                        <option value="eq">equal to</option>
                        <option value="neq">not equal to</option>
                        <option value="lt">less than</option>
                        <option value="lte">less than or equal to</option>
                        <option value="gt">greater than</option>
                        <option value="gte">greater than or equal to</option>
                    </select>
                </div>
                <div class="zs-dv-val-row" style="display:none;align-items:center;margin-bottom:8px;">
                    <label for="zs-dv-val1-in" style="width:90px;" class="zs-dv-val1-label">Value</label>
                    <input id="zs-dv-val1-in" class="zs-dv-val1 zs-dv-val" type="text" style="flex:1;padding:3px;box-sizing:border-box;" />
                    <span class="zs-dv-to" style="margin:0 6px;display:none;">to</span>
                    <input class="zs-dv-val2 zs-dv-val" type="text" aria-label="Upper bound" style="flex:1;display:none;padding:3px;box-sizing:border-box;" />
                </div>
                <div class="zs-dv-list-row" style="display:none;margin-bottom:8px;">
                    <label for="zs-dv-list-in" style="display:block;margin-bottom:4px;">Source (comma-separated, e.g. Yes,No,Maybe)</label>
                    <textarea id="zs-dv-list-in" class="zs-dv-list" rows="3" style="width:100%;padding:4px;box-sizing:border-box;font-family:inherit;"></textarea>
                </div>
                <div style="display:flex;align-items:center;margin-bottom:8px;">
                    <label style="width:90px;">&nbsp;</label>
                    <label style="display:flex;align-items:center;">
                        <input type="checkbox" class="zs-dv-req" /> &nbsp;Treat empty as invalid
                    </label>
                </div>
                <div style="display:flex;align-items:center;margin-bottom:12px;">
                    <label for="zs-dv-ref-in" style="width:90px;">Apply to</label>
                    <input id="zs-dv-ref-in" class="zs-dv-ref" type="text" style="flex:1;padding:3px;box-sizing:border-box;" />
                </div>
                <div style="display:flex;gap:6px;justify-content:flex-end;margin-top:8px;">
                    <button class="zs-dv-cancel" style="padding:4px 14px;">Cancel</button>
                    <button class="zs-dv-save" style="padding:4px 14px;background:#4b89ff;color:#fff;border:0;border-radius:3px;">Save</button>
                </div>
            </div>
        </div>"#,
    )
}

/// Wire the Data Validation modal: type-change toggles operator/value
/// rows, Save commits a `Validator` to the renderer's `validations`,
/// Cancel/close-icon/outside-click/Escape hide. Returns a handle that
/// the context menu can use to open the modal.
pub(crate) fn wire_data_validation_modal(
    modal_node: web_sys::Element,
    renderer: &SharedRenderer,
) -> Rc<RefCell<bool>> {
    use wasm_bindgen::JsCast;
    // Resolve the inner `.zs-dv-root` once — the wrapper passed in has
    // an inline `display:none` on its child, not itself.
    let inner_modal: web_sys::Element = modal_node
        .query_selector(".zs-dv-root")
        .ok()
        .flatten()
        .unwrap_or_else(|| modal_node.clone());
    let visible: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
    let set_visible = move |v: bool, modal: &web_sys::Element| {
        let _ = modal
            .unchecked_ref::<web_sys::HtmlElement>()
            .style()
            .set_property("display", if v { "block" } else { "none" });
    };

    // Type change: show/hide operator row, value row, list row.
    if let Ok(Some(type_select)) = modal_node.query_selector(".zs-dv-type") {
        let modal_for_type = modal_node.clone();
        let type_select_for_cb = type_select.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let type_val = type_select_for_cb
                .unchecked_ref::<web_sys::HtmlInputElement>()
                .value();
            update_dv_rows(&modal_for_type, &type_val);
        });
        let _ = type_select.add_event_listener_with_callback(
            "change",
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }

    // Operator change: re-run update_dv_rows so the "to" / val2 fields
    // appear when "between" / "not between" is selected and disappear
    // for the single-value operators.
    if let Ok(Some(op_select)) = modal_node.query_selector(".zs-dv-op") {
        let modal_for_op = modal_node.clone();
        let op_select_for_cb = op_select.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            // Read the select's value by finding the selected option's value.
            let op_val = op_select_for_cb
                .query_selector("option:checked")
                .ok()
                .flatten()
                .and_then(|e| e.get_attribute("value"))
                .unwrap_or_default();
            let type_val = modal_for_op
                .query_selector(".zs-dv-type")
                .ok()
                .flatten()
                .and_then(|e| e.query_selector("option:checked").ok().flatten())
                .and_then(|e| e.get_attribute("value"))
                .unwrap_or_default();
            update_dv_rows_with_op(&modal_for_op, &type_val, &op_val);
        });
        let _ = op_select.add_event_listener_with_callback(
            "change",
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }

    // Save button.
    if let Ok(Some(save_btn)) = modal_node.query_selector(".zs-dv-save") {
        let modal_for_save = inner_modal.clone();
        let renderer_for_save = renderer.clone();
        let visible_for_save = visible.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            handle_dv_save(&modal_for_save, &renderer_for_save);
            *visible_for_save.borrow_mut() = false;
            let _ = modal_for_save
                .unchecked_ref::<web_sys::HtmlElement>()
                .style()
                .set_property("display", "none");
        });
        let _ = save_btn.add_event_listener_with_callback(
            "click",
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }

    // Cancel button.
    if let Ok(Some(cancel_btn)) = modal_node.query_selector(".zs-dv-cancel") {
        let modal_for_cancel = inner_modal.clone();
        let visible_for_cancel = visible.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let _ = modal_for_cancel
                .unchecked_ref::<web_sys::HtmlElement>()
                .style()
                .set_property("display", "none");
            *visible_for_cancel.borrow_mut() = false;
        });
        let _ = cancel_btn.add_event_listener_with_callback(
            "click",
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }

    // Close icon.
    if let Ok(Some(close_icon)) = modal_node.query_selector(".zs-dv-close") {
        let modal_for_close = inner_modal.clone();
        let visible_for_close = visible.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            let _ = modal_for_close
                .unchecked_ref::<web_sys::HtmlElement>()
                .style()
                .set_property("display", "none");
            *visible_for_close.borrow_mut() = false;
        });
        let _ = close_icon.add_event_listener_with_callback(
            "click",
            cb.as_ref().unchecked_ref(),
        );
        cb.forget();
    }

    // Outside click: close the modal if the click is outside it.
    {
        let modal_for_outside = inner_modal.clone();
        let visible_for_outside = visible.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if !*visible_for_outside.borrow() {
                return;
            }
            let target = event.target();
            let Some(target_el) = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
                return;
            };
            if !modal_for_outside.contains(Some(&target_el)) {
                let _ = modal_for_outside
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                *visible_for_outside.borrow_mut() = false;
            }
        });
        let _ = window()
            .add_event_listener_with_callback("mousedown", cb.as_ref().unchecked_ref());
        cb.forget();
    }

    // Escape: close the modal if open.
    {
        let modal_for_esc = inner_modal.clone();
        let visible_for_esc = visible.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if !*visible_for_esc.borrow() {
                return;
            }
            let ke: KeyboardEvent = event.dyn_into().unwrap();
            if ke.key() == "Escape" {
                let _ = modal_for_esc
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
                *visible_for_esc.borrow_mut() = false;
            }
        });
        let _ = window()
            .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref());
        cb.forget();
    }

    // Suppress unused-variable warning for the `set_visible` helper
    // (kept for future use; the direct style writes above are equivalent).
    let _ = set_visible;
    visible
}

/// Show/hide the operator, value, and list rows in the DV modal based on
/// the chosen type.
pub(crate) fn update_dv_rows(modal: &web_sys::Element, type_val: &str) {
    // Read the current operator from the DOM (fallback for callers that
    // don't pass it explicitly).
    let op_val = modal
        .query_selector(".zs-dv-op option:checked")
        .ok()
        .flatten()
        .and_then(|e| e.get_attribute("value"))
        .unwrap_or_default();
    update_dv_rows_with_op(modal, type_val, &op_val);
}

pub(crate) fn update_dv_rows_with_op(modal: &web_sys::Element, type_val: &str, op_val: &str) {
    use wasm_bindgen::JsCast;
    let set_row = |q: &str, display: &str| {
        if let Ok(Some(el)) = modal.query_selector(q) {
            let _ = el.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", display);
        }
    };
    // Default: all hidden.
    set_row(".zs-dv-op-row", "none");
    set_row(".zs-dv-val-row", "none");
    set_row(".zs-dv-list-row", "none");
    set_row(".zs-dv-to", "none");
    set_row(".zs-dv-val2", "none");
    match type_val {
        "list" => set_row(".zs-dv-list-row", "block"),
        "number" | "text-length" => {
            set_row(".zs-dv-op-row", "flex");
            set_row(".zs-dv-val-row", "flex");
            // The "to" label and second value input only show for between /
            // not-between operators.
            if op_val == "be" || op_val == "nbe" {
                set_row(".zs-dv-to", "inline");
                set_row(".zs-dv-val2", "block");
            }
        }
        _ => {} // empty / email / phone: only the type dropdown is meaningful
    }
}

/// Build a `Validator` from the modal's current input values and commit
/// it to the renderer's `validations` for the chosen ref.
pub(crate) fn handle_dv_save(modal: &web_sys::Element, renderer: &SharedRenderer) {
    use wasm_bindgen::JsCast;
    let value_of = |q: &str| -> String {
        modal
            .query_selector(q)
            .ok()
            .flatten()
            .and_then(|e| {
                if let Some(input) = e.dyn_ref::<web_sys::HtmlInputElement>() {
                    Some(input.value())
                } else if let Some(text) = e.dyn_ref::<web_sys::HtmlTextAreaElement>() {
                    Some(text.value())
                } else {
                    // <select>: read the currently selected option's value.
                    e.query_selector("option:checked")
                        .ok()
                        .flatten()
                        .and_then(|opt| opt.get_attribute("value"))
                }
            })
            .unwrap_or_default()
    };
    let text_of = |q: &str| -> String {
        modal
            .query_selector(q)
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<web_sys::HtmlTextAreaElement>().ok())
            .map(|i| i.value())
            .unwrap_or_default()
    };
    let checked = |q: &str| -> bool {
        modal
            .query_selector(q)
            .ok()
            .flatten()
            .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
            .map(|i| i.checked())
            .unwrap_or(false)
    };

    let type_val = value_of(".zs-dv-type");
    let op_val = value_of(".zs-dv-op");
    let val1 = value_of(".zs-dv-val1");
    let val2 = value_of(".zs-dv-val2");
    let list_csv = text_of(".zs-dv-list");
    let required = checked(".zs-dv-req");
    let ref_str = value_of(".zs-dv-ref");

    use crate::core::validation::Validator;
    let mut r = renderer.borrow_mut();
    if ref_str.trim().is_empty() {
        return; // ignore: no target range
    }
    if type_val.is_empty() {
        // "Any value" → clear any validator on the ref.
        r.clear_validations_in_range(&ref_str);
    } else if type_val == "list" {
        let csv = list_csv.trim().to_string();
        let v = Validator::new("list", required, &csv, "");
        r.set_validations_for_range(&ref_str, v);
    } else if type_val == "number" || type_val == "text-length" {
        let value = if op_val == "be" || op_val == "nbe" {
            format!("{},{}", val1.trim(), val2.trim())
        } else {
            val1.trim().to_string()
        };
        let v = Validator::new(&type_val, required, &value, &op_val);
        r.set_validations_for_range(&ref_str, v);
    } else {
        // email / phone — no operator, no value.
        let v = Validator::new(&type_val, required, "", "");
        r.set_validations_for_range(&ref_str, v);
    }
    r.render();
}

/// Open the Data Validation modal, pre-filling the fields from any
/// existing validator at the top-left of the current selection.
pub(crate) fn open_dv_modal(
    modal: &web_sys::Element,
    renderer: &SharedRenderer,
    visible: &Rc<RefCell<bool>>,
) {
    use crate::renderer::alphabets::xy2expr;
    use wasm_bindgen::JsCast;
    let (ref_str, existing) = {
        let r = renderer.borrow();
        let s = r.get_selector();
        let ref_str = if s.ri == s.eri && s.ci == s.eci {
            xy2expr(s.ci, s.ri)
        } else {
            format!(
                "{}:{}",
                xy2expr(s.ci.min(s.eci), s.ri.min(s.eri)),
                xy2expr(s.ci.max(s.eci), s.ri.max(s.eri))
            )
        };
        let existing = r
            .data
            .validations
            .get(s.ri, s.ci)
            .map(|v| v.validator.clone());
        (ref_str, existing)
    };
    let set_input = |q: &str, v: &str| {
        if let Ok(Some(el)) = modal.query_selector(q) {
            if let Some(input) = el.dyn_ref::<web_sys::HtmlInputElement>() {
                input.set_value(v);
            } else if let Some(text) = el.dyn_ref::<web_sys::HtmlTextAreaElement>() {
                text.set_value(v);
            } else {
                // <select>: set the `selected` attribute on the matching
                // option so the select's `.value` reflects our choice.
                let target_q = format!("option[value=\"{}\"]", v);
                if let Ok(Some(opt)) = el.query_selector(&target_q) {
                    // Clear selected from siblings first so the new
                    // selection takes effect.
                    if let Ok(opts) = el.query_selector_all("option") {
                        for i in 0..opts.length() {
                            if let Some(o) = opts.get(i) {
                                let oe = o.dyn_into::<web_sys::Element>();
                                if let Ok(oe) = oe {
                                    let _ = oe.remove_attribute("selected");
                                }
                            }
                        }
                    }
                    let _ = opt.set_attribute("selected", "");
                }
                // Dispatch a synthetic change so any row-visibility
                // listener (the operator/value/list rows) fires.
                let _ = el.dispatch_event(&web_sys::Event::new("change").ok().unwrap());
            }
        }
    };
    set_input(".zs-dv-ref", &ref_str);
    if let Some(v) = existing {
        set_input(".zs-dv-type", &v.type_);
        set_input(".zs-dv-op", &v.operator);
        if v.type_ == "list" {
            set_input(".zs-dv-list", &v.value);
        } else if v.operator == "be" || v.operator == "nbe" {
            let parts: Vec<&str> = v.value.split(',').collect();
            if parts.len() == 2 {
                set_input(".zs-dv-val1", parts[0]);
                set_input(".zs-dv-val2", parts[1]);
            }
        } else {
            set_input(".zs-dv-val1", &v.value);
        }
        // Required checkbox
        if let Ok(Some(cb_el)) = modal.query_selector(".zs-dv-req") {
            if let Some(input) = cb_el.dyn_ref::<web_sys::HtmlInputElement>() {
                input.set_checked(v.required);
            }
        }
    } else {
        set_input(".zs-dv-type", "");
        set_input(".zs-dv-op", "be");
        set_input(".zs-dv-list", "");
        set_input(".zs-dv-val1", "");
        set_input(".zs-dv-val2", "");
        if let Ok(Some(cb_el)) = modal.query_selector(".zs-dv-req") {
            if let Some(input) = cb_el.dyn_ref::<web_sys::HtmlInputElement>() {
                input.set_checked(false);
            }
        }
    }
    // Update row visibility based on the chosen type.
    let type_val = modal
        .query_selector(".zs-dv-type")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into::<web_sys::HtmlInputElement>().ok())
        .map(|i| i.value())
        .unwrap_or_default();
    update_dv_rows(modal, &type_val);

    let _ = modal
        .unchecked_ref::<web_sys::HtmlElement>()
        .style()
        .set_property("display", "block");
    *visible.borrow_mut() = true;
}
