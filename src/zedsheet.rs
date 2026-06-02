use std::cell::RefCell;
use std::rc::Rc;

use gloo::utils::{document, window};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, HtmlElement, HtmlInputElement, HtmlTextAreaElement, KeyboardEvent, MouseEvent, WheelEvent};

use crate::renderer::alphabets::{exp2xy, xy2expr};

use crate::component::element::{h, Element};
use crate::component::options::Options;
use crate::component::toolbar::Toolbar;
use crate::config::CSS_PREFIX;
use crate::core::data_proxy::{DataProxy, SheetsRegistry};
use crate::renderer::table_renderer::{DragKind, TableRenderer};

/// Which attribute a toolbar dropdown applies.
#[derive(Clone, Copy)]
enum DdKind {
    Format,
    Font,
    FontSize,
}

/// An in-progress header-resize or scrollbar drag.
#[derive(Clone, Copy)]
struct DragState {
    kind: DragKind,
    start_x: f64,
    start_y: f64,
    start_size: f64,
}

type SharedRenderer = Rc<RefCell<TableRenderer>>;
type EditingCell = Rc<RefCell<Option<(usize, usize)>>>;
type Sheets = Rc<RefCell<Vec<DataProxy>>>;
type ActiveSheet = Rc<RefCell<usize>>;
/// Refreshes the formula bar + toolbar state from the active cell.
type SyncFn = Rc<dyn Fn()>;

/// Top-level spreadsheet. Builds the DOM shell (toolbar + sheet canvas + bottom
/// bar), owns the shared renderer, and wires pointer/keyboard interaction.
pub struct ZedSheet {
    renderer: SharedRenderer,
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
        if options.show_bottom_bar {
            let mut bottom = h("div", Some(&format!("{}-bottombar", CSS_PREFIX)));
            let mut add_btn = h("span", Some(&format!("{}-icon", CSS_PREFIX)));
            add_btn.set_inner_html("&#43;".to_string()); // plus sign
            let _ = add_btn.el.as_ref().map(|e| {
                let _ = e.set_attribute("style", "cursor:pointer;padding:0 10px;font-size:18px;line-height:40px;display:inline-block;");
            });
            let mut menu = h("ul", Some(&format!("{}-menu", CSS_PREFIX)));
            bottom.append_child(&mut add_btn);
            bottom.append_child(&mut menu);
            root.append_child(&mut bottom);
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
            let _ = e.set_attribute("class", "zs-editor-error x-spreadsheet-toast");
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
            let _ = e.set_attribute("class", "zs-dv-toast x-spreadsheet-toast");
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
            })
        };

        // Handle to open the Data Validation modal — populated after the
        // modal is mounted (issue #9). The context menu's "validation"
        // arm calls this when the user picks the menu item.
        let dv_open: Rc<RefCell<Option<Rc<dyn Fn()>>>> =
            Rc::new(RefCell::new(None));

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

        wire_events(
            &mut canvas_el,
            &renderer,
            &textarea,
            &editing,
            editor_error_node,
            list_popover_node.clone(),
            list_popover_visible.clone(),
            &sync,
        );
        if let Some(menu_node) = cmenu_el.el.clone() {
            wire_context_menu(&mut canvas_el, menu_node, &renderer, dv_open.clone());
        }
        if let Some(fb) = fbar_node.clone() {
            wire_formula_bar(fb, &renderer, &sync, fx_menu_node, toast_node);
        }
        // Map of toolbar action → dropdown menu node (for show-on-click).
        let mut menus: Vec<(String, web_sys::Element)> = dropdown_nodes
            .iter()
            .map(|(a, n, _, _)| (a.clone(), n.clone()))
            .collect();
        if let Some(bm) = border_menu_node.clone() {
            menus.push(("borders".to_string(), bm));
        }

        if let Some(mut tb) = toolbar_el {
            wire_toolbar(&mut tb, &renderer, palette_node.clone(), &palette_mode, menus, &sync);
        }
        if let Some(bm) = border_menu_node {
            wire_border_menu(bm, &renderer, &sync);
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
            wire_bottombar(menu, add, &renderer, &sheets, &active, &sync);
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

        // List-validity popover (issue #9): a single <ul> reused across
        // cells. Mounted hidden; shown when the user clicks the ▼ glyph on
        // a list-valid cell. Clicking a <li> sets the cell value.
        // (Setup is done earlier; only the wiring callback references the
        // already-declared `list_popover_node` / `list_popover_visible`.)

        sync();

        Self { renderer }
    }

    pub fn renderer(&self) -> SharedRenderer {
        self.renderer.clone()
    }

    /// The workbook-wide sheets registry, so the host can toggle per-sheet
    /// options like read-only mode from outside the renderer (issue #24).
    pub(crate) fn sheets_registry(&self) -> Option<SheetsRegistry> {
        // `data.sheets` is a Weak back-reference (issue #4); upgrade it to a
        // strong Rc for the caller.
        self.renderer.borrow().data.sheets.as_ref().and_then(|w| w.upgrade())
    }
}

fn client_box(el: &Element) -> (f64, f64) {
    el.el
        .as_ref()
        .and_then(|e| {
            e.dyn_ref::<web_sys::HtmlElement>()
                .map(|h| (h.client_width() as f64, h.client_height() as f64))
        })
        .unwrap_or((0f64, 0f64))
}

/// Toggle the `active` class on a toolbar button identified by its data-action.
fn toggle_active(toolbar: &web_sys::Element, action: &str, on: bool) {
    if let Ok(Some(btn)) = toolbar.query_selector(&format!("[data-action=\"{}\"]", action)) {
        let cl = btn.class_list();
        if on {
            let _ = cl.add_1("active");
        } else {
            let _ = cl.remove_1("active");
        }
    }
}

