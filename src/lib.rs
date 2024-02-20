extern crate console_error_panic_hook;
use std::panic;

use gloo::console::console;
use renderer::table_renderer::{Col, Row, TableRenderer};
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



#[wasm_bindgen(start)]
pub fn start() {
    panic::set_hook(Box::new(console_error_panic_hook::hook));

    let data: TableData = TableData {
        cols: vec![Col::default(), Col::default(), Col::default(), Col::default(), Col::default()],
        rows: vec![Row::default(), Row::default(), Row::default(), Row::default(), Row::default()],
        cells: vec![]
    };
    

    let container = document().get_element_by_id("zedsheet").unwrap().dyn_into::<HtmlCanvasElement>().unwrap();
    let mut table_renderer = TableRenderer::new(container, 500f64, 500f64, data);
    table_renderer.freeze("B2");
    table_renderer.render();
}