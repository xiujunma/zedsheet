use std::cell::RefCell;
use std::rc::Rc;

use gloo::utils::{document, window};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, HtmlElement, HtmlInputElement, HtmlTextAreaElement};

use crate::renderer::alphabets::xy2expr;

use crate::component::element::{h, Element};
use crate::component::options::Options;
use crate::component::toolbar::Toolbar;
use crate::config::CSS_PREFIX;
use crate::core::data_proxy::{DataProxy, SheetsRegistry};
use crate::renderer::table_renderer::{DragKind, TableRenderer};

// UI wiring is split across submodules; this module owns the ZedSheet shell +
// ::new orchestration and the shared types below. Each submodule does
// `use super::*`, so these `pub(crate) use` re-exports give them the shared
// types/helpers and each other's entry points.
mod util;
mod formula_bar;
mod toolbar;
mod context_menu;
mod data_validation;
mod cond_format_modal;
mod chart_modal;
mod filter_menu;
mod print;
mod find_replace;
mod bottom_bar;
mod events;

pub(crate) use util::*;
pub(crate) use formula_bar::*;
pub(crate) use toolbar::*;
pub(crate) use context_menu::*;
pub(crate) use data_validation::*;
pub(crate) use cond_format_modal::*;
pub(crate) use chart_modal::*;
pub(crate) use filter_menu::*;
pub(crate) use print::*;
pub(crate) use find_replace::*;
pub(crate) use bottom_bar::*;
pub(crate) use events::*;

/// Which attribute a toolbar dropdown applies.
#[derive(Clone, Copy)]
pub(crate) enum DdKind {
    Format,
    Font,
    FontSize,
}

/// An in-progress header-resize or scrollbar drag.
#[derive(Clone, Copy)]
pub(crate) struct DragState {
    pub(crate) kind: DragKind,
    pub(crate) start_x: f64,
    pub(crate) start_y: f64,
    pub(crate) start_size: f64,
}

pub(crate) type SharedRenderer = Rc<RefCell<TableRenderer>>;
pub(crate) type EditingCell = Rc<RefCell<Option<(usize, usize)>>>;
pub(crate) type Sheets = Rc<RefCell<Vec<DataProxy>>>;
pub(crate) type ActiveSheet = Rc<RefCell<usize>>;
/// Refreshes the formula bar + toolbar state from the active cell.
pub(crate) type SyncFn = Rc<dyn Fn()>;
/// Open-a-dialog handle, populated once its modal is mounted (issues #9, #11).
pub(crate) type OpenHandle = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
/// Serializes the whole workbook (all sheets) to a JSON string (issue #20).
pub(crate) type GetDataFn = Rc<dyn Fn() -> String>;
/// Replaces the whole workbook from a JSON string and re-renders (issue #20).
pub(crate) type LoadDataFn = Rc<dyn Fn(&str)>;
/// Snapshot of the live active sheet (issue #15: CSV exports the active sheet).
pub(crate) type ActiveSheetFn = Rc<dyn Fn() -> DataProxy>;

/// Top-level spreadsheet. Builds the DOM shell (toolbar + sheet canvas + bottom
/// bar), owns the shared renderer, and wires pointer/keyboard interaction.
pub struct ZedSheet {
    renderer: SharedRenderer,
    get_data: GetDataFn,
    load_data: LoadDataFn,
    active_sheet: ActiveSheetFn,
}

