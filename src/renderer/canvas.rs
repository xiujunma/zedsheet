#![allow(dead_code)]

use wasm_bindgen::JsValue;
use web_sys::HtmlCanvasElement;
use web_sys::CanvasRenderingContext2d;
use wasm_bindgen::JsCast;
use web_sys::TextMetrics;
use web_sys::ImageData;
use web_sys::ImageBitmap;
use web_sys::DomMatrix;
use web_sys::CanvasWindingRule;
use js_sys::Float64Array;

#[derive(Debug, Clone, Copy)]
pub enum LineCap {
    Butt,
    Round,
    Square,
}

#[derive(Debug, Clone, Copy)]
pub enum LineJoin {
    Miter,
    Round,
    Bevel,
}

#[derive(Debug, Clone, Copy)]
pub enum TextAlign {
    Start,
    End,
    Left,
    Right,
    Center,
}

#[derive(Debug, Clone, Copy)]
pub enum TextBaseline {
    Top,
    Hanging,
    Middle,
    Alphabetic,
    Ideographic,
    Bottom,
}

#[derive(Debug, Clone, Copy)]
pub enum Direction {
    Ltr,
    Rtl,
    Inherit,
}
#[derive(Debug, Clone)]
pub struct LineProperties {
    line_width: u8,
    line_cap: LineCap,
    line_join: LineJoin,
}
#[derive(Debug, Clone)]
pub struct TextProperties {
    font: String,
    text_align: TextAlign,
    text_baseline: TextBaseline,
    direction: Direction,
}
#[derive(Debug, Clone)]
pub struct FileStrokeProperties {
    fill_style: String,
    stroke_style: String,
}
#[derive(Debug, Clone)]
pub struct ShadowProperties {
    shadow_blur: u8,
    shadow_color: String,
    shadow_offset_x: u8,
    shadow_offset_y: u8,
}
#[derive(Debug, Clone)]
pub struct CompositingProperties {
    global_alpha: f32,
    global_composite_operation: String,
}
#[derive(Debug, Clone)]
pub struct Canvas {
    target: HtmlCanvasElement,
    pub ctx: CanvasRenderingContext2d,
    scale: f64
}

