extern crate console_error_panic_hook;
use std::panic;

use gloo::console::console;
use renderer::table_renderer::TableRenderer;
use wasm_bindgen::prelude::*;
use web_sys::{console, HtmlCanvasElement};
use gloo::utils::document;

mod renderer;
mod component;
mod zedsheet;
mod config;
mod core;
mod data;

use data::table_data::TableData;


const TABLE_DATA: &'static TableData = &TableData {
    cols: vec![],
    rows: vec![],
    cells: vec![]
};

#[wasm_bindgen(start)]
pub fn start() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));
    let container = document().get_element_by_id("zedsheet").unwrap().dyn_into::<HtmlCanvasElement>().unwrap();
    let mut table_renderer = TableRenderer::new(container, 500f64, 500f64, TABLE_DATA);
    table_renderer.freeze("B2");
    table_renderer.render();
}