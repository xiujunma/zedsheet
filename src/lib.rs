mod renderer;

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;

use renderer::canvas::Canvas;
use web_sys::HtmlCanvasElement;

#[wasm_bindgen(start)]
pub fn start() {
    let document = web_sys::window().unwrap().document().unwrap();
    let canvas = document.get_element_by_id("zedsheet").unwrap();
    let canvas: HtmlCanvasElement = canvas
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| ())
        .unwrap();
    
    let canvas = Canvas::new(canvas, 1 as f64);
    canvas.draw();
}
