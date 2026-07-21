#[allow(unused_imports)]
use super::*;
use crate::component::element::Element;
use crate::core::data_proxy::DataProxy;
use gloo::utils::window;
use wasm_bindgen::JsCast;
use web_sys::{HtmlElement, HtmlTextAreaElement};

pub(crate) fn render_tabs(menu_el: &web_sys::Element, names: &[String], active: usize) {
    let mut html = String::new();
    for (i, name) in names.iter().enumerate() {
        let cls = if i == active { "active" } else { "" };
        html.push_str(&format!(
            "<li data-index=\"{}\" class=\"{}\">{}</li>",
            i,
            cls,
            escape_html(name)
        ));
    }
    menu_el.set_inner_html(&html);
}

/// Persist the active sheet back into `sheets`, then load `new_idx` into the
/// renderer and refresh the tab strip.
pub(crate) fn switch_sheet(
    renderer: &SharedRenderer,
    sheets: &Sheets,
    active: &ActiveSheet,
    menu_el: &web_sys::Element,
    new_idx: usize,
) {
    let cur = *active.borrow();
    if new_idx == cur || new_idx >= sheets.borrow().len() {
        return;
    }
    let current_data = renderer.borrow().data_clone();
    {
        let mut s = sheets.borrow_mut();
        s[cur] = current_data;
    }
    *active.borrow_mut() = new_idx;
    let new_data = sheets.borrow()[new_idx].clone();
    {
        let mut r = renderer.borrow_mut();
        r.set_data(new_data);
        r.render();
    }
    let names: Vec<String> = sheets.borrow().iter().map(|d| d.name.clone()).collect();
    render_tabs(menu_el, &names, new_idx);
}

