use super::autocomplete;
#[allow(unused_imports)]
use super::*;
use crate::component::element::Element;
use crate::config::CSS_PREFIX;
use gloo::utils::window;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlInputElement, HtmlTextAreaElement, KeyboardEvent};

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
    let fns = [
        "SUM", "AVERAGE", "MAX", "MIN", "COUNT", "PRODUCT", "ABS", "ROUND", "IF",
        // Dynamic-array functions (issue #33).
        "FILTER", "SORT", "UNIQUE", "SEQUENCE",
    ];
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

// --- Formula autocomplete (issue #26) -------------------------------------

/// Current caret byte offset in `input`.
fn caret_of(input: &HtmlInputElement) -> usize {
    let len = input.value().len() as u32;
    input.selection_start().ok().flatten().unwrap_or(len) as usize
}

/// Position `popover` directly under the formula input and show it.
fn ac_position(input: &HtmlInputElement, popover: &HtmlElement) {
    if let Ok(el) = input.clone().dyn_into::<web_sys::Element>() {
        let r = el.get_bounding_client_rect();
        let st = popover.style();
        let _ = st.set_property("display", "block");
        let _ = st.set_property("left", &format!("{}px", r.left()));
        let _ = st.set_property("top", &format!("{}px", r.bottom()));
    }
}

/// Refresh the popover for the input's current text/caret. Shows matching
/// function names (with `index` highlighted), or an argument-signature hint
/// when inside a call, or hides it. Returns the number of name matches shown
/// (0 when showing a signature or hidden) — handlers use this to know whether
/// keyboard navigation applies.
fn ac_refresh(input: &HtmlInputElement, popover: &HtmlElement, index: usize) -> usize {
    let value = input.value();
    let caret = caret_of(input);
    if let Some((_, prefix)) = autocomplete::prefix_at(&value, caret) {
        let m = autocomplete::matches(&prefix);
        if !m.is_empty() {
            let n = m.len();
            let sel = index.min(n - 1);
            let mut html = String::new();
            for (i, (name, sig)) in m.iter().enumerate() {
                let bg = if i == sel { "background:#e8f0fe;" } else { "" };
                html.push_str(&format!(
                    "<div class=\"{p}-ac-item\" data-ac-name=\"{name}\" \
                       style=\"padding:3px 10px;cursor:pointer;white-space:nowrap;{bg}\">\
                       <b>{name}</b> <span style=\"color:#999;font-size:11px;\">{sig}</span></div>",
                    p = CSS_PREFIX,
                ));
            }
            popover.set_inner_html(&html);
            ac_position(input, popover);
            return n;
        }
    }
    if let Some(sig) = autocomplete::active_signature(&value, caret) {
        popover.set_inner_html(&format!(
            "<div style=\"padding:3px 10px;color:#333;white-space:nowrap;\">{sig}</div>"
        ));
        ac_position(input, popover);
        return 0;
    }
    let _ = popover.style().set_property("display", "none");
    0
}

/// Replace the function-name prefix at the caret with `name(`, caret inside.
fn ac_accept(input: &HtmlInputElement, name: &str) {
    let value = input.value();
    let caret = caret_of(input);
    if let Some((start, _)) = autocomplete::prefix_at(&value, caret) {
        let new = format!("{}{}({}", &value[..start], name, &value[caret..]);
        input.set_value(&new);
        let pos = (start + name.len() + 1) as u32;
        let _ = input.set_selection_range(pos, pos);
    }
}

/// True if the popover is currently shown.
fn ac_visible(popover: &HtmlElement) -> bool {
    popover
        .style()
        .get_property_value("display")
        .map(|d| d != "none")
        .unwrap_or(false)
}

