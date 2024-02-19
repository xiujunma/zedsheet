use component::options::Options;
use renderer::table_renderer::TableRenderer;
use wasm_bindgen::prelude::*;
use web_sys::HtmlCanvasElement;
use zedsheet::ZedSheet;
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
    // ZedSheet::new("#zedsheet", Options::default());
    let container = document().query_selector("#zedsheet").unwrap().unwrap().dyn_into::<HtmlCanvasElement>().unwrap();
    let mut table_renderer = TableRenderer::new(container, 500f64, 500f64, TABLE_DATA);
    table_renderer.render();
}