impl ZedSheet {
    pub fn new(selector: &str, options: Options, mut data: DataProxy) -> Self {
        let target = document()
            .query_selector(selector)
            .expect("query_selector failed")
            .expect("target element not found");

        let mut root = h("div", Some(CSS_PREFIX));

        let mut toolbar_el: Option<Element> = None;
        if options.show_toolbar {
            let toolbar = Toolbar::new();
            let mut el = toolbar.element().clone();
            root.append_child(&mut el);
            toolbar_el = Some(el);
        }

        // Excel-style formula bar: name box + cancel/confirm + fx + input.
        let mut fbar = h("div", Some(&format!("{}-formula-bar", CSS_PREFIX)));
        fbar.set_inner_html(formula_bar_html());
        let _ = fbar.el.as_ref().map(|e| {
            let _ = e.set_attribute(
                "style",
                "display:flex;align-items:center;height:25px;border-bottom:1px solid #e0e2e4;background:#fff;font-size:13px;",
            );
        });
        let fbar_node = fbar.el.clone();
        root.append_child(&mut fbar);

        // Sheet area: a positioned wrapper holding the canvas + editor overlay.
        let mut sheet_el = h("div", Some(&format!("{}-sheet", CSS_PREFIX)));
        let mut canvas_el = h("canvas", Some(&format!("{}-table", CSS_PREFIX)));
        let mut editor_el = h("textarea", Some(&format!("{}-editor", CSS_PREFIX)));
        let mut cmenu_el = h("div", Some(&format!("{}-contextmenu", CSS_PREFIX)));
        cmenu_el.set_inner_html(context_menu_html());
        let _ = cmenu_el.el.as_ref().map(|e| {
            let _ = e.set_attribute("style", "display:none;width:180px;");
        });
        sheet_el.append_child(&mut canvas_el);
        sheet_el.append_child(&mut editor_el);
        sheet_el.append_child(&mut cmenu_el);
        root.append_child(&mut sheet_el);

        // Bottom bar: add(+) button followed by the sheet-tab menu.
        let mut bottom_menu_el: Option<Element> = None;
        let mut bottom_add_el: Option<Element> = None;
        // Raw tab-strip element, kept so `load_data` can refresh the tabs (#20).
        let mut tab_menu_node: Option<web_sys::Element> = None;
        if options.show_bottom_bar {
            let mut bottom = h("div", Some(&format!("{}-bottombar", CSS_PREFIX)));
            // Sprite-based add(+) icon, matching the reference bottombar
            // (`new Icon('add')`). A text "+" inside `.{p}-icon` gets clipped
            // to a dot by the class's 18px overflow-hidden box.
            let mut add_btn = h("span", Some(&format!("{}-icon", CSS_PREFIX)));
            add_btn.set_inner_html(format!("<div class=\"{}-icon-img add\"></div>", CSS_PREFIX));
            let _ = add_btn.el.as_ref().map(|e| {
                let _ = e.set_attribute("style", "cursor:pointer;margin:11px 4px 11px 8px;");
            });
            let mut menu = h("ul", Some(&format!("{}-menu", CSS_PREFIX)));
            bottom.append_child(&mut add_btn);
            bottom.append_child(&mut menu);
            root.append_child(&mut bottom);
            tab_menu_node = menu.el.clone();
            bottom_menu_el = menu.el.clone().map(Element::from);
            bottom_add_el = add_btn.el.clone().map(Element::from);
        }

        // Color picker palette (shared by the text-color and fill-color buttons).
        let mut palette_el = h("div", Some(&format!("{}-color-palette", CSS_PREFIX)));
        palette_el.set_inner_html(color_palette_html());
        let _ = palette_el.el.as_ref().map(|e| {
            let _ = e.set_attribute(
                "style",
                "display:none;position:absolute;z-index:200;background:#fff;border:1px solid #ccc;padding:5px;box-shadow:1px 2px 5px rgba(0,0,0,0.15);",
            );
        });
        let palette_node = palette_el.el.clone();
        root.append_child(&mut palette_el);

        // Toolbar dropdown menus (format / font / fontsize).
        let mut dropdown_nodes: Vec<(String, web_sys::Element, DdKind, &'static str)> = Vec::new();
        let dropdowns: [(&str, DdKind, &'static str, &str, Vec<(&str, &str)>); 3] = [
            (
                "dd-format",
                DdKind::Format,
                "zs-dd-format",
                "120px",
                vec![
                    ("normal", "Normal"),
                    ("number", "Number 1,000.12"),
                    ("percent", "Percent 10.12%"),
                    ("rmb", "RMB ￥10.00"),
                    ("usd", "USD $10.00"),
                    ("eur", "EUR €10.00"),
                    ("date", "Date 2024-01-15"),
                    ("time", "Time 13:30:00"),
                    ("datetime", "Date Time"),
                    ("__custom__", "Custom…"),
                ],
            ),
            (
                "dd-font",
                DdKind::Font,
                "zs-dd-font",
                "120px",
                vec![
                    ("Arial", "Arial"),
                    ("Helvetica", "Helvetica"),
                    ("Source Sans Pro", "Source Sans Pro"),
                    ("Comic Sans MS", "Comic Sans MS"),
                    ("Courier New", "Courier New"),
                    ("Verdana", "Verdana"),
                    ("Lato", "Lato"),
                ],
            ),
            (
                "dd-fontsize",
                DdKind::FontSize,
                "zs-dd-fontsize",
                "60px",
                vec![
                    ("10", "10"), ("11", "11"), ("12", "12"), ("13", "13"),
                    ("14", "14"), ("16", "16"), ("18", "18"), ("20", "20"),
                    ("24", "24"), ("28", "28"), ("32", "32"), ("36", "36"), ("48", "48"),
                ],
            ),
        ];
        for (action, kind, title_id, width, items) in dropdowns {
            let mut menu = h("div", Some(&format!("{}-dropdown-menu", CSS_PREFIX)));
            menu.set_inner_html(dropdown_menu_html(&items));
            let _ = menu.el.as_ref().map(|e| {
                let _ = e.set_attribute(
                    "style",
                    &format!("display:none;position:absolute;z-index:200;background:#fff;border:1px solid #ccc;box-shadow:1px 2px 5px 2px rgba(51,51,51,0.15);max-height:300px;overflow:auto;width:{};", width),
                );
            });
            if let Some(node) = menu.el.clone() {
                dropdown_nodes.push((action.to_string(), node, kind, title_id));
            }
            root.append_child(&mut menu);
        }

        // Formula-bar fx function picker menu.
        let mut fx_menu = h("div", Some(&format!("{}-dropdown-menu", CSS_PREFIX)));
        fx_menu.set_inner_html(fx_menu_html());
        let _ = fx_menu.el.as_ref().map(|e| {
            let _ = e.set_attribute(
                "style",
                "display:none;position:absolute;z-index:200;background:#fff;border:1px solid #ccc;box-shadow:1px 2px 5px 2px rgba(51,51,51,0.15);max-height:300px;overflow:auto;width:120px;",
            );
        });
        let fx_menu_node = fx_menu.el.clone();
        root.append_child(&mut fx_menu);

        // Borders dropdown menu (opened by the toolbar borders button).
        let mut border_menu = h("div", Some(&format!("{}-dropdown-menu", CSS_PREFIX)));
        border_menu.set_inner_html(border_menu_html());
        let _ = border_menu.el.as_ref().map(|e| {
            let _ = e.set_attribute(
                "style",
                "display:none;position:absolute;z-index:200;background:#fff;border:1px solid #ccc;box-shadow:1px 2px 5px 2px rgba(51,51,51,0.15);width:130px;",
            );
        });
        let border_menu_node = border_menu.el.clone();
        root.append_child(&mut border_menu);

        // Freeze-panes dropdown menu (opened by the toolbar freeze button, #18).
        let mut freeze_menu = h("div", Some(&format!("{}-dropdown-menu", CSS_PREFIX)));
        freeze_menu.set_inner_html(freeze_menu_html());
        let _ = freeze_menu.el.as_ref().map(|e| {
            let _ = e.set_attribute(
                "style",
                "display:none;position:absolute;z-index:200;background:#fff;border:1px solid #ccc;box-shadow:1px 2px 5px 2px rgba(51,51,51,0.15);width:150px;",
            );
        });
        let freeze_menu_node = freeze_menu.el.clone();
        root.append_child(&mut freeze_menu);

        // Toolbar tooltip (shown on hover over a button).
        let mut tooltip_el = h("div", Some(&format!("{}-tooltip", CSS_PREFIX)));
        let _ = tooltip_el.el.as_ref().map(|e| {
            let _ = e.set_attribute(
                "style",
                "display:none;transform:translateX(-50%);white-space:nowrap;pointer-events:none;",
            );
        });
        let tooltip_node = tooltip_el.el.clone();
        root.append_child(&mut tooltip_el);

        // Find & replace panel (opened with Ctrl/Cmd+F).
        let mut find_el = h("div", Some("zs-findbar"));
        find_el.set_inner_html(find_panel_html());
        let _ = find_el.el.as_ref().map(|e| {
            let _ = e.set_attribute(
                "style",
                "display:none;position:absolute;top:70px;right:24px;z-index:300;background:#fff;border:1px solid #ccc;box-shadow:1px 2px 8px rgba(0,0,0,0.2);padding:8px;width:300px;font-size:13px;",
            );
        });
        let find_node = find_el.el.clone();
        root.append_child(&mut find_el);

        // Clear any prior mount so repeated init calls don't stack instances.
        target.set_inner_html("");
        let mut target_el: Element = target.into();
        target_el.append_child(&mut root);

        // Size the canvas to the available area.
        let toolbar_h = if options.show_toolbar { 41f64 } else { 0f64 };
        let bottom_h = if options.show_bottom_bar { 41f64 } else { 0f64 };
        let fbar_h = 26f64;
        let (cw, ch) = client_box(&target_el);
        let width = if cw > 0f64 { cw } else { 900f64 };
        let height = (if ch > 0f64 { ch } else { 600f64 } - toolbar_h - bottom_h - fbar_h).max(200f64);

        let canvas = canvas_el
            .el
            .clone()
            .unwrap()
            .dyn_into::<HtmlCanvasElement>()
            .expect("canvas element");

        // All sheets live here; the renderer holds a copy of the active one.
        let sheets: Sheets = Rc::new(RefCell::new(vec![data.clone()]));
        // Wire the workbook-wide sheets registry on every DataProxy so
        // cross-sheet formulas (`Sheet2!A1`, issue #4) can resolve against
        // the named sheet. Each DataProxy gets the same `Rc<RefCell<Vec<…>>>`
        // so the registry is shared across clones and sheet operations.
        for d in sheets.borrow_mut().iter_mut() {
            d.set_sheets(&sheets);
        }
        // The renderer's own DataProxy is the *original* `data`, not the
        // clone inside the Vec — wire it too so the active-sheet evaluator
        // can see peers. (Clones of a wired DataProxy copy the Weak, so
        // subsequent sheet switches stay wired automatically.)
        data.set_sheets(&sheets);
        let active: ActiveSheet = Rc::new(RefCell::new(0));

        let mut renderer = TableRenderer::new(canvas, width, height, data);
        renderer.set_selector(0, 0, 0, 0);
        renderer.render();

        let renderer: SharedRenderer = Rc::new(RefCell::new(renderer));

        let textarea: HtmlTextAreaElement = editor_el
            .el
            .clone()
            .unwrap()
            .dyn_into::<HtmlTextAreaElement>()
            .expect("textarea element");
        init_editor_style(&textarea);

        // Sibling of the editor textarea: red error label shown when a
        // commit is rejected by a data-validation rule (issue #9).
        let mut editor_error_el = h("div", Some("zs-editor-error"));
        let _ = editor_error_el.el.as_ref().map(|e| {
            let _ = e.set_attribute("class", "zs-editor-error zedsheet-toast");
            let _ = e.set_attribute(
                "style",
                "display:none;position:absolute;font-size:12px;color:#b71c1c;\
                 background:#fff5f5;padding:4px 8px;border:1px solid #e53935;\
                 border-radius:3px;z-index:101;max-width:240px;pointer-events:none;",
            );
        });
        let editor_error_node = editor_error_el
            .el
            .clone()
            .and_then(|e| e.dyn_into::<HtmlElement>().ok());
        root.append_child(&mut editor_error_el);

        let editing: EditingCell = Rc::new(RefCell::new(None));

        // Toast for top-of-screen validation errors from the formula bar
        // (and any other commit path that can't keep the editor open).
        let mut toast_el = h("div", Some("zs-dv-toast"));
        let _ = toast_el.el.as_ref().map(|e| {
            let _ = e.set_attribute("class", "zs-dv-toast zedsheet-toast");
            let _ = e.set_attribute(
                "style",
                "display:none;position:fixed;top:12px;left:50%;transform:translateX(-50%);\
                 z-index:1200;font-size:13px;background:#fff5f5;color:#b71c1c;\
                 border:1px solid #e53935;border-radius:4px;padding:6px 12px;\
                 box-shadow:0 2px 6px rgba(0,0,0,0.2);",
            );
        });
        let toast_node = toast_el
            .el
            .clone()
            .and_then(|e| e.dyn_into::<HtmlElement>().ok());
        root.append_child(&mut toast_el);

        let palette_mode: Rc<RefCell<String>> = Rc::new(RefCell::new(String::new()));

        // Resolve the formula-bar inputs and build the UI-sync closure.
        let name_box = fbar_node
            .as_ref()
            .and_then(|f| f.query_selector(".zs-name-box").ok().flatten())
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let formula_input = fbar_node
            .as_ref()
            .and_then(|f| f.query_selector(".zs-formula-input").ok().flatten())
            .and_then(|e| e.dyn_into::<HtmlInputElement>().ok());
        let toolbar_node = toolbar_el.as_ref().and_then(|e| e.el.clone());

        let sync: SyncFn = {
            let renderer = renderer.clone();
            let name_box = name_box.clone();
            let formula_input = formula_input.clone();
            let toolbar_node = toolbar_node.clone();
            // Captured for change-driven persistence (issue #20).
            let persist_selector = selector.to_string();
            let persist_sheets = sheets.clone();
            let persist_active = active.clone();
            Rc::new(move || {
                let r = renderer.borrow();
                let s = r.get_selector();
                let (ri, ci) = (s.ri, s.ci);
                if let Some(nb) = &name_box {
                    nb.set_value(&xy2expr(ci, ri));
                }
                if let Some(fi) = &formula_input {
                    fi.set_value(&r.cell_text_at(ri, ci));
                }
                let style = r.data.get_cell_style(ri, ci);
                if let Some(tb) = &toolbar_node {
                    toggle_disabled(tb, "undo", !r.can_undo());
                    toggle_disabled(tb, "redo", !r.can_redo());
                    toggle_active(tb, "font-bold", style.bold);
                    toggle_active(tb, "font-italic", style.italic);
                    toggle_active(tb, "underline", style.underline);
                    toggle_active(tb, "strike", style.strike);
                    toggle_active(tb, "textwrap", style.text_wrap);
                    toggle_active(tb, "align-left", style.align == "left");
                    toggle_active(tb, "align-center", style.align == "center");
                    toggle_active(tb, "align-right", style.align == "right");
                    toggle_active(tb, "align-top", style.valign == "top");
                    toggle_active(tb, "align-middle", style.valign == "middle");
                    toggle_active(tb, "align-bottom", style.valign == "bottom");
                }
                set_text_by_id("zs-dd-font", &style.font_family);
                set_text_by_id("zs-dd-fontsize", &style.font_size.to_string());
                set_text_by_id("zs-dd-format", format_label(&style.format));

                // Persist on actual data changes (issue #20). `note_change`
                // de-dupes against the last snapshot, so pure selection moves
                // — which don't alter the serialized workbook — are no-ops.
                drop(r);
                let json = current_workbook_json(&renderer, &persist_sheets, &persist_active);
                crate::persist::note_change(&persist_selector, &json);
            })
        };

        // Handle to open the Data Validation modal — populated after the
        // modal is mounted (issue #9). The context menu's "validation"
        // arm calls this when the user picks the menu item.
        let dv_open: OpenHandle = Rc::new(RefCell::new(None));
        // Same pattern for the Conditional Formatting dialog (issue #11).
        let cf_open: OpenHandle = Rc::new(RefCell::new(None));
        // …and the Charts dialog (issue #16).
        let chart_open: OpenHandle = Rc::new(RefCell::new(None));

        // List-validity popover (issue #9): a single <ul> reused across
        // cells. Mounted hidden; the canvas mousedown handler (wired by
        // `wire_events` below) opens it when the user clicks the ▼ glyph.
        let mut list_popover = h("ul", Some("zs-listpop"));
        list_popover.set_inner_html(String::new());
        let list_popover_node: Option<web_sys::Element> =
            list_popover.el.clone().and_then(|e| e.dyn_into().ok());
        let _ = list_popover_node.as_ref().map(|n| {
            let _ = n.set_attribute(
                "style",
                "display:none;position:absolute;z-index:900;background:#fff;\
                 border:1px solid #999;box-shadow:1px 2px 6px rgba(0,0,0,0.2);\
                 padding:4px 0;margin:0;list-style:none;max-height:200px;\
                 overflow-y:auto;font-size:13px;min-width:140px;",
            );
        });
        root.append_child(&mut list_popover);
        let list_popover_visible: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));

        // Click handler on a <li>: set the cell value (always passes
        // validation because list values are in the validator's CSV).
        if let Some(ref pop) = list_popover_node {
            let pop_for_listener = pop.clone();
            let pop_for_hide = pop.clone();
            let renderer_for_pop = renderer.clone();
            let pop_visible_for_cb = list_popover_visible.clone();
            let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                let target = event.target();
                let Some(target_el) = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
                    return;
                };
                let value = target_el.get_attribute("data-value").unwrap_or_default();
                if value.is_empty() {
                    return;
                }
                let (ri, ci) = {
                    let r = renderer_for_pop.borrow();
                    let s = r.get_selector();
                    (s.ri, s.ci)
                };
                let _ = renderer_for_pop.borrow_mut().set_cell_text_at(ri, ci, &value);
                let _ = renderer_for_pop.borrow_mut().render();
                let _ = pop_for_hide.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
                *pop_visible_for_cb.borrow_mut() = false;
            });
            let _ = pop_for_listener.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref());
            cb.forget();
        }
        // Outside click closes the popover.
        {
            let pop = list_popover_node.clone();
            let pop_visible_for_outside = list_popover_visible.clone();
            let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                if !*pop_visible_for_outside.borrow() {
                    return;
                }
                let Some(pop_el) = pop.as_ref() else { return };
                let target = event.target();
                let Some(target_el) = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
                    return;
                };
                if !pop_el.contains(Some(&target_el)) {
                    let _ = pop_el.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
                    *pop_visible_for_outside.borrow_mut() = false;
                }
            });
            let _ = window()
                .add_event_listener_with_callback("mousedown", cb.as_ref().unchecked_ref());
            cb.forget();
        }

        // AutoFilter dropdown (issue #10): a single reused panel opened from
        // the ▼ glyph on a filter-range header cell (wired in `wire_events`).
        let mut filter_menu_el = h("div", Some("zs-filtermenu"));
        let filter_menu_node: Option<web_sys::Element> =
            filter_menu_el.el.clone().and_then(|e| e.dyn_into().ok());
        let _ = filter_menu_node.as_ref().map(|n| {
            let _ = n.set_attribute(
                "style",
                "display:none;position:absolute;z-index:900;background:#fff;\
                 border:1px solid #999;box-shadow:1px 2px 6px rgba(0,0,0,0.2);\
                 padding:4px 0;margin:0;font-size:13px;min-width:180px;",
            );
        });
        root.append_child(&mut filter_menu_el);
        let filter_menu_visible: Rc<RefCell<bool>> = Rc::new(RefCell::new(false));
        if let Some(ref fm) = filter_menu_node {
            wire_filter_menu(fm.clone(), &renderer, &sync, &filter_menu_visible);
        }
        // Outside click closes the filter menu.
        {
            let fm = filter_menu_node.clone();
            let fm_visible_for_outside = filter_menu_visible.clone();
            let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
                if !*fm_visible_for_outside.borrow() {
                    return;
                }
                let Some(fm_el) = fm.as_ref() else { return };
                let target = event.target();
                let Some(target_el) = target.and_then(|t| t.dyn_into::<web_sys::Element>().ok()) else {
                    return;
                };
                if !fm_el.contains(Some(&target_el)) {
                    let _ = fm_el.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
                    *fm_visible_for_outside.borrow_mut() = false;
                }
            });
            let _ = window()
                .add_event_listener_with_callback("mousedown", cb.as_ref().unchecked_ref());
            cb.forget();
        }

        wire_events(
            &mut canvas_el,
            &renderer,
            &textarea,
            &editing,
            editor_error_node.clone(),
            list_popover_node.clone(),
            list_popover_visible.clone(),
            filter_menu_node.clone(),
            filter_menu_visible.clone(),
            &sync,
        );
        if let Some(menu_node) = cmenu_el.el.clone() {
            wire_context_menu(&mut canvas_el, menu_node, &renderer, &sync, dv_open.clone(), cf_open.clone(), chart_open.clone());
        }
        if let Some(fb) = fbar_node.clone() {
            wire_formula_bar(
                fb,
                &renderer,
                &textarea,
                editor_error_node.clone(),
                &editing,
                &sync,
                fx_menu_node,
                toast_node,
            );
        }
        // Map of toolbar action → dropdown menu node (for show-on-click).
        let mut menus: Vec<(String, web_sys::Element)> = dropdown_nodes
            .iter()
            .map(|(a, n, _, _)| (a.clone(), n.clone()))
            .collect();
        if let Some(bm) = border_menu_node.clone() {
            menus.push(("borders".to_string(), bm));
        }
        if let Some(fm) = freeze_menu_node.clone() {
            menus.push(("freeze".to_string(), fm));
        }

        if let Some(mut tb) = toolbar_el {
            wire_toolbar(&mut tb, &renderer, palette_node.clone(), &palette_mode, menus, &sync);
        }
        if let Some(bm) = border_menu_node {
            wire_border_menu(bm, &renderer, &sync);
        }
        if let Some(fm) = freeze_menu_node {
            wire_freeze_menu(fm, &renderer, &sync);
        }
        if let (Some(tb), Some(tip)) = (toolbar_node.clone(), tooltip_node) {
            wire_tooltip(tb, tip);
        }
        if let Some(pal) = palette_node {
            wire_palette(pal, &renderer, &palette_mode);
        }
        for (_, node, kind, title_id) in dropdown_nodes {
            wire_dropdown(node, kind, title_id, &renderer);
        }
        if let (Some(menu), Some(add)) = (bottom_menu_el, bottom_add_el) {
            wire_bottombar(
                menu,
                add,
                &renderer,
                &sheets,
                &active,
                &textarea,
                editor_error_node,
                &editing,
                &sync,
            );
        }
        if let Some(fp) = find_node {
            wire_find(fp, &renderer, &sync);
        }

        // Data Validation modal (issue #9): mount once at root, hidden by
        // default; opened by the right-click context menu.
        let mut dv_modal = h("div", Some("zs-dv-modal-root"));
        dv_modal.set_inner_html(data_validation_modal_html());
        let dv_modal_node: Option<web_sys::Element> =
            dv_modal.el.clone().and_then(|e| e.dyn_into().ok());
        root.append_child(&mut dv_modal);
        if let Some(ref node) = dv_modal_node {
            // Resolve the inner `.zs-dv-root` (the one with the inline
            // `display:none`); the outer wrapper div is the node we pass
            // to listeners for query_selector, but the visibility toggle
            // happens on the inner.
            let inner_for_open: web_sys::Element = node
                .query_selector(".zs-dv-root")
                .ok()
                .flatten()
                .unwrap_or_else(|| node.clone());
            let handle = wire_data_validation_modal(node.clone(), &renderer);
            let modal_for_open = inner_for_open;
            let renderer_for_open = renderer.clone();
            let handle_for_open = handle.clone();
            *dv_open.borrow_mut() = Some(Rc::new(move || {
                open_dv_modal(&modal_for_open, &renderer_for_open, &handle_for_open);
            }));
        }

        // Conditional Formatting modal (issue #11): mounted hidden at root,
        // opened by the right-click context menu via `cf_open`.
        let mut cf_modal = h("div", Some("zs-cf-modal-root"));
        cf_modal.set_inner_html(cond_format_modal_html());
        let cf_modal_node: Option<web_sys::Element> =
            cf_modal.el.clone().and_then(|e| e.dyn_into().ok());
        root.append_child(&mut cf_modal);
        if let Some(ref node) = cf_modal_node {
            let inner: web_sys::Element = node
                .query_selector(".zs-cf-root")
                .ok()
                .flatten()
                .unwrap_or_else(|| node.clone());
            wire_cond_format_modal(inner.clone(), &renderer, &sync);
            let renderer_for_open = renderer.clone();
            *cf_open.borrow_mut() = Some(Rc::new(move || {
                open_cf_modal(&inner, &renderer_for_open);
            }));
        }

        // Charts modal (issue #16): mounted hidden at root, opened by the
        // right-click context menu via `chart_open`.
        let mut chart_modal = h("div", Some("zs-chart-modal-root"));
        chart_modal.set_inner_html(chart_modal_html());
        let chart_modal_node: Option<web_sys::Element> =
            chart_modal.el.clone().and_then(|e| e.dyn_into().ok());
        root.append_child(&mut chart_modal);
        if let Some(ref node) = chart_modal_node {
            let inner: web_sys::Element = node
                .query_selector(".zs-chart-root")
                .ok()
                .flatten()
                .unwrap_or_else(|| node.clone());
            wire_chart_modal(inner.clone(), &renderer, &sync);
            let renderer_for_open = renderer.clone();
            *chart_open.borrow_mut() = Some(Rc::new(move || {
                open_chart_modal(&inner, &renderer_for_open);
            }));
        }

        // List-validity popover (issue #9): a single <ul> reused across
        // cells. Mounted hidden; shown when the user clicks the ▼ glyph on
        // a list-valid cell. Clicking a <li> sets the cell value.
        // (Setup is done earlier; only the wiring callback references the
        // already-declared `list_popover_node` / `list_popover_visible`.)

        // Workbook get/load closures backing the public JS persistence API
        // (issue #20). They capture the shared renderer, the sheet registry,
        // the active index, the tab strip, and `sync`, so `lib` can read or
        // replace the whole workbook without re-deriving any of that wiring.
        let get_data: GetDataFn = {
            let renderer = renderer.clone();
            let sheets = sheets.clone();
            let active = active.clone();
            Rc::new(move || current_workbook_json(&renderer, &sheets, &active))
        };
        let active_sheet: ActiveSheetFn = {
            let renderer = renderer.clone();
            Rc::new(move || renderer.borrow().data_clone())
        };
        let load_data: LoadDataFn = {
            let renderer = renderer.clone();
            let sheets = sheets.clone();
            let active = active.clone();
            let sync = sync.clone();
            let tab_menu = tab_menu_node.clone();
            Rc::new(move |json: &str| {
                let loaded = crate::core::workbook::deserialize(json);
                *sheets.borrow_mut() = loaded;
                // Re-wire the shared registry on every restored sheet so
                // cross-sheet formulas (`Sheet2!A1`) keep resolving (issue #4).
                for d in sheets.borrow_mut().iter_mut() {
                    d.set_sheets(&sheets);
                }
                *active.borrow_mut() = 0;
                let first = sheets.borrow()[0].clone();
                {
                    let mut r = renderer.borrow_mut();
                    r.set_data(first);
                    r.set_selector(0, 0, 0, 0);
                    r.render();
                }
                if let Some(menu) = &tab_menu {
                    let names: Vec<String> =
                        sheets.borrow().iter().map(|d| d.name.clone()).collect();
                    render_tabs(menu, &names, 0);
                }
                sync();
            })
        };

        sync();

        Self {
            renderer,
            get_data,
            load_data,
            active_sheet,
        }
    }

    pub fn renderer(&self) -> SharedRenderer {
        self.renderer.clone()
    }

    /// Closure that serializes the whole workbook to a JSON array (issue #20).
    pub(crate) fn get_data_fn(&self) -> GetDataFn {
        self.get_data.clone()
    }

    /// Closure that replaces the whole workbook from JSON, re-rendering and
    /// refreshing the sheet tabs (issue #20).
    pub(crate) fn load_data_fn(&self) -> LoadDataFn {
        self.load_data.clone()
    }

    /// Closure that snapshots the live active sheet (issue #15).
    pub(crate) fn active_sheet_fn(&self) -> ActiveSheetFn {
        self.active_sheet.clone()
    }

    /// The workbook-wide sheets registry, so the host can toggle per-sheet
    /// options like read-only mode from outside the renderer (issue #24).
    pub(crate) fn sheets_registry(&self) -> Option<SheetsRegistry> {
        // `data.sheets` is a Weak back-reference (issue #4); upgrade it to a
        // strong Rc for the caller.
        self.renderer.borrow().data.sheets.as_ref().and_then(|w| w.upgrade())
    }
}

/// Serialize the live workbook — the renderer's (possibly-unsaved) active sheet
/// plus the stored copies of the others — to a JSON array string (issue #20).
fn current_workbook_json(renderer: &SharedRenderer, sheets: &Sheets, active: &ActiveSheet) -> String {
    let idx = *active.borrow();
    let live = renderer.borrow().data_clone();
    let arr: Vec<serde_json::Value> = sheets
        .borrow()
        .iter()
        .enumerate()
        .map(|(i, s)| if i == idx { live.get_data() } else { s.get_data() })
        .collect();
    serde_json::to_string(&serde_json::Value::Array(arr)).unwrap_or_else(|_| "[]".to_string())
}

