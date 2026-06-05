use std::cell::RefCell;
use std::rc::Rc;
use gloo::utils::{document, window};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use crate::component::element::Element;
use crate::config::CSS_PREFIX;
use super::util::set_canvas_cursor;
#[allow(unused_imports)]
use super::*;

pub(crate) fn color_palette_html() -> String {
    let colors = [
        "#000000", "#434343", "#666666", "#999999", "#b7b7b7", "#cccccc", "#d9d9d9", "#ffffff",
        "#e53935", "#fb8c00", "#fdd835", "#43a047", "#1e88e5", "#3949ab", "#8e24aa", "#d81b60",
        "#ef9a9a", "#ffcc80", "#fff59d", "#a5d6a7", "#90caf9", "#9fa8da", "#ce93d8", "#f48fb1",
    ];
    let mut s = String::new();
    for c in colors {
        s.push_str(&format!(
            "<span data-color=\"{c}\" style=\"display:inline-block;width:16px;height:16px;margin:2px;border:1px solid #ddd;cursor:pointer;background:{c};\"></span>",
            c = c
        ));
    }
    format!("<div style=\"width:152px;\">{}</div>", s)
}

/// Show a styled tooltip below the hovered toolbar button (`data-tip`), and
/// hide it when the pointer leaves the toolbar.
pub(crate) fn wire_tooltip(toolbar: web_sys::Element, tooltip: web_sys::Element) {
    {
        let tooltip = tooltip.clone();
        let mut tb: Element = toolbar.clone().into();
        tb.add_event_listener("mouseover", move |event: web_sys::Event| {
            let Some(target) = event.target() else { return };
            let Ok(el) = target.dyn_into::<web_sys::Element>() else { return };
            let btn = el
                .get_attribute("data-tip")
                .map(|_| el.clone())
                .or_else(|| el.closest("[data-tip]").ok().flatten());
            match btn {
                Some(btn) => {
                    let tip = btn.get_attribute("data-tip").unwrap_or_default();
                    if tip.is_empty() {
                        hide_tooltip(&tooltip);
                        return;
                    }
                    tooltip.set_text_content(Some(&tip));
                    let rect = btn.get_bounding_client_rect();
                    let style = tooltip.unchecked_ref::<web_sys::HtmlElement>().style();
                    let _ = style.set_property("left", &format!("{}px", rect.left() + rect.width() / 2f64));
                    let _ = style.set_property("top", &format!("{}px", rect.bottom() + 8f64));
                    let _ = style.set_property("display", "block");
                }
                None => hide_tooltip(&tooltip),
            }
        });
    }
    {
        let tooltip = tooltip.clone();
        let mut tb: Element = toolbar.into();
        tb.add_event_listener("mouseout", move |_event: web_sys::Event| {
            hide_tooltip(&tooltip);
        });
    }
}

