use std::rc::Rc;
use gloo::utils::window;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlTextAreaElement, KeyboardEvent};
use crate::component::element::Element;
use crate::config::CSS_PREFIX;
#[allow(unused_imports)]
use super::*;

pub(crate) fn formula_bar_html() -> String {
    "<input class=\"zs-name-box\" style=\"width:80px;height:100%;box-sizing:border-box;border:none;border-right:1px solid #e0e2e4;padding:0 6px;outline:none;font-size:13px;\" />\
     <span class=\"zs-fb-cancel\" style=\"width:24px;text-align:center;color:#999;cursor:pointer;\">✕</span>\
     <span class=\"zs-fb-confirm\" style=\"width:24px;text-align:center;color:#999;cursor:pointer;\">✓</span>\
     <span class=\"zs-fx\" style=\"width:34px;text-align:center;color:#999;font-style:italic;border-right:1px solid #e0e2e4;cursor:pointer;\">fx</span>\
     <input class=\"zs-formula-input\" style=\"flex:1;height:100%;box-sizing:border-box;border:none;padding:0 8px;outline:none;font-size:13px;\" />"
        .to_string()
}

/// Functions offered by the formula-bar fx picker.
pub(crate) fn fx_menu_html() -> String {
    let fns = ["SUM", "AVERAGE", "MAX", "MIN", "COUNT", "PRODUCT", "ABS", "ROUND", "IF"];
    let mut s = String::new();
    for f in fns {
        s.push_str(&format!(
            "<div class=\"{p}-item\" data-fxfn=\"{f}\" style=\"cursor:pointer;\">{f}</div>",
            p = CSS_PREFIX,
            f = f
        ));
    }
    s
}

