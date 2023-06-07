#![allow(dead_code)]

use web_sys::HtmlCanvasElement;
use web_sys::CanvasRenderingContext2d;
use wasm_bindgen::JsCast;

pub enum LineCap {
    Butt,
    Round,
    Square,
}

pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

pub enum TextAlign {
    Start,
    End,
    Left,
    Right,
    Center,
}

pub enum TextBaseline {
    Top,
    Hanging,
    Middle,
    Alphabetic,
    Ideographic,
    Bottom,
}

pub enum Direction {
    Ltr,
    Rtl,
    Inherit,
}

pub struct LineProperties {
    line_width: u8,
    line_cap: LineCap,
    line_join: LineJoin,
}

pub struct TextProperties {
    font: String,
    text_align: TextAlign,
    text_baseline: TextBaseline,
    direction: Direction,
}

pub struct FileStrokeProperties {
    fill_style: String,
    stroke_style: String,
}

pub struct ShadowProperties {
    shadow_blur: u8,
    shadow_color: String,
    shadow_offset_x: u8,
    shadow_offset_y: u8,
}

pub struct CompositingProperties {
    global_alpha: f32,
    global_composite_operation: String,
}

pub struct Canvas {
    target: HtmlCanvasElement,
    ctx: CanvasRenderingContext2d,
    scale: f64
}

impl Canvas {
    pub fn new(target: HtmlCanvasElement, scale: f64) -> Self {
        let ctx = target
            .get_context("2d")
            .unwrap()
            .unwrap()
            .dyn_into::<CanvasRenderingContext2d>()
            .unwrap();
        Self {
            target,
            ctx,
            scale
        }
    }

    pub fn set_size(&self, width: u32, height: u32) -> &Self {
        let style = self.target.style();
        style.set_property("width", &format!("{}px", width)).unwrap();
        style.set_property("height", &format!("{}px", height)).unwrap();
        let dpr = web_sys::window().unwrap().device_pixel_ratio();

        self.target.set_width((width as f64 * dpr).floor() as u32);
        self.target.set_height((height as f64 * dpr).floor() as u32);
        self.ctx.scale(dpr * self.scale, dpr * self.scale).unwrap();
        
        return self
    }

    pub fn prop() {
        // TODO
    }

    pub fn draw(&self) {
        println!("draw");
    }
}