/// Delegated click handler on the toolbar: maps a button's `data-action` to a
/// renderer mutation and re-renders. Color buttons open the palette instead.
pub(crate) fn wire_toolbar(
    toolbar_el: &mut Element,
    renderer: &SharedRenderer,
    palette: Option<web_sys::Element>,
    palette_mode: &Rc<RefCell<String>>,
    menus: Vec<(String, web_sys::Element)>,
    sync: &SyncFn,
) {
    let renderer = renderer.clone();
    let palette_mode = palette_mode.clone();
    let sync = sync.clone();
    toolbar_el.add_event_listener("click", move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        let Ok(el) = target.dyn_into::<web_sys::Element>() else { return };
        // The click may land on the button or an inner node; walk up for the action.
        let button = el
            .get_attribute("data-action")
            .map(|_| el.clone())
            .or_else(|| el.closest("[data-action]").ok().flatten());
        let Some(button) = button else { return };
        let Some(action) = button.get_attribute("data-action") else { return };

        // Color buttons open the shared palette positioned under the button.
        if action == "color" || action == "bgcolor" {
            if let Some(pal) = &palette {
                *palette_mode.borrow_mut() = action.clone();
                show_palette_under(pal, &button);
            }
            return;
        }

        // Print renders the sheet into a hidden iframe and opens the native
        // print dialog (issue #17).
        if action == "print" {
            open_print(&renderer);
            return;
        }

        // Dropdown buttons open their registered menu under the button.
        if let Some((_, menu)) = menus.iter().find(|(a, _)| *a == action) {
            show_palette_under(menu, &button);
            return;
        }

        // Format Painter (issue #31): toggle the armed state, capturing the
        // active cell's style. The next selection applies it (see the window
        // mouseup handler in events.rs). The canvas cursor reflects the state.
        if action == "paintformat" {
            let armed = renderer.borrow_mut().toggle_format_painter();
            set_canvas_cursor(if armed { Some("copy") } else { None });
            return;
        }

        let mut r = renderer.borrow_mut();
        match action.as_str() {
            "undo" => r.undo(),
            "redo" => r.redo(),
            "font-bold" => r.toggle_bold(),
            "font-italic" => r.toggle_italic(),
            "underline" => r.toggle_underline(),
            "strike" => r.toggle_strike(),
            "textwrap" => r.toggle_text_wrap(),
            "merge" => r.merge_selection(),
            "clearformat" => r.clear_format(),
            // "freeze" opens a dropdown (registered in `menus`), handled above.
            "autofilter" => r.toggle_autofilter(),
            "align-left" => r.set_align("left"),
            "align-center" => r.set_align("center"),
            "align-right" => r.set_align("right"),
            // Vertical align (top/middle/bottom) is a dropdown — its items are
            // handled by `wire_valign_menu`, not here.
            _ => return,
        }
        r.render();
        drop(r);
        sync();
    });
}

/// Wire the color palette: clicking a swatch applies the color per the current
/// mode (text vs fill); clicking elsewhere closes it.
pub(crate) fn wire_palette(palette: web_sys::Element, renderer: &SharedRenderer, palette_mode: &Rc<RefCell<String>>) {
    {
        let renderer = renderer.clone();
        let palette_mode = palette_mode.clone();
        let palette_for_hide = palette.clone();
        let mut palette_el: Element = palette.clone().into();
        palette_el.add_event_listener("click", move |event: web_sys::Event| {
            let Some(target) = event.target() else { return };
            let Ok(el) = target.dyn_into::<web_sys::Element>() else { return };
            let Some(color) = el.get_attribute("data-color") else { return };
            {
                let mut r = renderer.borrow_mut();
                if *palette_mode.borrow() == "bgcolor" {
                    r.set_bgcolor(&color);
                } else {
                    r.set_text_color(&color);
                }
                r.render();
            }
            hide_palette(&palette_for_hide);
        });
    }
    // Close on outside click (but not when clicking a toolbar color button,
    // which reopens it on the same mousedown→click sequence).
    {
        let palette = palette.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Some(target) = event.target() {
                if let Ok(node) = target.clone().dyn_into::<web_sys::Node>() {
                    if palette.contains(Some(&node)) {
                        return;
                    }
                }
                // Keep it open if the mousedown is on a color toolbar button.
                if let Ok(el) = target.dyn_into::<web_sys::Element>() {
                    if let Ok(Some(btn)) = el.closest("[data-action]") {
                        if let Some(a) = btn.get_attribute("data-action") {
                            if a == "color" || a == "bgcolor" {
                                return;
                            }
                        }
                    }
                }
            }
            hide_palette(&palette);
        });
        window()
            .add_event_listener_with_callback("mousedown", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }
}

/// Build the rows for a toolbar dropdown menu. `items` are (value, label).
pub(crate) fn dropdown_menu_html(items: &[(&str, &str)]) -> String {
    let mut s = String::new();
    for (val, label) in items {
        s.push_str(&format!(
            "<div class=\"{p}-item\" data-ddval=\"{v}\" style=\"cursor:pointer;\">{l}</div>",
            p = CSS_PREFIX,
            v = val,
            l = label
        ));
    }
    s
}

