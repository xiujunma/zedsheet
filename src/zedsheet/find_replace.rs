#[allow(unused_imports)]
use super::*;
use crate::component::element::Element;
use gloo::utils::window;
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, KeyboardEvent};

pub(crate) fn find_panel_html() -> String {
    "<div style=\"display:flex;align-items:center;gap:4px;margin-bottom:6px;\">\
       <input class=\"zs-find-input\" placeholder=\"Find\" style=\"flex:1;padding:3px 6px;border:1px solid #ccc;outline:none;\" />\
       <span class=\"zs-find-count\" style=\"min-width:46px;text-align:right;color:#888;font-size:12px;\">0/0</span>\
       <span class=\"zs-find-close\" style=\"cursor:pointer;padding:0 6px;color:#888;\">✕</span>\
     </div>\
     <input class=\"zs-replace-input\" placeholder=\"Replace with\" style=\"width:100%;box-sizing:border-box;padding:3px 6px;border:1px solid #ccc;outline:none;\" />\
     <div style=\"display:flex;gap:6px;margin-top:8px;justify-content:flex-end;\">\
       <button class=\"zs-find-next\">Find next</button>\
       <button class=\"zs-replace-one\">Replace</button>\
       <button class=\"zs-replace-all\">Replace all</button>\
     </div>"
        .to_string()
}