/// Toggle the `disabled` class on a toolbar button identified by its data-action.
fn toggle_disabled(toolbar: &web_sys::Element, action: &str, on: bool) {
    if let Ok(Some(btn)) = toolbar.query_selector(&format!("[data-action=\"{}\"]", action)) {
        let cl = btn.class_list();
        if on {
            let _ = cl.add_1("disabled");
        } else {
            let _ = cl.remove_1("disabled");
        }
    }
}

/// Set the text content of an element by id (used for dropdown titles).
fn set_text_by_id(id: &str, text: &str) {
    if let Some(el) = document().get_element_by_id(id) {
        el.set_text_content(Some(text));
    }
}

/// Human label for a format key, shown in the format dropdown title.
fn format_label(key: &str) -> &'static str {
    match key {
        "number" => "Number 1,000.12",
        "percent" => "Percent 10.12%",
        "rmb" => "RMB ￥10.00",
        "usd" => "USD $10.00",
        "eur" => "EUR €10.00",
        "date" => "Date 2024-01-15",
        "time" => "Time 13:30:00",
        "datetime" => "Date Time",
        "text" => "Text",
        _ => "Normal",
    }
}

/// Parse a single cell reference like `B3` to `(col, row)` (0-based), returning
/// None for anything that isn't `letters+digits` (so `exp2xy` can't panic).
fn parse_ref(s: &str) -> Option<(usize, usize)> {
    let s = s.trim();
    let mut seen_digit = false;
    let (mut has_letter, mut has_digit) = (false, false);
    for c in s.chars() {
        if c.is_ascii_alphabetic() {
            if seen_digit {
                return None; // letters must precede digits
            }
            has_letter = true;
        } else if c.is_ascii_digit() {
            seen_digit = true;
            has_digit = true;
        } else {
            return None;
        }
    }
    if has_letter && has_digit {
        Some(exp2xy(s))
    } else {
        None
    }
}

/// True if `s` is acceptable as a named-range name: it starts with a letter and
/// is otherwise letters / digits / underscore. (Strings that parse as a cell
/// reference are handled as references before this is reached.)
fn is_valid_name(s: &str) -> bool {
    let s = s.trim();
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    s.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
}

/// Markup for the formula bar: name box, cancel/confirm, fx, formula input.
fn formula_bar_html() -> String {
    "<input class=\"zs-name-box\" style=\"width:80px;height:100%;box-sizing:border-box;border:none;border-right:1px solid #e0e2e4;padding:0 6px;outline:none;font-size:13px;\" />\
     <span class=\"zs-fb-cancel\" style=\"width:24px;text-align:center;color:#999;cursor:pointer;\">✕</span>\
     <span class=\"zs-fb-confirm\" style=\"width:24px;text-align:center;color:#999;cursor:pointer;\">✓</span>\
     <span class=\"zs-fx\" style=\"width:34px;text-align:center;color:#999;font-style:italic;border-right:1px solid #e0e2e4;cursor:pointer;\">fx</span>\
     <input class=\"zs-formula-input\" style=\"flex:1;height:100%;box-sizing:border-box;border:none;padding:0 8px;outline:none;font-size:13px;\" />"
        .to_string()
}

/// Functions offered by the formula-bar fx picker.
fn fx_menu_html() -> String {
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
fn wire_formula_bar(
    fbar: web_sys::Element,
    renderer: &SharedRenderer,
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
                fi.set_value(&value);
                let _ = fi.focus();
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
        let mut el: Element = c.into();
        el.add_event_listener("click", move |_e: web_sys::Event| {
            commit();
        });
    }
    if let Some(c) = cancel {
        let sync = sync.clone();
        let mut el: Element = c.into();
        el.add_event_listener("click", move |_e: web_sys::Event| {
            sync();
        });
    }
}

fn init_editor_style(ta: &HtmlTextAreaElement) {
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
    let _ = style.set_property("z-index", "100");
}

