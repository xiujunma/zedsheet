use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlCanvasElement, MouseEvent};

#[wasm_bindgen]
pub struct EventManager {
    canvas: HtmlCanvasElement,
}

#[wasm_bindgen]
impl EventManager {
    #[wasm_bindgen(constructor)]
    pub fn new(canvas: HtmlCanvasElement) -> Self {
        EventManager { canvas }
    }

    pub fn attach_click_handler(&self, callback: &JsValue) {
        let canvas = self.canvas.clone();
        let cb = callback.clone();

        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            let rect = get_bounding_rect(&canvas);
            let x = event.client_x() as f64 - rect.0;
            let y = event.client_y() as f64 - rect.1;

            if let Some(func) = cb.dyn_ref::<js_sys::Function>() {
                let this = JsValue::NULL;
                let _ = func.call2(&this, &JsValue::from_f64(x), &JsValue::from_f64(y));
            }
        }) as Box<dyn Fn(MouseEvent)>);

        self.canvas
            .add_event_listener_with_callback("click", closure.as_ref().unchecked_ref())
            .unwrap();
        closure.forget();
    }

    pub fn attach_double_click_handler(&self, callback: &JsValue) {
        let canvas = self.canvas.clone();
        let cb = callback.clone();

        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            let rect = get_bounding_rect(&canvas);
            let x = event.client_x() as f64 - rect.0;
            let y = event.client_y() as f64 - rect.1;

            if let Some(func) = cb.dyn_ref::<js_sys::Function>() {
                let this = JsValue::NULL;
                let _ = func.call2(&this, &JsValue::from_f64(x), &JsValue::from_f64(y));
            }
        }) as Box<dyn Fn(MouseEvent)>);

        self.canvas
            .add_event_listener_with_callback("dblclick", closure.as_ref().unchecked_ref())
            .unwrap();
        closure.forget();
    }

    pub fn attach_mouse_move_handler(&self, callback: &JsValue) {
        let canvas = self.canvas.clone();
        let cb = callback.clone();

        let closure = Closure::wrap(Box::new(move |event: MouseEvent| {
            let rect = get_bounding_rect(&canvas);
            let x = event.client_x() as f64 - rect.0;
            let y = event.client_y() as f64 - rect.1;

            if let Some(func) = cb.dyn_ref::<js_sys::Function>() {
                let this = JsValue::NULL;
                let _ = func.call2(&this, &JsValue::from_f64(x), &JsValue::from_f64(y));
            }
        }) as Box<dyn Fn(MouseEvent)>);

        self.canvas
            .add_event_listener_with_callback("mousemove", closure.as_ref().unchecked_ref())
            .unwrap();
        closure.forget();
    }

    pub fn get_cell_at(x: f64, y: f64) -> JsValue {
        // Calculate cell position based on row height 20 and col width 100
        let col = (x / 100.0) as usize;
        let row = (y / 20.0) as usize;

        let arr = js_sys::Array::new();
        arr.push(&JsValue::from(row as u32));
        arr.push(&JsValue::from(col as u32));
        JsValue::from(arr)
    }
}

fn get_bounding_rect(canvas: &HtmlCanvasElement) -> (f64, f64, f64, f64) {
    // Fallback values - use element attributes
    let left = 0.0;
    let top = 0.0;
    let mut width = 800.0;
    let mut height = 600.0;

    // Try to get style for width/height
    if let Some(style) = canvas.style().dyn_ref::<web_sys::CssStyleDeclaration>() {
        if let Ok(w) = style.get_property_value("width") {
            if let Some(w_val) = w.strip_suffix("px") {
                width = w_val.parse().unwrap_or(800.0);
            }
        }
        if let Ok(h) = style.get_property_value("height") {
            if let Some(h_val) = h.strip_suffix("px") {
                height = h_val.parse().unwrap_or(600.0);
            }
        }
    }

    (left, top, width, height)
}
