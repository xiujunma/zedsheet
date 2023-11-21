use wasm_bindgen::JsValue;
use crate::renderer::area::Area;
use crate::renderer::canvas::Canvas;
use crate::renderer::cell_render::cell_border_render;
use crate::renderer::range::Range;
use crate::renderer::table_renderer::{BorderLine, BorderLineStyle, BorderType, Gridline, Rect, TableRenderer};
use crate::renderer::table_renderer::BorderType::{All, Inside, Vertical};

pub fn render_lines(canvas: &Canvas, gridline: &Gridline, cb: fn()) {
    if gridline.width > 0f64 {
        canvas
            .save()
            .begin_path();

        canvas.ctx.set_line_width(gridline.width - 0.5);
        canvas.ctx.set_stroke_style(&JsValue::from_str(&gridline.color));

        cb();
        canvas.restore();
    }
}

pub fn render_cell_grid_line(canvas: &Canvas, gridline: &Gridline, rect: &Rect) {
    render_lines(canvas, gridline, || {
        canvas
            .translate(rect.x, rect.y)
            .line(rect.width, 0f64, rect.width, rect.height)
            .line(0f64, rect.height, rect.width, rect.height);

    });
}

pub fn render_border(canvas: &Canvas, area: &Area, range: &Range, border_rect: &Rect, border_type: BorderType, line_style: BorderLineStyle, color: &str, auto_align: Option<bool>) {
    if border_type == BorderType::Outside || border_type == BorderType::All {
        let border_line = BorderLine {
            left: Some((line_style, color.to_string())),
            top: Some((line_style, color.to_string())),
            right: Some((line_style, color.to_string())),
            bottom: Some((line_style, color.to_string())),
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    } else if border_type == BorderType::Left {
        let border_line = BorderLine {
            left: Some((line_style, color.to_string())),
            top: None,
            right: None,
            bottom: None,
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    } else if border_type == BorderType::Top {
        let border_line = BorderLine {
            left: None,
            top: Some((line_style, color.to_string())),
            right: None,
            bottom: None,
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    } else if border_type == BorderType::Right {
        let border_line = BorderLine {
            left: None,
            top: None,
            right: Some((line_style, color.to_string())),
            bottom: None,
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    } else if border_type == BorderType::Bottom {
        let border_line = BorderLine {
            left: None,
            top: None,
            right: None,
            bottom: Some((line_style, color.to_string())),
        };
        cell_border_render(canvas, border_rect, &border_line, auto_align);
    }

    if border_type == All || border_type == Inside || border_type == BorderType::Horizontal || border_type == Vertical {
        // TODO
    }
}

pub fn render(renderer: &TableRenderer) {

}