/// Rows for the borders dropdown (each with a sprite icon + label).
pub(crate) fn border_menu_html() -> String {
    let items = [
        ("all", "border-all", "All borders"),
        ("outer", "border-outside", "Outer"),
        ("top", "border-top", "Top"),
        ("bottom", "border-bottom", "Bottom"),
        ("left", "border-left", "Left"),
        ("right", "border-right", "Right"),
        ("none", "border-none", "None"),
    ];
    let mut s = String::new();
    for (mode, icon, label) in items {
        s.push_str(&format!(
            "<div class=\"{p}-item\" data-border=\"{mode}\" style=\"cursor:pointer;display:flex;align-items:center;gap:6px;\">\
               <span class=\"{p}-icon\"><span class=\"{p}-icon-img {icon}\"></span></span>{label}\
             </div>",
            p = CSS_PREFIX, mode = mode, icon = icon, label = label
        ));
    }
    s
}

/// Wire the borders dropdown: a row applies that border mode to the selection.
pub(crate) fn wire_border_menu(menu: web_sys::Element, renderer: &SharedRenderer, sync: &SyncFn) {
    let renderer = renderer.clone();
    let sync = sync.clone();
    let menu_for_hide = menu.clone();
    let mut el: Element = menu.into();
    el.add_event_listener("click", move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
        let item = elx
            .get_attribute("data-border")
            .map(|_| elx.clone())
            .or_else(|| elx.closest("[data-border]").ok().flatten());
        let Some(item) = item else { return };
        let Some(mode) = item.get_attribute("data-border") else { return };
        {
            let mut r = renderer.borrow_mut();
            r.set_borders(&mode);
            r.render();
        }
        sync();
        hide_palette(&menu_for_hide);
    });
}

/// Build the freeze-panes dropdown rows (issue #18).
pub(crate) fn freeze_menu_html() -> String {
    let items = [
        ("top-row", "Freeze top row"),
        ("first-col", "Freeze first column"),
        ("panes", "Freeze panes"),
        ("none", "Unfreeze"),
    ];
    let mut s = String::new();
    for (mode, label) in items {
        s.push_str(&format!(
            "<div class=\"{p}-item\" data-freeze=\"{mode}\" style=\"cursor:pointer;display:flex;align-items:center;gap:6px;\">\
               {label}\
             </div>",
            p = CSS_PREFIX, mode = mode, label = label
        ));
    }
    s
}

/// Wire the freeze dropdown: each row sets (or clears) the frozen panes for the
/// active selection (issue #18).
pub(crate) fn wire_freeze_menu(menu: web_sys::Element, renderer: &SharedRenderer, sync: &SyncFn) {
    let renderer = renderer.clone();
    let sync = sync.clone();
    let menu_for_hide = menu.clone();
    let mut el: Element = menu.into();
    el.add_event_listener("click", move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
        let item = elx
            .get_attribute("data-freeze")
            .map(|_| elx.clone())
            .or_else(|| elx.closest("[data-freeze]").ok().flatten());
        let Some(item) = item else { return };
        let Some(mode) = item.get_attribute("data-freeze") else { return };
        {
            let mut r = renderer.borrow_mut();
            match mode.as_str() {
                "top-row" => r.freeze_top_row(),
                "first-col" => r.freeze_first_col(),
                "panes" => r.freeze_at_selection(),
                "none" => r.unfreeze(),
                _ => {
                    hide_palette(&menu_for_hide);
                    return;
                }
            }
            r.render();
        }
        sync();
        hide_palette(&menu_for_hide);
    });
}

