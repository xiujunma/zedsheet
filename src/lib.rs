use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlCanvasElement;

use crate::renderer::table_renderer::TableRenderer;
use crate::renderer::viewport::Viewport;

mod renderer;

mod element;
mod utils;

#[wasm_bindgen(start)]
pub fn start() {
    let document = web_sys::window().unwrap().document().unwrap();
    let element = document.get_element_by_id("zedsheet").unwrap();
    let canvas: HtmlCanvasElement = element
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| ())
        .unwrap();

    let table_renderer: &'static mut TableRenderer =
        &mut TableRenderer::new(canvas, 200_f64, 200_f64);
    table_renderer.scale(1_f64);

    let viewport = Viewport::new(table_renderer);

    // let canvas = Canvas::new(canvas, 1_f64);
    // canvas.fill_rect(0_f64, 0_f64, 100_f64, 100_f64);
    warn!("Hello, world!");
}
