//! AutoFilter header dropdown (issue #10): per-column sort rows and value
//! checkboxes, opened by clicking the ▼ glyph on a filter-range header cell.
//! A single panel is reused across columns, mirroring the list-validity
//! popover (issue #9).

use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use crate::component::element::Element;
#[allow(unused_imports)]
use super::*;

/// Minimal HTML-attribute/text escaping for user cell values.
fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Populate and show the filter menu for column `ci` at canvas coords (x, y).
pub(crate) fn show_filter_menu(
    menu: Option<&web_sys::Element>,
    renderer: &SharedRenderer,
    ci: usize,
    x: f64,
    y: f64,
    visible_flag: &Rc<RefCell<bool>>,
) {
    let Some(m) = menu else { return };
    let items = renderer.borrow().filter_items(ci);
    let all_checked = items.iter().all(|(_, _, c)| *c);

    let mut html = String::from(
        "<div data-fsort=\"asc\" style=\"padding:5px 12px;cursor:pointer;\">Sort A &rarr; Z</div>\
         <div data-fsort=\"desc\" style=\"padding:5px 12px;cursor:pointer;\">Sort Z &rarr; A</div>\
         <div style=\"border-top:1px solid #e0e2e4;margin:4px 0;\"></div>",
    );
    html.push_str(&format!(
        "<label style=\"display:block;padding:3px 12px;cursor:pointer;\">\
           <input type=\"checkbox\" data-fall {}/> (Select all)\
         </label>\
         <div style=\"max-height:170px;overflow-y:auto;\">",
        if all_checked { "checked " } else { "" }
    ));
    for (v, n, checked) in &items {
        let escaped = esc(v);
        let label = if v.is_empty() { "(Blanks)".to_string() } else { escaped.clone() };
        html.push_str(&format!(
            "<label style=\"display:block;padding:3px 12px 3px 24px;cursor:pointer;white-space:nowrap;\">\
               <input type=\"checkbox\" data-fval=\"{escaped}\" {}/> {label} ({n})\
             </label>",
            if *checked { "checked " } else { "" }
        ));
    }
    html.push_str(
        "</div>\
         <div style=\"border-top:1px solid #e0e2e4;margin:4px 0;\"></div>\
         <div style=\"display:flex;gap:8px;justify-content:flex-end;padding:6px 12px;\">\
           <button data-fclear style=\"padding:3px 10px;cursor:pointer;\">Clear</button>\
           <button data-fapply style=\"padding:3px 10px;cursor:pointer;\">Apply</button>\
         </div>",
    );
    m.set_inner_html(&html);
    let _ = m.set_attribute("data-ci", &ci.to_string());

    // Position at the click; flip above when it would overflow the viewport
    // bottom (mirrors the list popover, issue #9).
    let vh = web_sys::window()
        .and_then(|w| w.inner_height().ok().and_then(|v| v.as_f64()))
        .unwrap_or(800.0);
    let top = if y + 280.0 > vh { (y - 280.0).max(0.0) } else { y };
    let style = m.unchecked_ref::<web_sys::HtmlElement>().style();
    let _ = style.set_property("left", &format!("{}px", x));
    let _ = style.set_property("top", &format!("{}px", top));
    let _ = style.set_property("display", "block");
    *visible_flag.borrow_mut() = true;
}

/// Wire the (single, reused) filter menu: the sort rows apply immediately,
/// Apply commits the checked values ("all" when everything is checked),
/// Clear resets the column, and the select-all checkbox drives the rest.
pub(crate) fn wire_filter_menu(
    menu: web_sys::Element,
    renderer: &SharedRenderer,
    sync: &SyncFn,
    visible_flag: &Rc<RefCell<bool>>,
) {
    let renderer = renderer.clone();
    let sync = sync.clone();
    let visible = visible_flag.clone();
    let menu_node = menu.clone();
    let mut el: Element = menu.into();
    el.add_event_listener("click", move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
        let ci: usize = menu_node
            .get_attribute("data-ci")
            .and_then(|v| v.parse().ok())
            .unwrap_or(0);
        let hide = |menu: &web_sys::Element, visible: &Rc<RefCell<bool>>| {
            let _ = menu
                .unchecked_ref::<web_sys::HtmlElement>()
                .style()
                .set_property("display", "none");
            *visible.borrow_mut() = false;
        };

        // Select-all drives every value checkbox; the menu stays open.
        if elx.has_attribute("data-fall") {
            let checked = elx.unchecked_ref::<HtmlInputElement>().checked();
            if let Ok(list) = menu_node.query_selector_all("input[data-fval]") {
                for i in 0..list.length() {
                    if let Some(input) =
                        list.item(i).and_then(|n| n.dyn_into::<HtmlInputElement>().ok())
                    {
                        input.set_checked(checked);
                    }
                }
            }
            return;
        }

        if let Some(sort_el) = elx.closest("[data-fsort]").ok().flatten() {
            let asc = sort_el.get_attribute("data-fsort").as_deref() == Some("asc");
            {
                let mut r = renderer.borrow_mut();
                r.sort_filter(ci, asc);
                r.render();
            }
            sync();
            hide(&menu_node, &visible);
            return;
        }

        if elx.closest("[data-fapply]").ok().flatten().is_some() {
            let mut values: Vec<String> = Vec::new();
            let mut total = 0usize;
            if let Ok(list) = menu_node.query_selector_all("input[data-fval]") {
                total = list.length() as usize;
                for i in 0..list.length() {
                    if let Some(input) =
                        list.item(i).and_then(|n| n.dyn_into::<HtmlInputElement>().ok())
                    {
                        if input.checked() {
                            values.push(input.get_attribute("data-fval").unwrap_or_default());
                        }
                    }
                }
            }
            {
                let mut r = renderer.borrow_mut();
                if values.len() == total {
                    r.set_column_filter(ci, "all", Vec::new());
                } else {
                    r.set_column_filter(ci, "in", values);
                }
                r.render();
            }
            sync();
            hide(&menu_node, &visible);
            return;
        }

        if elx.closest("[data-fclear]").ok().flatten().is_some() {
            {
                let mut r = renderer.borrow_mut();
                r.set_column_filter(ci, "all", Vec::new());
                r.render();
            }
            sync();
            hide(&menu_node, &visible);
        }
        // Plain label/checkbox clicks fall through: native toggle, menu stays.
    });
}