/// Wire the bottom bar: tab clicks switch sheets, double-click renames,
/// right-click deletes, and the add button appends a sheet.
///
/// Takes the in-cell editor handles because every sheet swap must commit a
/// pending edit FIRST: `commit_edit` writes through the renderer's current
/// data, so an unreconciled commit after the swap would land the value on
/// the wrong sheet.
///
/// `mode` gates the structural actions in view-only: Add / Rename / Delete
/// are inert when the workbook is mounted in `Mode::ViewOnly`. Without the
/// gate, a view-only user could "upgrade" the workbook to editable by
/// adding a fresh (non-read-only) sheet (Phase 7).
#[allow(clippy::too_many_arguments)]
pub(crate) fn wire_bottombar(
    menu_el: Element,
    mut add_el: Element,
    mode: crate::component::options::Mode,
    renderer: &SharedRenderer,
    sheets: &Sheets,
    active: &ActiveSheet,
    textarea: &HtmlTextAreaElement,
    editor_error: Option<HtmlElement>,
    editing: &EditingCell,
    sync: &SyncFn,
) {
    let view_only = mode == crate::component::options::Mode::ViewOnly;
    let menu_node = menu_el.el.clone().unwrap();

    // Initial render of the tab strip.
    {
        let names: Vec<String> = sheets.borrow().iter().map(|d| d.name.clone()).collect();
        render_tabs(&menu_node, &names, *active.borrow());
    }

    // Tab click (delegated): switch to the clicked sheet.
    {
        let renderer = renderer.clone();
        let sheets = sheets.clone();
        let active = active.clone();
        let menu_for_handler = menu_node.clone();
        let sync = sync.clone();
        let textarea = textarea.clone();
        let editor_error = editor_error.clone();
        let editing = editing.clone();
        let mut menu_el_mut = menu_el;
        menu_el_mut.add_event_listener("click", move |event: web_sys::Event| {
            let Some(target) = event.target() else { return };
            let Ok(el) = target.dyn_into::<web_sys::Element>() else {
                return;
            };
            let li = el
                .get_attribute("data-index")
                .map(|_| el.clone())
                .or_else(|| el.closest("[data-index]").ok().flatten());
            let Some(li) = li else { return };
            if let Some(idx) = li
                .get_attribute("data-index")
                .and_then(|s| s.parse::<usize>().ok())
            {
                // Commit a pending edit to THIS sheet before swapping data.
                if !reconcile_editor(&renderer, &textarea, editor_error.as_ref(), &editing) {
                    return;
                }
                switch_sheet(&renderer, &sheets, &active, &menu_for_handler, idx);
                sync();
            }
        });
    }

    // Double-click a tab: rename via prompt. View-only: no-op so a
    // user can't rename sheets to disguise the workbook as their own.
    {
        let renderer = renderer.clone();
        let sheets = sheets.clone();
        let active = active.clone();
        let menu_for = menu_node.clone();
        let mut menu_dbl: Element = menu_node.clone().into();
        menu_dbl.add_event_listener("dblclick", move |event: web_sys::Event| {
            if view_only {
                return;
            }
            let Some(idx) = tab_index_from_event(&event) else {
                return;
            };
            let cur_name = sheets
                .borrow()
                .get(idx)
                .map(|d| d.name.clone())
                .unwrap_or_default();
            if let Ok(Some(name)) =
                window().prompt_with_message_and_default("Sheet name:", &cur_name)
            {
                let name = name.trim().to_string();
                if name.is_empty() {
                    return;
                }
                if let Some(s) = sheets.borrow_mut().get_mut(idx) {
                    s.name = name.clone();
                }
                if idx == *active.borrow() {
                    renderer.borrow_mut().data.name = name;
                }
                let names: Vec<String> = sheets.borrow().iter().map(|d| d.name.clone()).collect();
                render_tabs(&menu_for, &names, *active.borrow());
            }
        });
    }

    // Right-click a tab: delete (when more than one sheet remains).
    // View-only: no-op — keep the structural chrome read-only too.
    {
        let renderer = renderer.clone();
        let sheets = sheets.clone();
        let active = active.clone();
        let menu_for = menu_node.clone();
        let sync = sync.clone();
        let textarea = textarea.clone();
        let editor_error = editor_error.clone();
        let editing = editing.clone();
        let mut menu_ctx: Element = menu_node.clone().into();
        menu_ctx.add_event_listener("contextmenu", move |event: web_sys::Event| {
            event.prevent_default();
            if view_only {
                return;
            }
            let Some(idx) = tab_index_from_event(&event) else {
                return;
            };
            if sheets.borrow().len() <= 1 {
                return;
            }
            // Settle a pending edit before any data swap (see wire_bottombar).
            if !reconcile_editor(&renderer, &textarea, editor_error.as_ref(), &editing) {
                return;
            }
            let nm = sheets.borrow()[idx].name.clone();
            if !matches!(
                window().confirm_with_message(&format!("Delete sheet \"{}\"?", nm)),
                Ok(true)
            ) {
                return;
            }
            let len_after = {
                let mut s = sheets.borrow_mut();
                s.remove(idx);
                s.len()
            };
            let cur = *active.borrow();
            let new_active = if cur > idx {
                cur - 1
            } else if cur == idx {
                idx.min(len_after - 1)
            } else {
                cur
            };
            *active.borrow_mut() = new_active;
            let new_data = sheets.borrow()[new_active].clone();
            {
                let mut r = renderer.borrow_mut();
                r.set_data(new_data);
                r.render();
            }
            let names: Vec<String> = sheets.borrow().iter().map(|d| d.name.clone()).collect();
            render_tabs(&menu_for, &names, new_active);
            sync();
        });
    }

    // Add button: append a new sheet and switch to it. View-only:
    // no-op at the action level — adding a fresh (default-editable)
    // sheet in a view-only workbook would let the user effectively
    // escape the mode. (Even though `data.is_read_only()` would block
    // edits, the structural action itself shouldn't be available.)
    {
        let renderer = renderer.clone();
        let sheets = sheets.clone();
        let active = active.clone();
        let menu_for_add = menu_node.clone();
        let sync = sync.clone();
        let textarea = textarea.clone();
        let editor_error = editor_error.clone();
        let editing = editing.clone();
        add_el.add_event_listener("click", move |_event: web_sys::Event| {
            if view_only {
                return;
            }
            // Settle a pending edit before any data swap (see wire_bottombar).
            if !reconcile_editor(&renderer, &textarea, editor_error.as_ref(), &editing) {
                return;
            }
            let new_idx = {
                let mut s = sheets.borrow_mut();
                let n = s.len() + 1;
                let mut new_sheet = DataProxy::new(&format!("sheet{}", n));
                // Wire the registry on the freshly added sheet so its
                // formulas can resolve cross-sheet refs (issue #4).
                new_sheet.set_sheets(&sheets);
                s.push(new_sheet);
                s.len() - 1
            };
            // Defense-in-depth: if the gate above is ever relaxed
            // (e.g. to allow structural adds but not data edits),
            // the freshly created sheet must still inherit
            // read-only from the workbook's view-only mode so
            // switching to it doesn't reveal an editable tab
            // (Phase 7 follow-on).
            if view_only {
                sheets.borrow_mut()[new_idx].set_read_only(true);
            }
            // Persist current sheet, then load the freshly added (empty) one.
            let current_data = renderer.borrow().data_clone();
            {
                let cur = *active.borrow();
                sheets.borrow_mut()[cur] = current_data;
            }
            *active.borrow_mut() = new_idx;
            let new_data = sheets.borrow()[new_idx].clone();
            {
                let mut r = renderer.borrow_mut();
                r.set_data(new_data);
                r.render();
            }
            let names: Vec<String> = sheets.borrow().iter().map(|d| d.name.clone()).collect();
            render_tabs(&menu_for_add, &names, new_idx);
            sync();
        });
    }
}

/// Markup for the find & replace panel.
pub(crate) fn tab_index_from_event(event: &web_sys::Event) -> Option<usize> {
    let target = event.target()?;
    let el = target.dyn_into::<web_sys::Element>().ok()?;
    let li = el
        .get_attribute("data-index")
        .map(|_| el.clone())
        .or_else(|| el.closest("[data-index]").ok().flatten())?;
    li.get_attribute("data-index")
        .and_then(|s| s.parse::<usize>().ok())
}