/// Position the textarea over a cell, seed it with the cell's text, and focus.
fn start_edit(
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
        (r.cell_screen_rect(ri, ci), r.cell_text_at(ri, ci))
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
fn commit_edit(
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

/// Show a brief toast at the top of the page. Used for validation errors
/// from the formula bar and other commit paths that can't keep the editor
/// open (issue #9). Auto-hides after 2.5 seconds.
fn show_toast(toast: Option<&HtmlElement>, msg: &str) {
    if let Some(t) = toast {
        t.set_text_content(Some(msg));
        let _ = t.style().set_property("display", "block");
        let toast_for_hide = t.clone();
        let cb = Closure::<dyn FnMut()>::new(move || {
            let _ = toast_for_hide.style().set_property("display", "none");
        });
        if let Some(w) = web_sys::window() {
            let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                cb.as_ref().unchecked_ref(),
                2500,
            );
        }
        cb.forget();
    }
}

/// Open the list-validity popover (issue #9) anchored at `(x, y)` showing
/// the allowed values for the cell. The popover element is mutated in
/// place; its `data-cell` attribute records the (ri, ci) for the click
/// handler.
fn show_list_popover(
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

fn cancel_edit(textarea: &HtmlTextAreaElement, editor_error: Option<&HtmlElement>, editing: &EditingCell) {
    *editing.borrow_mut() = None;
    let _ = textarea.style().set_property("display", "none");
    let _ = textarea.style().set_property("border", "");
    if let Some(e) = editor_error {
        let _ = e.style().set_property("display", "none");
    }
}

/// Render the sheet-tab `<li>` items into the menu (active tab highlighted).
fn render_tabs(menu_el: &web_sys::Element, names: &[String], active: usize) {
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

fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Persist the active sheet back into `sheets`, then load `new_idx` into the
/// renderer and refresh the tab strip.
fn switch_sheet(
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
fn wire_bottombar(
    menu_el: Element,
    mut add_el: Element,
    renderer: &SharedRenderer,
    sheets: &Sheets,
    active: &ActiveSheet,
    sync: &SyncFn,
) {
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
        let mut menu_el_mut = menu_el;
        menu_el_mut.add_event_listener("click", move |event: web_sys::Event| {
            let Some(target) = event.target() else { return };
            let Ok(el) = target.dyn_into::<web_sys::Element>() else { return };
            let li = el
                .get_attribute("data-index")
                .map(|_| el.clone())
                .or_else(|| el.closest("[data-index]").ok().flatten());
            let Some(li) = li else { return };
            if let Some(idx) = li.get_attribute("data-index").and_then(|s| s.parse::<usize>().ok()) {
                switch_sheet(&renderer, &sheets, &active, &menu_for_handler, idx);
                sync();
            }
        });
    }

    // Double-click a tab: rename via prompt.
    {
        let renderer = renderer.clone();
        let sheets = sheets.clone();
        let active = active.clone();
        let menu_for = menu_node.clone();
        let mut menu_dbl: Element = menu_node.clone().into();
        menu_dbl.add_event_listener("dblclick", move |event: web_sys::Event| {
            let Some(idx) = tab_index_from_event(&event) else { return };
            let cur_name = sheets.borrow().get(idx).map(|d| d.name.clone()).unwrap_or_default();
            if let Ok(Some(name)) = window().prompt_with_message_and_default("Sheet name:", &cur_name) {
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
    {
        let renderer = renderer.clone();
        let sheets = sheets.clone();
        let active = active.clone();
        let menu_for = menu_node.clone();
        let sync = sync.clone();
        let mut menu_ctx: Element = menu_node.clone().into();
        menu_ctx.add_event_listener("contextmenu", move |event: web_sys::Event| {
            event.prevent_default();
            let Some(idx) = tab_index_from_event(&event) else { return };
            if sheets.borrow().len() <= 1 {
                return;
            }
            let nm = sheets.borrow()[idx].name.clone();
            if !matches!(window().confirm_with_message(&format!("Delete sheet \"{}\"?", nm)), Ok(true)) {
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

    // Add button: append a new sheet and switch to it.
    {
        let renderer = renderer.clone();
        let sheets = sheets.clone();
        let active = active.clone();
        let menu_for_add = menu_node.clone();
        let sync = sync.clone();
        add_el.add_event_listener("click", move |_event: web_sys::Event| {
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
fn find_panel_html() -> String {
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
fn wire_find(panel: web_sys::Element, renderer: &SharedRenderer, sync: &SyncFn) {
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
        let mut el: Element = find_input.clone().dyn_into::<web_sys::Element>().unwrap().into();
        el.add_event_listener("input", move |_e: web_sys::Event| refresh());
    }
    {
        let idx = idx.clone();
        let matches = matches.clone();
        let reveal_current = reveal_current.clone();
        let mut el: Element = find_input.clone().dyn_into::<web_sys::Element>().unwrap().into();
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
                    if m.is_empty() { None } else { Some(m[*idx.borrow()]) }
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
                let _ = panel_for_close.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
            });
        }
    }
    {
        let panel_for_esc = panel.clone();
        let mut el: Element = panel.clone().into();
        el.add_event_listener("keydown", move |e: web_sys::Event| {
            let ke: KeyboardEvent = e.dyn_into().unwrap();
            if ke.key() == "Escape" {
                let _ = panel_for_esc.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
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
                let _ = panel.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "block");
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

/// Extract the tab index (`data-index`) from a bottom-bar pointer event.
fn tab_index_from_event(event: &web_sys::Event) -> Option<usize> {
    let target = event.target()?;
    let el = target.dyn_into::<web_sys::Element>().ok()?;
    let li = el
        .get_attribute("data-index")
        .map(|_| el.clone())
        .or_else(|| el.closest("[data-index]").ok().flatten())?;
    li.get_attribute("data-index").and_then(|s| s.parse::<usize>().ok())
}

/// Markup for the right-click context menu.
fn context_menu_html() -> String {
    let item = |cmd: &str, label: &str| {
        format!("<div class=\"{p}-item\" data-cmenu=\"{cmd}\">{label}</div>", p = CSS_PREFIX, cmd = cmd, label = label)
    };
    let divider = format!("<div class=\"{p}-item divider\"></div>", p = CSS_PREFIX);
    [
        item("copy", "Copy"),
        item("cut", "Cut"),
        item("paste", "Paste"),
        divider.clone(),
        item("insert-row", "Insert row"),
        item("insert-col", "Insert column"),
        item("delete-row", "Delete row"),
        item("delete-col", "Delete column"),
        divider.clone(),
        item("note", "Insert / edit note"),
        item("delete-note", "Delete note"),
        divider.clone(),
        item("link", "Insert / edit link"),
        item("remove-link", "Remove link"),
        divider.clone(),
        item("clear", "Clear contents"),
        // Issue #9: data validation
        item("validation", "Data Validation…"),
        // Text alignment helpers (issue #25). The "set_rotation" /
        // "bump_indent" / "toggle_shrink_to_fit" actions are wired in
        // `wire_context_menu`.
        divider.clone(),
        item("rotate-0", "Rotate 0°"),
        item("rotate-45", "Rotate 45°"),
        item("rotate-90", "Rotate 90°"),
        item("rotate--45", "Rotate -45°"),
        item("shrink-toggle", "Shrink to fit"),
        item("indent-inc", "Increase indent"),
        item("indent-dec", "Decrease indent"),
    ]
    .join("")
}

/// Wire the right-click context menu: open on canvas contextmenu, run the
/// chosen command, and close on outside click.
fn wire_context_menu(
    canvas_el: &mut Element,
    menu_node: web_sys::Element,
    renderer: &SharedRenderer,
    dv_open: Rc<RefCell<Option<Rc<dyn Fn()>>>>,
) {
    // Open on right-click, after selecting the cell under the cursor.
    {
        let renderer = renderer.clone();
        let menu = menu_node.clone();
        canvas_el.add_event_listener("contextmenu", move |event: web_sys::Event| {
            event.prevent_default();
            let me: MouseEvent = event.dyn_into().unwrap();
            let (x, y) = (me.offset_x() as f64, me.offset_y() as f64);
            let hit = renderer.borrow().cell_at(x, y);
            if let Some((ri, ci)) = hit {
                let mut r = renderer.borrow_mut();
                // Issue #19: only collapse when the right-click is outside
                // every selected range (Excel behavior).
                if !r.contains_selected(ri, ci) {
                    r.clear_multi_range();
                    r.select_cell(ri, ci);
                    r.render();
                }
            }
            let style = menu.unchecked_ref::<web_sys::HtmlElement>().style();
            let _ = style.set_property("display", "block");
            let _ = style.set_property("left", &format!("{}px", x));
            let _ = style.set_property("top", &format!("{}px", y));
        });
    }

    // Run a command on item click, then hide.
    {
        let renderer = renderer.clone();
        let menu = menu_node.clone();
        let menu_for_click = menu_node.clone();
        let mut menu_el: Element = menu_node.clone().into();
        menu_el.add_event_listener("click", move |event: web_sys::Event| {
            let Some(target) = event.target() else { return };
            let Ok(el) = target.dyn_into::<web_sys::Element>() else { return };
            let cmd = el
                .get_attribute("data-cmenu")
                .or_else(|| el.closest("[data-cmenu]").ok().flatten().and_then(|e| e.get_attribute("data-cmenu")));
            let Some(cmd) = cmd else { return };

            // Editing a note needs a prompt outside the renderer borrow.
            if cmd == "note" {
                let current = renderer.borrow().selection_note().unwrap_or_default();
                if let Ok(Some(text)) =
                    window().prompt_with_message_and_default("Cell note:", &current)
                {
                    let mut r = renderer.borrow_mut();
                    r.set_selection_note(if text.trim().is_empty() { None } else { Some(text) });
                    r.render();
                }
                let _ = menu_for_click.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
                return;
            }

            // Editing a hyperlink also needs a prompt outside the renderer borrow.
            if cmd == "link" {
                let current = renderer.borrow().selection_link().unwrap_or_default();
                if let Ok(Some(text)) =
                    window().prompt_with_message_and_default("Link URL:", &current)
                {
                    let mut r = renderer.borrow_mut();
                    // set_selection_link normalizes the URL; blank input clears it.
                    r.set_selection_link(if text.trim().is_empty() { None } else { Some(text) });
                    r.render();
                }
                let _ = menu_for_click.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
                return;
            }

            // Data Validation modal (issue #9): open before the borrow_mut
            // match below so the open handle can take its own borrow.
            if cmd == "validation" {
                if let Some(open) = dv_open.borrow().as_ref() {
                    open();
                }
                let _ = menu_for_click.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
                return;
            }

            {
                let mut r = renderer.borrow_mut();
                // Read-only mode blocks every *write* menu action. Copy is
                // read-only on the data, so it stays available (issue #24).
                let read_only = r.data.is_read_only();
                match cmd.as_str() {
                    "copy" => r.copy_selection(),
                    "cut" if !read_only => r.cut_selection(),
                    "paste" if !read_only => r.paste(),
                    "insert-row" if !read_only => r.insert_row_at_selection(),
                    "insert-col" if !read_only => r.insert_col_at_selection(),
                    "delete-row" if !read_only => r.delete_rows_at_selection(),
                    "delete-col" if !read_only => r.delete_cols_at_selection(),
                    "delete-note" if !read_only => r.set_selection_note(None),
                    "remove-link" if !read_only => r.set_selection_link(None),
                    "clear" if !read_only => r.clear_selection_content(),
                    // Toggle the per-cell `editable` flag on the active cell.
                    // Works regardless of the sheet-wide read-only mode so
                    // a user can mark cells for later protection, but the
                    // toggle itself is a no-op in read-only mode.
                    "editable" if !read_only => {
                        let (sri, sci) = (r.selector.ri, r.selector.ci);
                        let was_editable = r.data.get_cell(sri, sci).map(|c| c.editable).unwrap_or(true);
                        r.data.set_cell_editable(sri, sci, !was_editable);
                    }
                    // Text alignment helpers (issue #25). Style changes are
                    // independent of the sheet's read-only mode — they're
                    // presentation, not data, so they apply even on a
                    // locked sheet. The `set_sheets_registry` clone we
                    // update is the renderer's, so the next render uses
                    // the new rotation/indent/shrink_to_fit immediately.
                    "rotate-0" if !read_only => r.set_rotation(0.0),
                    "rotate-45" if !read_only => r.set_rotation(45.0),
                    "rotate-90" if !read_only => r.set_rotation(90.0),
                    "rotate--45" if !read_only => r.set_rotation(-45.0),
                    "shrink-toggle" if !read_only => r.toggle_shrink_to_fit(),
                    "indent-inc" if !read_only => r.bump_indent(10),
                    "indent-dec" if !read_only => r.bump_indent(-10),
                    _ => {}
                }
                r.render();
            }
            let _ = menu_for_click.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
        });
        // Hide when clicking outside the menu.
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if let Some(target) = event.target() {
                if let Ok(node) = target.dyn_into::<web_sys::Node>() {
                    if menu.contains(Some(&node)) {
                        return;
                    }
                }
            }
            let _ = menu.unchecked_ref::<web_sys::HtmlElement>().style().set_property("display", "none");
        });
        window()
            .add_event_listener_with_callback("mousedown", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }
}

/// Data Validation modal markup (issue #9). Reuses the existing
/// `.x-spreadsheet-modal` CSS at `src/index.css:781-833` for the header
/// and content sections. The root div is positioned + hidden by default
/// (the opener sets `display:block`).
fn data_validation_modal_html() -> String {
    format!(
        r#"<div class="x-spreadsheet-modal zs-dv-root" style="display:none;position:fixed;left:50%;top:50%;transform:translate(-50%,-50%);z-index:1100;background:#fff;border-radius:4px;border:1px solid rgba(0,0,0,0.1);box-shadow:rgba(0,0,0,0.2) 0px 2px 8px;font-size:13px;line-height:1.25em;width:420px;">
            <div class="x-spreadsheet-modal-header" style="padding:8px 12px;border-bottom:1px solid #e6e6e6;font-weight:600;display:flex;align-items:center;justify-content:space-between;">
                <span>Data Validation</span>
                <span class="x-spreadsheet-icon zs-dv-close" style="cursor:pointer;color:#999;font-size:14px;">✕</span>
            </div>
            <div class="x-spreadsheet-modal-content" style="padding:12px;">
                <div style="display:flex;align-items:center;margin-bottom:8px;">
                    <label style="width:90px;">Allow</label>
                    <select class="zs-dv-type" style="flex:1;padding:3px;">
                        <option value="">Any value</option>
                        <option value="list">List</option>
                        <option value="number">Number</option>
                        <option value="text-length">Text length</option>
                        <option value="email">Email</option>
                        <option value="phone">Phone</option>
                    </select>
                </div>
                <div class="zs-dv-op-row" style="display:none;align-items:center;margin-bottom:8px;">
                    <label style="width:90px;">Operator</label>
                    <select class="zs-dv-op" style="flex:1;padding:3px;">
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
                    <label style="width:90px;" class="zs-dv-val1-label">Value</label>
                    <input class="zs-dv-val1 zs-dv-val" type="text" style="flex:1;padding:3px;box-sizing:border-box;" />
                    <span class="zs-dv-to" style="margin:0 6px;display:none;">to</span>
                    <input class="zs-dv-val2 zs-dv-val" type="text" style="flex:1;display:none;padding:3px;box-sizing:border-box;" />
                </div>
                <div class="zs-dv-list-row" style="display:none;margin-bottom:8px;">
                    <label style="display:block;margin-bottom:4px;">Source (comma-separated, e.g. Yes,No,Maybe)</label>
                    <textarea class="zs-dv-list" rows="3" style="width:100%;padding:4px;box-sizing:border-box;font-family:inherit;"></textarea>
                </div>
                <div style="display:flex;align-items:center;margin-bottom:8px;">
                    <label style="width:90px;">&nbsp;</label>
                    <label style="display:flex;align-items:center;">
                        <input type="checkbox" class="zs-dv-req" /> &nbsp;Treat empty as invalid
                    </label>
                </div>
                <div style="display:flex;align-items:center;margin-bottom:12px;">
                    <label style="width:90px;">Apply to</label>
                    <input class="zs-dv-ref" type="text" style="flex:1;padding:3px;box-sizing:border-box;" />
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
fn wire_data_validation_modal(
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
fn update_dv_rows(modal: &web_sys::Element, type_val: &str) {
    use wasm_bindgen::JsCast;
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

fn update_dv_rows_with_op(modal: &web_sys::Element, type_val: &str, op_val: &str) {
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
fn handle_dv_save(modal: &web_sys::Element, renderer: &SharedRenderer) {
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
fn open_dv_modal(
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

/// Swatches for the color palette dropdown.
fn color_palette_html() -> String {
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

/// Show the color palette beneath a toolbar button.
fn show_palette_under(palette: &web_sys::Element, button: &web_sys::Element) {
    let rect = button.get_bounding_client_rect();
    let style = palette.unchecked_ref::<web_sys::HtmlElement>().style();
    let _ = style.set_property("left", &format!("{}px", rect.left()));
    let _ = style.set_property("top", &format!("{}px", rect.bottom()));
    let _ = style.set_property("display", "block");
}

fn hide_palette(palette: &web_sys::Element) {
    let _ = palette
        .unchecked_ref::<web_sys::HtmlElement>()
        .style()
        .set_property("display", "none");
}

fn hide_tooltip(tooltip: &web_sys::Element) {
    let _ = tooltip
        .unchecked_ref::<web_sys::HtmlElement>()
        .style()
        .set_property("display", "none");
}

/// Show a styled tooltip below the hovered toolbar button (`data-tip`), and
/// hide it when the pointer leaves the toolbar.
fn wire_tooltip(toolbar: web_sys::Element, tooltip: web_sys::Element) {
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
fn wire_toolbar(
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

        // Dropdown buttons open their registered menu under the button.
        if let Some((_, menu)) = menus.iter().find(|(a, _)| *a == action) {
            show_palette_under(menu, &button);
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
            "freeze" => r.toggle_freeze(),
            "align-left" => r.set_align("left"),
            "align-center" => r.set_align("center"),
            "align-right" => r.set_align("right"),
            "align-top" => r.set_valign("top"),
            "align-middle" => r.set_valign("middle"),
            "align-bottom" => r.set_valign("bottom"),
            _ => return,
        }
        r.render();
        drop(r);
        sync();
    });
}

/// Wire the color palette: clicking a swatch applies the color per the current
/// mode (text vs fill); clicking elsewhere closes it.
fn wire_palette(palette: web_sys::Element, renderer: &SharedRenderer, palette_mode: &Rc<RefCell<String>>) {
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
fn dropdown_menu_html(items: &[(&str, &str)]) -> String {
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
fn border_menu_html() -> String {
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
fn wire_border_menu(menu: web_sys::Element, renderer: &SharedRenderer, sync: &SyncFn) {
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

/// Wire a toolbar dropdown: a row click applies the value, updates the button
/// title, and closes the menu; an outside click closes it.
fn wire_dropdown(menu: web_sys::Element, kind: DdKind, title_id: &'static str, renderer: &SharedRenderer) {
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

fn wire_events(
    canvas_el: &mut Element,
    renderer: &SharedRenderer,
    textarea: &HtmlTextAreaElement,
    editing: &EditingCell,
    editor_error_node: Option<HtmlElement>,
    list_popover_node: Option<web_sys::Element>,
    list_popover_visible: Rc<RefCell<bool>>,
    sync: &SyncFn,
) {
    let dragging = Rc::new(RefCell::new(false));
    let drag: Rc<RefCell<Option<DragState>>> = Rc::new(RefCell::new(None));

    // A floating popup that shows a cell's note on hover.
    let note_popup: web_sys::Element = {
        let el = document().create_element("div").unwrap();
        let _ = el.set_attribute(
            "style",
            "display:none;position:fixed;z-index:400;max-width:240px;background:#fffbe6;border:1px solid #d9c97a;box-shadow:1px 2px 6px rgba(0,0,0,0.2);padding:6px 8px;font-size:12px;white-space:pre-wrap;pointer-events:none;color:#333;",
        );
        document().body().unwrap().append_child(&el).unwrap();
        el
    };

    // mousedown: start a header-resize / scrollbar drag, or select a cell.
    {
        let renderer = renderer.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        let dragging = dragging.clone();
        let drag = drag.clone();
        let sync = sync.clone();
        let list_popover = list_popover_node.clone();
        let list_popover_visible = list_popover_visible.clone();
        canvas_el.add_event_listener("mousedown", move |event: web_sys::Event| {
            let me: MouseEvent = event.dyn_into().unwrap();
            let (x, y) = (me.offset_x() as f64, me.offset_y() as f64);

            // Header boundary → start a resize.
            let resize = renderer.borrow().resize_target(x, y);
            if let Some(kind) = resize {
                let start_size = match kind {
                    DragKind::ColResize(ci) => renderer.borrow().col_width_at(ci),
                    DragKind::RowResize(ri) => renderer.borrow().row_height_at(ri),
                    _ => 0f64,
                };
                *drag.borrow_mut() = Some(DragState { kind, start_x: x, start_y: y, start_size });
                return;
            }

            // Scrollbar track → start a scroll drag and jump immediately.
            let sb = renderer.borrow().scrollbar_target(x, y);
            if let Some(kind) = sb {
                *drag.borrow_mut() = Some(DragState { kind, start_x: x, start_y: y, start_size: 0f64 });
                apply_scroll_drag(&renderer, kind, x, y);
                return;
            }

            // Fill handle → start a fill drag from the current selection.
            if renderer.borrow().is_on_fill_handle(x, y) {
                renderer.borrow_mut().start_fill();
                *drag.borrow_mut() =
                    Some(DragState { kind: DragKind::Fill, start_x: x, start_y: y, start_size: 0f64 });
                return;
            }

            // List-validity glyph hit-test (issue #9): clicking the ▼ on a
            // list-valid cell opens the popover instead of starting a
            // selection. The glyph sits in the rightmost ~14px of the cell.
            // Compute hit-test details in a single borrow scope to avoid
            // overlapping immutable borrows on the renderer's RefCell.
            // NOTE: this is a `let = { ... }` block expression, so a bare
            // `return` here would exit the whole mousedown closure (and skip
            // cell selection below). Every non-glyph path must yield `None`.
            let glyph_hit: Option<(usize, usize, f64, f64)> = {
                let r = renderer.borrow();
                match r.cell_at(x, y) {
                    Some((ri, ci)) => {
                        let (origin_ri, origin_ci) = r.merge_origin(ri, ci);
                        if r.cell_has_list_validator(origin_ri, origin_ci) {
                            let rect = r.cell_screen_rect(origin_ri, origin_ci);
                            let in_glyph = x >= rect.x + rect.width - 17.0
                                && x <= rect.x + rect.width
                                && y >= rect.y
                                && y <= rect.y + rect.height;
                            if in_glyph {
                                Some((origin_ri, origin_ci, rect.x, rect.y))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    }
                    None => None,
                }
            };
            if let Some((origin_ri, origin_ci, _rx, _ry)) = glyph_hit {
                // Select the cell and open the popover. We defer the
                // `visible=true` write to after this event loop tick so the
                // global "outside click" mousedown listener (which sees the
                // same event we just handled) bails on `visible == false`
                // rather than closing the popover we just opened.
                {
                    let mut r = renderer.borrow_mut();
                    r.select_cell(origin_ri, origin_ci);
                    r.render();
                }
                let popover_for_open = list_popover.clone();
                let renderer_for_open = renderer.clone();
                let visible_for_open = list_popover_visible.clone();
                let cb = Closure::<dyn FnMut()>::new(move || {
                    show_list_popover(
                        popover_for_open.as_ref(),
                        &renderer_for_open,
                        origin_ri, origin_ci, x, y,
                        &visible_for_open,
                    );
                });
                if let Some(w) = web_sys::window() {
                    let _ = w.set_timeout_with_callback_and_timeout_and_arguments_0(
                        cb.as_ref().unchecked_ref(),
                        0,
                    );
                }
                cb.forget();
                return;
            }

            // Click-outside silently commits; on validation failure, the
            // editor stays open with the red border (issue #9). Either way,
            // we still fall through and process the click.
            let _ = commit_edit(&renderer, &textarea, editor_error.as_ref(), &editing);
            let hit = renderer.borrow().cell_at(x, y);
            if let Some((ri, ci)) = hit {
                // Ctrl/Cmd-click on a hyperlink cell follows the link.
                if me.ctrl_key() || me.meta_key() {
                    if let Some(url) = renderer.borrow().link_at(ri, ci) {
                        let _ = window().open_with_url_and_target(&url, "_blank");
                        return;
                    }
                }
                let mut r = renderer.borrow_mut();
                if me.ctrl_key() || me.meta_key() {
                    // Issue #19: Ctrl/Cmd-click adds a disjoint range. If the
                    // click landed inside an existing range, do nothing
                    // (matches Excel — toggling disjoint selection).
                    if !r.contains_selected(ri, ci) {
                        // First Ctrl+click: promote the current single-rect
                        // selection to a multi-range entry so the user's
                        // first picked cell stays selected.
                        if !r.multi_range_is_active() {
                            r.promote_selector_to_range();
                        }
                        let (sr, sc) = r.merge_origin(ri, ci);
                        r.add_range(sr, sc, sr, sc);
                    }
                } else {
                    // Plain click clears any Ctrl/Cmd-added ranges and
                    // starts a new single-rect selection.
                    r.clear_multi_range();
                    r.select_cell(ri, ci);
                }
                r.render();
                *dragging.borrow_mut() = true;
                drop(r);
                sync();
            }
        });
    }

    // mousemove: apply an active drag, extend a selection, or update the cursor.
    {
        let renderer = renderer.clone();
        let dragging = dragging.clone();
        let drag = drag.clone();
        let note_popup = note_popup.clone();
        canvas_el.add_event_listener("mousemove", move |event: web_sys::Event| {
            let me: MouseEvent = event.dyn_into().unwrap();
            let (x, y) = (me.offset_x() as f64, me.offset_y() as f64);

            // Active header-resize / scrollbar drag.
            if let Some(ds) = *drag.borrow() {
                hide_tooltip(&note_popup);
                let mut r = renderer.borrow_mut();
                match ds.kind {
                    DragKind::ColResize(ci) => {
                        r.set_col_width_clamped(ci, ds.start_size + (x - ds.start_x));
                        r.render();
                    }
                    DragKind::RowResize(ri) => {
                        r.set_row_height_clamped(ri, ds.start_size + (y - ds.start_y));
                        r.render();
                    }
                    DragKind::VScroll | DragKind::HScroll => {
                        drop(r);
                        apply_scroll_drag(&renderer, ds.kind, x, y);
                    }
                    DragKind::Fill => {
                        // Extend the selection toward the cursor as a fill preview.
                        if let Some((ri, ci)) = r.cell_at(x, y) {
                            r.select_to(ri, ci);
                            r.render();
                        }
                    }
                }
                return;
            }

            // Drag-select.
            if *dragging.borrow() {
                hide_tooltip(&note_popup);
                let hit = renderer.borrow().cell_at(x, y);
                if let Some((ri, ci)) = hit {
                    let mut r = renderer.borrow_mut();
                    if me.ctrl_key() || me.meta_key() {
                        // Issue #19: extend only the most-recently-added range
                        // when Ctrl/Cmd is held during drag.
                        r.select_to_last(ri, ci);
                    } else {
                        r.select_to(ri, ci);
                    }
                    r.render();
                }
                return;
            }

            // Hover feedback: resize cursor near header boundaries.
            {
                let r = renderer.borrow();
                match r.resize_target(x, y) {
                    Some(DragKind::ColResize(_)) => r.set_cursor("col-resize"),
                    Some(DragKind::RowResize(_)) => r.set_cursor("row-resize"),
                    _ => r.set_cursor("default"),
                }
            }

            // Note popup: show the hovered cell's note (if any).
            let note = renderer
                .borrow()
                .cell_at(x, y)
                .and_then(|(ri, ci)| renderer.borrow().note_at(ri, ci));
            match note {
                Some(text) => {
                    note_popup.set_text_content(Some(&text));
                    let style = note_popup.unchecked_ref::<web_sys::HtmlElement>().style();
                    let _ = style.set_property("left", &format!("{}px", me.client_x() + 12));
                    let _ = style.set_property("top", &format!("{}px", me.client_y() + 12));
                    let _ = style.set_property("display", "block");
                }
                None => hide_tooltip(&note_popup),
            }
        });
    }

    // dblclick: edit the clicked cell.
    {
        let renderer = renderer.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        canvas_el.add_event_listener("dblclick", move |event: web_sys::Event| {
            let me: MouseEvent = event.dyn_into().unwrap();
            let (x, y) = (me.offset_x() as f64, me.offset_y() as f64);
            let hit = renderer.borrow().cell_at(x, y);
            if let Some((ri, ci)) = hit {
                // Edit the merge origin when the cell is part of a merge.
                let (ri, ci) = renderer.borrow().merge_origin(ri, ci);
                start_edit(&renderer, &textarea, editor_error.as_ref(), &editing, ri, ci);
            }
        });
    }

    // wheel: scroll the body by whole cells.
    {
        let renderer = renderer.clone();
        canvas_el.add_event_listener("wheel", move |event: web_sys::Event| {
            let we: WheelEvent = event.clone().dyn_into().unwrap();
            we.prevent_default();
            let dy = we.delta_y();
            let dx = we.delta_x();
            let d_rows = if dy > 0.0 { 1 } else if dy < 0.0 { -1 } else { 0 };
            let d_cols = if dx > 0.0 { 1 } else if dx < 0.0 { -1 } else { 0 };
            if d_rows != 0 || d_cols != 0 {
                let mut r = renderer.borrow_mut();
                r.scroll_by(d_rows, d_cols);
                r.render();
            }
        });
    }

    // window keydown: arrow navigation + Enter-to-edit when not editing.
    {
        let renderer = renderer.clone();
        let textarea = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        let sync = sync.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            if editing.borrow().is_some() {
                return; // cell editor handles its own keys while editing
            }
            // Ignore grid keys while a formula-bar input (name box / formula
            // input) is focused — those inputs handle their own keystrokes.
            if let Some(active) = document().active_element() {
                if active.tag_name().eq_ignore_ascii_case("input") {
                    return;
                }
            }
            let ke: KeyboardEvent = event.dyn_into().unwrap();
            let key = ke.key();

            // Ctrl/Cmd shortcuts: clipboard + style toggles.
            if ke.ctrl_key() || ke.meta_key() {
                let mut handled = true;
                {
                    let mut r = renderer.borrow_mut();
                    match key.to_lowercase().as_str() {
                        "c" => r.copy_selection(),
                        "x" => r.cut_selection(),
                        "v" => r.paste(),
                        "b" => r.toggle_bold(),
                        "i" => r.toggle_italic(),
                        "u" => r.toggle_underline(),
                        // Ctrl/Cmd+Z undo; Ctrl/Cmd+Y or Ctrl/Cmd+Shift+Z redo.
                        "z" if ke.shift_key() => r.redo(),
                        "z" => r.undo(),
                        "y" => r.redo(),
                        _ => handled = false,
                    }
                    if handled {
                        r.render();
                    }
                }
                if handled {
                    ke.prevent_default();
                    sync();
                }
                return;
            }

            // Delete/Backspace clears the selected cells.
            if key == "Delete" || key == "Backspace" {
                {
                    let mut r = renderer.borrow_mut();
                    r.clear_selection_content();
                    r.render();
                }
                ke.prevent_default();
                sync();
                return;
            }

            let (mut dr, mut dc) = (0i32, 0i32);
            match key.as_str() {
                "ArrowUp" => dr = -1,
                "ArrowDown" => dr = 1,
                "ArrowLeft" => dc = -1,
                "ArrowRight" => dc = 1,
                "Enter" | "F2" => {
                    let (ri, ci) = {
                        let r = renderer.borrow();
                        let s = r.get_selector();
                        (s.ri, s.ci)
                    };
                    start_edit(&renderer, &textarea, editor_error.as_ref(), &editing, ri, ci);
                    ke.prevent_default();
                    return;
                }
                _ => return,
            }
            ke.prevent_default();
            {
                let mut r = renderer.borrow_mut();
                r.move_selection(dr, dc);
                r.render();
            }
            sync();
        });
        window()
            .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }

    // textarea keydown: commit/cancel while editing.
    {
        let renderer = renderer.clone();
        let textarea_inner = textarea.clone();
        let editing = editing.clone();
        let editor_error = editor_error_node.clone();
        let sync = sync.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |event: web_sys::Event| {
            let ke: KeyboardEvent = event.dyn_into().unwrap();
            match ke.key().as_str() {
                "Enter" => {
                    ke.prevent_default();
                    ke.stop_propagation();
                    // Issue #9: on validation failure, keep the editor open
                    // and skip the selection move so the user can fix the
                    // value in place.
                    if commit_edit(&renderer, &textarea_inner, editor_error.as_ref(), &editing).is_ok() {
                        let mut r = renderer.borrow_mut();
                        r.move_selection(1, 0);
                        r.render();
                    }
                    sync();
                }
                "Tab" => {
                    ke.prevent_default();
                    ke.stop_propagation();
                    if commit_edit(&renderer, &textarea_inner, editor_error.as_ref(), &editing).is_ok() {
                        let mut r = renderer.borrow_mut();
                        r.move_selection(0, 1);
                        r.render();
                    }
                    sync();
                }
                "Escape" => {
                    ke.prevent_default();
                    ke.stop_propagation();
                    cancel_edit(&textarea_inner, editor_error.as_ref(), &editing);
                }
                _ => {
                    ke.stop_propagation();
                }
            }
        });
        textarea
            .add_event_listener_with_callback("keydown", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }

    // window mouseup: end drag-select and any header/scrollbar/fill drag.
    {
        let dragging = dragging.clone();
        let drag = drag.clone();
        let renderer = renderer.clone();
        let sync = sync.clone();
        let cb = Closure::<dyn FnMut(web_sys::Event)>::new(move |_event: web_sys::Event| {
            // A fill-handle drag applies the fill on release.
            let was_fill = matches!(*drag.borrow(), Some(ds) if ds.kind == DragKind::Fill);
            if was_fill {
                {
                    let mut r = renderer.borrow_mut();
                    r.apply_fill();
                    r.render();
                }
                sync();
            }
            *dragging.borrow_mut() = false;
            *drag.borrow_mut() = None;
        });
        window()
            .add_event_listener_with_callback("mouseup", cb.as_ref().unchecked_ref())
            .unwrap();
        cb.forget();
    }
}

/// Map a scrollbar pointer position to a scroll fraction and apply it.
fn apply_scroll_drag(renderer: &SharedRenderer, kind: DragKind, x: f64, y: f64) {
    let mut r = renderer.borrow_mut();
    let (w, h, hw, ch) = {
        // width, height, row-header width, col-header height
        (r.width, r.height, r.row_header.width, r.col_header.height)
    };
    match kind {
        DragKind::VScroll => {
            let track = (h - ch).max(1f64);
            r.scroll_to_fraction_v((y - ch) / track);
        }
        DragKind::HScroll => {
            let track = (w - hw).max(1f64);
            r.scroll_to_fraction_h((x - hw) / track);
        }
        _ => {}
    }
    r.render();
}