/// Wire the formula bar: name box navigates, the input edits the active cell,
/// and the fx picker inserts a function template.
pub(crate) fn wire_formula_bar(
    fbar: web_sys::Element,
    renderer: &SharedRenderer,
    textarea: &HtmlTextAreaElement,
    editor_error: Option<HtmlElement>,
    editing: &EditingCell,
    sync: &SyncFn,
    fx_menu: Option<web_sys::Element>,
    toast_node: Option<HtmlElement>,
) {
    let name_box: Option<HtmlInputElement> = fbar
        .query_selector(".zs-name-box")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into().ok());
    let formula_input: Option<HtmlInputElement> = fbar
        .query_selector(".zs-formula-input")
        .ok()
        .flatten()
        .and_then(|e| e.dyn_into().ok());
    let cancel = fbar.query_selector(".zs-fb-cancel").ok().flatten();
    let confirm = fbar.query_selector(".zs-fb-confirm").ok().flatten();
    let fx_span = fbar.query_selector(".zs-fx").ok().flatten();

    // Focusing the formula bar while the in-cell editor is open commits the
    // editor first: both edit the same cell, and the bar's commit path
    // (Enter / ✓: write the active cell, move the selection) would otherwise
    // run under a still-open editor — leaving it floating over a moved
    // selection with two competing values. On validation failure, focus is
    // sent straight back to the invalid editor (issue #9 keeps it open).
    for input in [name_box.clone(), formula_input.clone()].into_iter().flatten() {
        let renderer = renderer.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error.clone();
        let sync = sync.clone();
        let mut el: Element = input.dyn_into::<web_sys::Element>().unwrap().into();
        el.add_event_listener("focus", move |_e: web_sys::Event| {
            if editing.borrow().is_none() {
                return;
            }
            if reconcile_editor(&renderer, &textarea, editor_error.as_ref(), &editing) {
                sync(); // refresh the bar with the just-committed value
            } else {
                let _ = textarea.focus();
            }
        });
    }

    // fx picker: click fx to open the menu, click a function to insert it.
    if let (Some(fx_span), Some(menu), Some(fi)) = (fx_span, fx_menu.clone(), formula_input.clone()) {
        // Open under the fx label.
        {
            let menu = menu.clone();
            let mut el: Element = fx_span.clone().into();
            el.add_event_listener("click", move |_e: web_sys::Event| {
                show_palette_under(&menu, &fx_span);
            });
        }
        // Insert the chosen function as `=FN()` with the caret inside the parens.
        {
            let menu_for_hide = menu.clone();
            let mut el: Element = menu.clone().into();
            el.add_event_listener("click", move |event: web_sys::Event| {
                let Some(target) = event.target() else { return };
                let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
                let Some(name) = elx.get_attribute("data-fxfn") else { return };
                let value = format!("={}()", name);
                let caret = value.len().saturating_sub(1) as u32;
                // Focus FIRST: the focus guard above may commit an open cell
                // editor and sync() the input — doing it after set_value
                // would clobber the inserted template.
                let _ = fi.focus();
                fi.set_value(&value);
                let _ = fi.set_selection_range(caret, caret);
                hide_palette(&menu_for_hide);
            });
        }
        // Close the fx menu on outside click.
        {
            let menu = menu.clone();
            let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                if let Some(target) = event.target() {
                    if let Ok(node) = target.clone().dyn_into::<web_sys::Node>() {
                        if menu.contains(Some(&node)) {
                            return;
                        }
                    }
                    if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                        if el.closest(".zs-fx").ok().flatten().is_some() {
                            return;
                        }
                    }
                }
                hide_palette(&menu);
            });
            window()
                .add_event_listener_with_callback("mousedown", cb.as_ref().unchecked_ref())
                .unwrap();
            cb.forget();
        }
    }

    // Commit the formula input to the active cell.
    let commit = {
        let renderer = renderer.clone();
        let formula_input = formula_input.clone();
        let sync = sync.clone();
        let toast = toast_node.clone();
        Rc::new(move || {
            if let Some(fi) = &formula_input {
                let v = fi.value();
                let (ri, ci) = {
                    let r = renderer.borrow();
                    let s = r.get_selector();
                    (s.ri, s.ci)
                };
                {
                    let mut r = renderer.borrow_mut();
                    if let Err(msg) = r.set_cell_text_at(ri, ci, &v) {
                        // Validation failed (issue #9): revert the input to the
                        // previous value and surface a brief toast. The cell
                        // value is unchanged.
                        let previous = r.data.get_cell_text(ri, ci);
                        if let Some(fi) = &formula_input {
                            fi.set_value(&previous);
                        }
                        show_toast(toast.as_ref(), &msg);
                    }
                    r.render();
                }
                sync();
            }
        }) as Rc<dyn Fn()>
    };

    // Name box: Enter navigates to the typed cell reference or range.
    if let Some(nb) = &name_box {
        let renderer = renderer.clone();
        let sync = sync.clone();
        let nb_inner = nb.clone();
        let mut el: Element = nb.clone().dyn_into::<web_sys::Element>().unwrap().into();
        el.add_event_listener("keydown", move |event: web_sys::Event| {
            let ke: KeyboardEvent = event.dyn_into().unwrap();
            if ke.key() != "Enter" {
                return;
            }
            let val = nb_inner.value().trim().to_uppercase();
            let moved = if let Some((a, b)) = val.split_once(':') {
                // Range like A1:B3.
                match (parse_ref(a), parse_ref(b)) {
                    (Some((c0, r0)), Some((c1, r1))) => {
                        let mut r = renderer.borrow_mut();
                        r.select_cell(r0, c0);
                        r.select_to(r1, c1);
                        r.render();
                        true
                    }
                    _ => false,
                }
            } else if let Some((c, r0)) = parse_ref(&val) {
                let mut r = renderer.borrow_mut();
                r.select_cell(r0, c);
                r.render();
                true
            } else {
                // Not a cell ref/range: navigate to an existing named range, or
                // define a new name over the current selection.
                let mut r = renderer.borrow_mut();
                if r.select_named(&val) {
                    r.render();
                    true
                } else if is_valid_name(&val) {
                    r.define_selection_name(&val);
                    r.render();
                    true
                } else {
                    false
                }
            };
            if moved {
                sync();
            }
        });
    }

    // Formula input: Enter commits (and moves down), Escape reverts.
    if let Some(fi) = &formula_input {
        let renderer = renderer.clone();
        let sync = sync.clone();
        let commit = commit.clone();
        let mut el: Element = fi.clone().dyn_into::<web_sys::Element>().unwrap().into();
        el.add_event_listener("keydown", move |event: web_sys::Event| {
            let ke: KeyboardEvent = event.dyn_into().unwrap();
            match ke.key().as_str() {
                "Enter" => {
                    ke.prevent_default();
                    commit();
                    {
                        let mut r = renderer.borrow_mut();
                        r.move_selection(1, 0);
                        r.render();
                    }
                    sync();
                }
                "Escape" => {
                    ke.prevent_default();
                    sync(); // revert input to the cell's stored value
                }
                _ => {}
            }
        });
    }

    if let Some(c) = confirm {
        let commit = commit.clone();
        let renderer = renderer.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error.clone();
        let sync = sync.clone();
        let mut el: Element = c.into();
        el.add_event_listener("click", move |_e: web_sys::Event| {
            // With the cell editor open, IT is the live edit session: ✓
            // commits the editor's (fresher) text, not the bar's stale copy.
            if editing.borrow().is_some() {
                if reconcile_editor(&renderer, &textarea, editor_error.as_ref(), &editing) {
                    sync();
                } else {
                    let _ = textarea.focus();
                }
                return;
            }
            commit();
        });
    }
    if let Some(c) = cancel {
        let sync = sync.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error.clone();
        let mut el: Element = c.into();
        el.add_event_listener("click", move |_e: web_sys::Event| {
            // With the cell editor open, ✗ cancels the edit session itself.
            if editing.borrow().is_some() {
                cancel_edit(&textarea, editor_error.as_ref(), &editing);
            }
            sync();
        });
    }
}