impl Canvas {
    pub fn new(target: HtmlCanvasElement, scale: f64) -> Self {
        let ctx: CanvasRenderingContext2d = target
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

    pub fn set_size(&self, width: f64, height: f64) -> &Self {
        let style = self.target.style();
        style.set_property("width", &format!("{}px", width)).unwrap();
        style.set_property("height", &format!("{}px", height)).unwrap();
        let dpr = web_sys::window().unwrap().device_pixel_ratio();

        self.target.set_width((width as f64 * dpr).floor() as u32);
        self.target.set_height((height as f64 * dpr).floor() as u32);
        self.ctx.scale(dpr * self.scale, dpr * self.scale).unwrap();
        
        return self
    }

    pub fn measure_text_width(&self, text: &str) -> f64 {
        self.measure_text(text).width()
    }

    pub fn measure_text(&self, text: &str) -> TextMetrics {
        self.ctx.measure_text(text).unwrap()
    }

    pub fn line(&self, x1: f64, y1: f64, x2: f64, y2: f64) -> &Self {
        self.move_to(x1, y1)
            .line_to(x2, y2)
            .stroke();
        return self
    }

    pub fn clear_rect(&self, x: f64, y: f64, width: f64, height: f64) -> &Self {
        self.ctx.clear_rect(x, y, width, height);
        return self
    }

    pub fn fill_rect(&self, x: f64, y: f64, width: f64, height: f64) -> &Self {
        self.ctx.fill_rect(x, y, width, height);
        return self
    }

    pub fn stroke_rect(&self, x: f64, y: f64, width: f64, height: f64) -> &Self {
        self.ctx.stroke_rect(x, y, width, height);
        return self
    }

    pub fn fill_text(&self, text: &str, x: f64, y: f64, max_width: Option<f64>) -> &Self {
        if max_width.is_some() {
            self.ctx.fill_text_with_max_width(text, x, y, max_width.unwrap()).unwrap();
        } else {
            self.ctx.fill_text(text, x, y).unwrap();
        }
        return self
    }

    pub fn stroke_text(&self, text: &str, x: f64, y: f64, max_width: Option<f64>) -> &Self {
        if max_width.is_some() {
            self.ctx.stroke_text_with_max_width(text, x, y, max_width.unwrap()).unwrap();
        } else {
            self.ctx.stroke_text(text, x, y).unwrap();
        }
        return self
    }

    pub fn get_line_dash(&self) -> Vec<f64> {
        let segments = self.ctx.get_line_dash();
        return segments.to_vec().iter()
            .map(|x| x.as_f64().unwrap())
            .collect()
    }

    pub fn set_line_dash(&self, segments: &Vec<f64>) -> &Self {
        // FIXME
        let segments_js_value = Float64Array::new_with_length(segments.len() as u32);
        for (i, segment) in segments.iter().enumerate() {
            segments_js_value.set_index(i as u32, *segment);
        }
        self.ctx.set_line_dash(&segments_js_value).unwrap();
        return self
    }

    pub fn create_linear_gradient(&self, x0: f64, y0: f64, x1: f64, y1: f64) -> &Self {
        self.ctx.create_linear_gradient(x0, y0, x1, y1);
        return self
    }

    pub fn create_radial_gradient(&self, x0: f64, y0: f64, r0: f64, x1: f64, y1: f64, r1: f64) -> &Self {
        self.ctx.create_radial_gradient(x0, y0, r0, x1, y1, r1).unwrap();
        return self
    }

    pub fn create_pattern(&self, image: &ImageBitmap, repetition: &str) -> &Self {
        self.ctx.create_pattern_with_image_bitmap(image, repetition).unwrap();
        return self
    }

    pub fn bezier_curve_to(&self, cp1x: f64, cp1y: f64, cp2x: f64, cp2y: f64, x: f64, y: f64) -> &Self {
        self.ctx.bezier_curve_to(cp1x, cp1y, cp2x, cp2y, x, y);
        return self
    }

    pub fn quadratic_curve_to(&self, cpx: f64, cpy: f64, x: f64, y: f64) -> &Self {
        self.ctx.quadratic_curve_to(cpx, cpy, x, y);
        return self
    }

    pub fn arc(&self, x: f64, y: f64, radius: f64, start_angle: f64, end_angle: f64, counterclockwise: Option<bool>) -> &Self {
        if counterclockwise.is_some() {
            self.ctx.arc_with_anticlockwise(x, y, radius, start_angle, end_angle, counterclockwise.unwrap()).unwrap();
        } else {
            self.ctx.arc(x, y, radius, start_angle, end_angle).unwrap();
        }
        return self
    }

    pub fn arc_to(&self, x1: f64, y1: f64, x2: f64, y2: f64, radius: f64) -> &Self {
        self.ctx.arc_to(x1, y1, x2, y2, radius).unwrap();
        return self
    }

    pub fn begin_path(&self) -> &Self {
        self.ctx.begin_path();
        return self
    }

    pub fn close_path(&self) -> &Self {
        self.ctx.close_path();
        return self
    }

    pub fn move_to(&self, x: f64, y: f64) -> &Self {
        self.ctx.move_to(x, y);
        return self
    }

    pub fn line_to(&self, x: f64, y: f64) -> &Self {
        self.ctx.line_to(x, y);
        return self
    }

    pub fn ellipse(&self, x: f64, y: f64, radius_x: f64, radius_y: f64, rotation: f64, start_angle: f64, end_angle: f64, counterclockwise: Option<bool>) -> &Self {
        if counterclockwise.is_some() {
            self.ctx.ellipse_with_anticlockwise(x, y, radius_x, radius_y, rotation, start_angle, end_angle, counterclockwise.unwrap()).unwrap();
        } else {
            self.ctx.ellipse(x, y, radius_x, radius_y, rotation, start_angle, end_angle).unwrap();
        }

        return self
    }

    pub fn rect(&self, x: f64, y: f64, width: f64, height: f64) -> &Self {
        self.ctx.rect(x, y, width, height);
        return self
    }

    pub fn round_rect(&self, x: f64, y: f64, width: f64, height: f64, radius: f64) -> &Self {
        self.begin_path()
            .move_to(x + radius, y)
            .arc_to(x + width, y, x + width, y + height, radius)
            .arc_to(x + width, y + height, x, y + height, radius)
            .arc_to(x, y + height, x, y, radius)
            .arc_to(x, y, x + width, y, radius)
            .close_path();
        return self
    }

    pub fn fill(&self, rule: Option<CanvasWindingRule>) -> &Self {
        if rule.is_some() {
            self.ctx.fill_with_canvas_winding_rule(rule.unwrap());
        } else {
            self.ctx.fill();
        }
        return self
    }

    pub fn stroke(&self) -> &Self {
        self.ctx.stroke();
        return self
    }

    pub fn clip(&self, rule: Option<CanvasWindingRule>) -> &Self {
        if rule.is_some() {
            self.ctx.clip_with_canvas_winding_rule(rule.unwrap());
        } else {
            self.ctx.clip();
        }
        return self
    }

    pub fn is_point_in_path(&self, x: f64, y: f64, winding_rule: Option<CanvasWindingRule>) -> bool {
        if winding_rule.is_some() {
            return self.ctx.is_point_in_path_with_f64_and_canvas_winding_rule(x, y, winding_rule.unwrap())
        } else {
            return self.ctx.is_point_in_path_with_f64(x, y)
        }
    }

    pub fn is_point_in_stroke(&self, x: f64, y: f64) -> bool {
        return self.ctx.is_point_in_stroke_with_x_and_y(x, y);
    }

    pub fn get_transform(&self) -> DomMatrix {
        return self.ctx.get_transform().unwrap();
    }

    pub fn rotate(&self, angle: f64) -> &Self {
        self.ctx.rotate(angle).unwrap();
        return self
    }

    pub fn scale(&self, x: f64, y: f64) -> &Self {
        self.ctx.scale(x, y).unwrap();
        return self
    }

    pub fn translate(&self, x: f64, y: f64) -> &Self {
        self.ctx.translate(x, y).unwrap();
        return self
    }

    pub fn set_transform(&self, a: f64, b: f64, c: f64, d: f64, e: f64, f: f64) -> &Self {
        self.ctx.set_transform(a, b, c, d, e, f).unwrap();
        return self
    }

    pub fn draw_image(&self, image: &ImageBitmap, dx: f64, dy: f64) -> &Self {
        self.ctx.draw_image_with_image_bitmap(image, dx, dy).unwrap();
        return self
    }

    pub fn create_image_data(&self, width: f64, height: f64) -> ImageData {
        return self.ctx.create_image_data_with_sw_and_sh(width, height).unwrap();
    }

    pub fn get_image_data(&self, sx: f64, sy: f64, sw: f64, sh: f64) -> ImageData {
        return self.ctx.get_image_data(sx, sy, sw, sh).unwrap();
    }

    pub fn put_image_data(&self, image_data: ImageData, dx: f64, dy: f64) -> &Self {
        self.ctx.put_image_data(&image_data, dx, dy).unwrap();
        return self
    }

    pub fn save(&self) -> &Self {
        self.ctx.save();
        return self
    }

    pub fn restore(&self) -> &Self {
        self.ctx.restore();
        return self
    }

    // properties
    pub fn set_fill_style(&self, style: &str) -> &Self {
        self.ctx.set_fill_style(&JsValue::from_str(style));
        return self
    }
}