/// Wire autocomplete onto a formula-entry input: a suggestion popover while
/// typing `=name`, keyboard navigation, and an argument-signature hint inside
/// a call (issue #26). The keydown listener is registered BEFORE the input's
/// Enter-commit handler so accepting a suggestion with Enter/Tab doesn't also
/// commit the cell.
fn wire_autocomplete(input: &HtmlInputElement) {
    let doc = gloo::utils::document();
    let Ok(popover) = doc.create_element("div") else {
        return;
    };
    let _ = popover.set_attribute(
        "style",
        "display:none;position:fixed;z-index:1200;background:#fff;\
         border:1px solid #ccc;box-shadow:1px 2px 6px rgba(0,0,0,0.15);\
         max-height:240px;overflow-y:auto;font-size:13px;min-width:160px;",
    );
    let Ok(pop): Result<HtmlElement, _> = popover.clone().dyn_into() else {
        return;
    };
    if let Some(body) = doc.body() {
        let _ = body.append_child(&popover);
    }
    let index = Rc::new(RefCell::new(0usize));

    // Live updates as the user types.
    {
        let inp = input.clone();
        let pop = pop.clone();
        let index = index.clone();
        let mut el: Element = input.clone().dyn_into::<web_sys::Element>().unwrap().into();
        el.add_event_listener("input", move |_e: web_sys::Event| {
            *index.borrow_mut() = 0;
            ac_refresh(&inp, &pop, 0);
        });
    }

    // Keyboard: navigate / accept / dismiss while the popover is open.
    {
        let inp = input.clone();
        let pop = pop.clone();
        let index = index.clone();
        let mut el: Element = input.clone().dyn_into::<web_sys::Element>().unwrap().into();
        el.add_event_listener("keydown", move |event: web_sys::Event| {
            if !ac_visible(&pop) {
                return;
            }
            let ke: KeyboardEvent = event.clone().dyn_into().unwrap();
            let value = inp.value();
            let caret = caret_of(&inp);
            let m = autocomplete::prefix_at(&value, caret)
                .map(|(_, p)| autocomplete::matches(&p))
                .unwrap_or_default();
            let n = m.len();
            match ke.key().as_str() {
                "ArrowDown" if n > 0 => {
                    let next = {
                        let mut i = index.borrow_mut();
                        *i = (*i + 1) % n;
                        *i
                    };
                    ke.prevent_default();
                    ac_refresh(&inp, &pop, next);
                }
                "ArrowUp" if n > 0 => {
                    let next = {
                        let mut i = index.borrow_mut();
                        *i = (*i + n - 1) % n;
                        *i
                    };
                    ke.prevent_default();
                    ac_refresh(&inp, &pop, next);
                }
                "Enter" | "Tab" if n > 0 => {
                    let sel = (*index.borrow()).min(n - 1);
                    ac_accept(&inp, m[sel].0);
                    ke.prevent_default();
                    event.stop_immediate_propagation(); // don't also commit the cell
                    *index.borrow_mut() = 0;
                    ac_refresh(&inp, &pop, 0); // surface the signature hint
                }
                "Escape" => {
                    let _ = pop.style().set_property("display", "none");
                    ke.prevent_default();
                    event.stop_immediate_propagation(); // don't revert the input
                }
                _ => {}
            }
        });
    }

    // Click (mousedown so the input keeps focus) a suggestion to accept it.
    {
        let inp = input.clone();
        let pop = pop.clone();
        let index = index.clone();
        let mut el: Element = popover.clone().into();
        el.add_event_listener("mousedown", move |event: web_sys::Event| {
            event.prevent_default();
            let Some(target) = event.target() else { return };
            let Ok(elx) = target.dyn_into::<web_sys::Element>() else {
                return;
            };
            if let Ok(Some(item)) = elx.closest("[data-ac-name]") {
                if let Some(name) = item.get_attribute("data-ac-name") {
                    ac_accept(&inp, &name);
                    *index.borrow_mut() = 0;
                    ac_refresh(&inp, &pop, 0);
                }
            }
        });
    }

    // Hide when the input loses focus (a suggestion mousedown preventDefault
    // keeps focus, so clicking the popover doesn't trigger this).
    {
        let pop = pop.clone();
        let mut el: Element = input.clone().dyn_into::<web_sys::Element>().unwrap().into();
        el.add_event_listener("blur", move |_e: web_sys::Event| {
            let _ = pop.style().set_property("display", "none");
        });
    }
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
    for input in [name_box.clone(), formula_input.clone()]
        .into_iter()
        .flatten()
    {
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
    if let (Some(fx_span), Some(menu), Some(fi)) = (fx_span, fx_menu.clone(), formula_input.clone())
    {
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
                let Ok(elx) = target.dyn_into::<web_sys::Element>() else {
                    return;
                };
                let Some(name) = elx.get_attribute("data-fxfn") else {
                    return;
                };
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

    // Autocomplete (issue #26). Wired BEFORE the Enter-commit handler below so
    // its keydown listener fires first and can swallow Enter/Tab when accepting
    // a suggestion (stop_immediate_propagation) instead of committing the cell.
    if let Some(fi) = &formula_input {
        wire_autocomplete(fi);
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
    let (rect, text, zoom) = {
        let mut r = renderer.borrow_mut();
        r.select_cell(ri, ci);
        r.render();
        // Size the editor to the whole merged region, not just the anchor cell.
        (
            r.merged_screen_rect(ri, ci),
            r.cell_text_at(ri, ci),
            r.zoom(),
        )
    };
    let style = textarea.style();
    let _ = style.set_property("left", &format!("{}px", rect.x));
    let _ = style.set_property("top", &format!("{}px", rect.y));
    let _ = style.set_property("width", &format!("{}px", rect.width));
    let _ = style.set_property("height", &format!("{}px", rect.height));
    // The cell rect is zoomed (issue #32); scale the editor font to match the
    // rendered text (base 13px set in init_editor_style).
    let _ = style.set_property("font-size", &format!("{}px", 13.0 * zoom));
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
pub(crate) fn cancel_edit(
    textarea: &HtmlTextAreaElement,
    editor_error: Option<&HtmlElement>,
    editing: &EditingCell,
) {
    *editing.borrow_mut() = None;
    let _ = textarea.style().set_property("display", "none");
    let _ = textarea.style().set_property("border", "");
    if let Some(e) = editor_error {
        let _ = e.style().set_property("display", "none");
    }
}
