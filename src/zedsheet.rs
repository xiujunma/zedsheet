use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;
use gloo::utils::document;

use crate::config::CSS_PREFIX;
use crate::component::element::{h, Element};
use crate::component::options::Options;
use crate::component::toolbar::Toolbar;
use crate::core::data_proxy::DataProxy;
use crate::renderer::table_renderer::TableRenderer;

/// Top-level spreadsheet. Builds the DOM shell (toolbar + sheet canvas +
/// bottom bar) inside the target element and drives the canvas renderer.
pub struct ZedSheet {
    pub renderer: TableRenderer,
}

impl ZedSheet {
    pub fn new(selector: &str, options: Options, data: DataProxy) -> Self {
        let target = document()
            .query_selector(selector)
            .expect("query_selector failed")
            .expect("target element not found");

        let mut root = h("div", Some(CSS_PREFIX));

        // Toolbar (top).
        if options.show_toolbar {
            let toolbar = Toolbar::new();
            root.append_child(&mut toolbar.element().clone());
        }

        // Sheet area: a positioned wrapper holding the canvas.
        let mut sheet_el = h("div", Some(&format!("{}-sheet", CSS_PREFIX)));
        let mut canvas_el = h("canvas", Some(&format!("{}-table", CSS_PREFIX)));
        sheet_el.append_child(&mut canvas_el);
        root.append_child(&mut sheet_el);

        // Bottom bar (placeholder for sheet tabs; wired in a later phase).
        if options.show_bottom_bar {
            let mut bottom = h("div", Some(&format!("{}-bottombar", CSS_PREFIX)));
            root.append_child(&mut bottom);
        }

        let mut target_el: Element = target.into();
        target_el.append_child(&mut root);

        // Determine the drawable size from the target's client box, falling
        // back to sensible defaults so the grid is always visible.
        let toolbar_h = if options.show_toolbar { 41f64 } else { 0f64 };
        let bottom_h = if options.show_bottom_bar { 41f64 } else { 0f64 };
        let (cw, ch) = client_box(&target_el);
        let width = if cw > 0f64 { cw } else { 900f64 };
        let height = (if ch > 0f64 { ch } else { 600f64 } - toolbar_h - bottom_h).max(200f64);

        let canvas = canvas_el
            .el
            .clone()
            .unwrap()
            .dyn_into::<HtmlCanvasElement>()
            .expect("canvas element");

        let mut renderer = TableRenderer::new(canvas, width, height, data);
        renderer.set_selector(0, 0, 0, 0);
        renderer.render();

        Self { renderer }
    }
}

fn client_box(el: &Element) -> (f64, f64) {
    el.el
        .as_ref()
        .and_then(|e| e.dyn_ref::<web_sys::HtmlElement>().map(|h| {
            (h.client_width() as f64, h.client_height() as f64)
        }))
        .unwrap_or((0f64, 0f64))
}