/// Wire the find & replace panel: search, navigate matches, replace.
pub(crate) fn wire_find(panel: web_sys::Element, renderer: &SharedRenderer, sync: &SyncFn) {
    let qsel = |sel: &str| panel.query_selector(sel).ok().flatten();
    let find_input: HtmlInputElement = qsel(".zs-find-input").unwrap().dyn_into().unwrap();
    let replace_input: HtmlInputElement = qsel(".zs-replace-input").unwrap().dyn_into().unwrap();
    let count_el = qsel(".zs-find-count").unwrap();

    // Shared match state.
    let matches: Rc<RefCell<Vec<(usize, usize)>>> = Rc::new(RefCell::new(Vec::new()));
    let idx: Rc<RefCell<usize>> = Rc::new(RefCell::new(0));

    // Update the "i/n" counter.
    let update_count = {
        let count_el = count_el.clone();
        let matches = matches.clone();
        let idx = idx.clone();
        Rc::new(move || {
            let n = matches.borrow().len();
            let i = if n == 0 { 0 } else { *idx.borrow() + 1 };
            count_el.set_text_content(Some(&format!("{}/{}", i, n)));
        }) as Rc<dyn Fn()>
    };

    // Recompute matches for the current query and reveal the first one.
    let refresh = {
        let renderer = renderer.clone();
        let sync = sync.clone();
        let find_input = find_input.clone();
        let matches = matches.clone();
        let idx = idx.clone();
        let update_count = update_count.clone();
        Rc::new(move || {
            let q = find_input.value();
            let found = renderer.borrow().find_matches(&q);
            *matches.borrow_mut() = found;
            *idx.borrow_mut() = 0;
            if let Some(&(ri, ci)) = matches.borrow().first() {
                let mut r = renderer.borrow_mut();
                r.select_and_reveal(ri, ci);
                r.render();
                drop(r);
                sync();
            }
            update_count();
        }) as Rc<dyn Fn()>
    };

    // Reveal the match at the current index.
    let reveal_current = {
        let renderer = renderer.clone();
        let sync = sync.clone();
        let matches = matches.clone();
        let idx = idx.clone();
        let update_count = update_count.clone();
        Rc::new(move || {
            let m = matches.borrow();
            if m.is_empty() {
                update_count();
                return;
            }
            let i = *idx.borrow();
            let (ri, ci) = m[i];
            {
                let mut r = renderer.borrow_mut();
                r.select_and_reveal(ri, ci);
                r.render();
            }
            sync();
            update_count();
        }) as Rc<dyn Fn()>
    };

    // Find input: recompute on each keystroke; Enter advances.
    {
        let refresh = refresh.clone();
        let mut el: Element = find_input
            .clone()
            .dyn_into::<web_sys::Element>()
            .unwrap()
            .into();
        el.add_event_listener("input", move |_e: web_sys::Event| refresh());
    }
    {
        let idx = idx.clone();
        let matches = matches.clone();
        let reveal_current = reveal_current.clone();
        let mut el: Element = find_input
            .clone()
            .dyn_into::<web_sys::Element>()
            .unwrap()
            .into();
        el.add_event_listener("keydown", move |e: web_sys::Event| {
            let ke: KeyboardEvent = e.dyn_into().unwrap();
            if ke.key() == "Enter" {
                let n = matches.borrow().len();
                if n > 0 {
                    let next = (*idx.borrow() + 1) % n;
                    *idx.borrow_mut() = next;
                    reveal_current();
                }
            }
        });
    }

    // Find next button.
    {
        let idx = idx.clone();
        let matches = matches.clone();
        let reveal_current = reveal_current.clone();
        if let Some(btn) = qsel(".zs-find-next") {
            let mut el: Element = btn.into();
            el.add_event_listener("click", move |_e: web_sys::Event| {
                let n = matches.borrow().len();
                if n > 0 {
                    let next = (*idx.borrow() + 1) % n;
                    *idx.borrow_mut() = next;
                    reveal_current();
                }
            });
        }
    }

    // Replace current match, then move to the next.
    {
        let renderer = renderer.clone();
        let sync = sync.clone();
        let find_input = find_input.clone();
        let replace_input = replace_input.clone();
        let matches = matches.clone();
        let idx = idx.clone();
        let refresh = refresh.clone();
        if let Some(btn) = qsel(".zs-replace-one") {
            let mut el: Element = btn.into();
            el.add_event_listener("click", move |_e: web_sys::Event| {
                let cur = {
                    let m = matches.borrow();
                    if m.is_empty() {
                        None
                    } else {
                        Some(m[*idx.borrow()])
                    }
                };
                if let Some((ri, ci)) = cur {
                    {
                        let mut r = renderer.borrow_mut();
                        r.replace_in_cell(ri, ci, &find_input.value(), &replace_input.value());
                        r.render();
                    }
                    sync();
                    refresh(); // recompute (the cell may no longer match)
                }
            });
        }
    }

    // Replace all in one undo step.
    {
        let renderer = renderer.clone();
        let sync = sync.clone();
        let find_input = find_input.clone();
        let replace_input = replace_input.clone();
        let count_el = count_el.clone();
        let matches = matches.clone();
        if let Some(btn) = qsel(".zs-replace-all") {
            let mut el: Element = btn.into();
            el.add_event_listener("click", move |_e: web_sys::Event| {
                let n = {
                    let mut r = renderer.borrow_mut();
                    let n = r.replace_all(&find_input.value(), &replace_input.value());
                    r.render();
                    n
                };
                sync();
                matches.borrow_mut().clear();
                count_el.set_text_content(Some(&format!("replaced {}", n)));
            });
        }
    }

    // Close button + Escape.
    {
        let panel_for_close = panel.clone();
        if let Some(btn) = qsel(".zs-find-close") {
            let mut el: Element = btn.into();
            el.add_event_listener("click", move |_e: web_sys::Event| {
                let _ = panel_for_close
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
            });
        }
    }
    {
        let panel_for_esc = panel.clone();
        let mut el: Element = panel.clone().into();
        el.add_event_listener("keydown", move |e: web_sys::Event| {
            let ke: KeyboardEvent = e.dyn_into().unwrap();
            if ke.key() == "Escape" {
                let _ = panel_for_esc
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "none");
            }
        });
    }

    // Ctrl/Cmd+F opens the panel and focuses the find input.
    {
        let panel = panel.clone();
        let find_input = find_input.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let ke: KeyboardEvent = event.dyn_into().unwrap();
            if (ke.ctrl_key() || ke.meta_key()) && ke.key().to_lowercase() == "f" {
                ke.prevent_default();
                let _ = panel
                    .unchecked_ref::<web_sys::HtmlElement>()
                    .style()
                    .set_property("display", "block");
                let _ = find_input.focus();
                find_input.select();
            }
        });
        window()
            .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }
}