pub(crate) fn init_editor_style(ta: &HtmlTextAreaElement) {
    let style = ta.style();
    let _ = style.set_property("position", "absolute");
    let _ = style.set_property("display", "none");
    let _ = style.set_property("box-sizing", "border-box");
    let _ = style.set_property("border", "2px solid #4b89ff");
    let _ = style.set_property("padding", "0 2px");
    let _ = style.set_property("margin", "0");
    let _ = style.set_property("outline", "none");
    let _ = style.set_property("resize", "none");
    let _ = style.set_property("overflow", "hidden");
    let _ = style.set_property("font", "13px Arial, sans-serif");
    // Opaque background + explicit text colour so the editor fully covers the
    // cell underneath. Required because some host resets (e.g. Tailwind's
    // Preflight) set `textarea { background-color: transparent; color: inherit }`,
    // which would otherwise let the rendered cell value bleed through the editor.
    let _ = style.set_property("background-color", "#ffffff");
    let _ = style.set_property("color", "#0a0a0a");
    let _ = style.set_property("z-index", "100");
}

/// Position the textarea over a cell, seed it with the cell's text, and focus.
pub(crate) fn start_edit(
    renderer: &SharedRenderer,
    textarea: &HtmlTextAreaElement,
    editor_error: Option<&HtmlElement>,
    editing: &EditingCell,
    ri: usize,
    ci: usize,
) {
    // Refuse to open the editor on a locked cell or a read-only sheet
    // (issue #24). Without this the user could still type into a
    // hidden/disabled textarea, and the commit would either silently
    // no-op or bypass the gate.
    {
        let r = renderer.borrow();
        if !r.data.is_cell_editable(ri, ci) {
            return;
        }
    }
    // Clear any prior validation error UI from the previous edit
    // (issue #9).
    let _ = textarea.style().set_property("border", "");
    if let Some(e) = editor_error {
        let _ = e.style().set_property("display", "none");
    }
    let (rect, text) = {
        let mut r = renderer.borrow_mut();
        r.select_cell(ri, ci);
        r.render();
        // Size the editor to the whole merged region, not just the anchor cell.
        (r.merged_screen_rect(ri, ci), r.cell_text_at(ri, ci))
    };
    let style = textarea.style();
    let _ = style.set_property("left", &format!("{}px", rect.x));
    let _ = style.set_property("top", &format!("{}px", rect.y));
    let _ = style.set_property("width", &format!("{}px", rect.width));
    let _ = style.set_property("height", &format!("{}px", rect.height));
    let _ = style.set_property("display", "block");
    textarea.set_value(&text);
    *editing.borrow_mut() = Some((ri, ci));
    let _ = textarea.focus();
    textarea.select();
}