/// Items for the vertical-align dropdown (x-spreadsheet parity). Each shows the
/// align sprite icon plus a label; `data-valign` carries the value.
pub(crate) fn valign_menu_html() -> String {
    let items = [("top", "Top"), ("middle", "Middle"), ("bottom", "Bottom")];
    let mut s = String::new();
    for (mode, label) in items {
        s.push_str(&format!(
            "<div class=\"{p}-item\" data-valign=\"{mode}\" style=\"cursor:pointer;display:flex;align-items:center;gap:6px;\">\
               <div class=\"{p}-icon\"><div class=\"{p}-icon-img align-{mode}\"></div></div>{label}\
             </div>",
            p = CSS_PREFIX, mode = mode, label = label
        ));
    }
    s
}

/// Wire the vertical-align dropdown: clicking an item applies it to the
/// selection via `set_valign` and closes the menu.
pub(crate) fn wire_valign_menu(menu: web_sys::Element, renderer: &SharedRenderer, sync: &SyncFn) {
    let renderer = renderer.clone();
    let sync = sync.clone();
    let menu_for_hide = menu.clone();
    let mut el: Element = menu.into();
    el.add_event_listener("click", move |event: web_sys::Event| {
        let Some(target) = event.target() else { return };
        let Ok(elx) = target.dyn_into::<web_sys::Element>() else { return };
        let item = elx
            .get_attribute("data-valign")
            .map(|_| elx.clone())
            .or_else(|| elx.closest("[data-valign]").ok().flatten());
        let Some(item) = item else { return };
        let Some(mode) = item.get_attribute("data-valign") else { return };
        {
            let mut r = renderer.borrow_mut();
            r.set_valign(&mode);
            r.render();
        }
        sync();
        hide_palette(&menu_for_hide);
    });
}

/// Wire a toolbar dropdown: a row click applies the value, updates the button
/// title, and closes the menu; an outside click closes it.
pub(crate) fn wire_dropdown(menu: web_sys::Element, kind: DdKind, title_id: &'static str, renderer: &SharedRenderer) {
    {
        let renderer = renderer.clone();
        let menu_for_hide = menu.clone();
        let mut menu_el: Element = menu.clone().into();
        menu_el.add_event_listener("click", move |event: web_sys::Event| {
            let Some(target) = event.target() else { return };
            let Ok(el) = target.dyn_into::<web_sys::Element>() else { return };
            let item = el
                .get_attribute("data-ddval")
                .map(|_| el.clone())
                .or_else(|| el.closest("[data-ddval]").ok().flatten());
            let Some(item) = item else { return };
            let Some(mut val) = item.get_attribute("data-ddval") else { return };

            // "Custom…" in the format dropdown prompts for a format string.
            let mut title_text = item.text_content();
            if matches!(kind, DdKind::Format) && val == "__custom__" {
                match window().prompt_with_message_and_default(
                    "Custom number format (e.g. #,##0.00, 0.0%, $#,##0.00):",
                    "#,##0.00",
                ) {
                    Ok(Some(pattern)) if !pattern.trim().is_empty() => {
                        val = pattern.trim().to_string();
                        title_text = Some(val.clone());
                    }
                    _ => {
                        hide_palette(&menu_for_hide);
                        return;
                    }
                }
            }

            {
                let mut r = renderer.borrow_mut();
                match kind {
                    DdKind::Format => r.set_format(&val),
                    DdKind::Font => r.set_font_family(&val),
                    DdKind::FontSize => {
                        if let Ok(px) = val.parse::<usize>() {
                            r.set_font_size(px);
                        }
                    }
                }
                r.render();
            }
            // Reflect the choice in the button's title.
            if let Some(title) = document().get_element_by_id(title_id) {
                title.set_text_content(title_text.as_deref());
            }
            hide_palette(&menu_for_hide);
        });
    }
    // Close on any mousedown outside this menu. (Clicking a dropdown button
    // reopens the right menu on the subsequent click event.)
    {
        let menu = menu.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Some(target) = event.target() {
                if let Ok(node) = target.dyn_into::<web_sys::Node>() {
                    if menu.contains(Some(&node)) {
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

