extern crate console_error_panic_hook;
use std::panic;

use renderer::table_renderer::TableRenderer;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;
use gloo::utils::document;

mod renderer;
mod component;
mod zedsheet;
mod config;
mod core;
mod data;
mod formula;

use data::table_data::TableData;
use core::cell::Cell;



#[wasm_bindgen(start)]
pub fn start() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

    let mut data = TableData::new(20, 10);

    // Add some sample data using core::cell::Cell
    data.set_cell(0, 0, Cell::with_text("Header 1"));
    data.set_cell(0, 1, Cell::with_text("Header 2"));
    data.set_cell(0, 2, Cell::with_text("Header 3"));
    data.set_cell(1, 0, Cell::with_text("A1"));
    data.set_cell(1, 1, Cell::with_text("100"));
    data.set_cell(2, 0, Cell::with_text("A2"));
    data.set_cell(2, 1, Cell::with_text("200"));
    // Formula cells use set_value to set the computed value
    let mut formula_cell = Cell::with_text("=SUM(B2:B3)");
    formula_cell.set_value("300"); // The computed value
    data.set_cell(3, 1, formula_cell);

    let container = document().get_element_by_id("zedsheet").unwrap().dyn_into::<HtmlCanvasElement>().unwrap();
    let mut table_renderer = TableRenderer::new(container, 800f64, 600f64, data);
    table_renderer.freeze("B2");
    // Set initial selector at cell B2
    table_renderer.set_selector(1, 1, 1, 1);
    table_renderer.render();

    // Setup mouse event handlers for cell selection
    setup_mouse_handlers();
}

#[wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console)]
    fn log(s: &str);
}

fn setup_mouse_handlers() {
    // This will be called from JS to handle clicks
    log("Setting up mouse handlers...");
}