/// Commit the editor's contents to the data model. Returns `Err(msg)` if
/// validation rejected the value (issue #9): in that case the editor
/// stays open with a red border and an error label below it, matching
/// Excel. Returns `Ok(())` on success (editor is hidden).
pub(crate) fn commit_edit(
    renderer: &SharedRenderer,
    textarea: &HtmlTextAreaElement,
    editor_error: Option<&HtmlElement>,
    editing: &EditingCell,
) -> Result<(), String> {
    let cell = editing.borrow_mut().take();
    let Some((ri, ci)) = cell else {
        let _ = textarea.style().set_property("display", "none");
        return Ok(());
    };
    let value = textarea.value();
    let result = {
        let mut r = renderer.borrow_mut();
        let res = r.set_cell_text_at(ri, ci, &value);
        r.render();
        res
    };
    if let Err(ref msg) = result {
        // Re-open the editor with the user's text preserved, red border,
        // and an error label below it. `editing` is restored so subsequent
        // keystrokes keep targeting the same cell.
        let style = textarea.style();
        let _ = style.set_property("display", "block");
        let _ = style.set_property("border", "2px solid #e53935");
        if let Some(e) = editor_error {
            e.set_text_content(Some(msg));
            let _ = e.style().set_property("display", "block");
        }
        *editing.borrow_mut() = Some((ri, ci));
    } else {
        let _ = textarea.style().set_property("display", "none");
        let _ = textarea.style().set_property("border", "");
        if let Some(e) = editor_error {
            let _ = e.style().set_property("display", "none");
        }
    }
    result
}

/// Commit the open in-cell editor before an interaction that would scroll
/// the viewport, resize geometry, or swap the data out from under it
/// (wheel, scrollbar/resize drags, sheet switches). The editor overlay is
/// positioned once at `start_edit`, so any of those would leave it floating
/// over the wrong cell. Returns `false` when a validation failure keeps the
/// editor open (issue #9) — the caller must swallow the interaction so
/// nothing moves under the invalid editor.
pub(crate) fn reconcile_editor(
    renderer: &SharedRenderer,
    textarea: &HtmlTextAreaElement,
    editor_error: Option<&HtmlElement>,
    editing: &EditingCell,
) -> bool {
    if editing.borrow().is_none() {
        return true;
    }
    commit_edit(renderer, textarea, editor_error, editing).is_ok()
}

/// Open the list-validity popover (issue #9) anchored at `(x, y)` showing
/// the allowed values for the cell. The popover element is mutated in
/// place; its `data-cell` attribute records the (ri, ci) for the click
/// handler.
pub(crate) fn cancel_edit(textarea: &HtmlTextAreaElement, editor_error: Option<&HtmlElement>, editing: &EditingCell) {
    *editing.borrow_mut() = None;
    let _ = textarea.style().set_property("display", "none");
    let _ = textarea.style().set_property("border", "");
    if let Some(e) = editor_error {
        let _ = e.style().set_property("display", "none");
    }
}
