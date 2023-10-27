#![allow(dead_code)]

use crate::renderer::table_renderer::{Align, TextLineType, VerticalAlign};

pub fn text_x(align: Align, width: f64, padding: f64) -> f64 {
    return match align {
        Align::Left => {
            padding
        },
        Align::Center => {
            width / 2_f64
        },
        Align::Right => {
            width - padding
        }
    }
}

pub fn text_y(align: VerticalAlign, height: f64, text_height: f64, font_height: f64, padding: f64) -> f64 {
    return match align {
        VerticalAlign::Top => {
            padding
        },
        VerticalAlign::Middle => {
            let y = height / 2_f64 - font_height / 2_f64;
            let min_height = font_height / 2 + padding;
            if y < min_height {
                min_height
            } else {
                y
            }
        },
        VerticalAlign::Bottom => {
            height - padding - text_height
        }
    }
}

pub fn text_line(text_line_type: TextLineType, align: Align, vertical_align: VerticalAlign, x: f64, y: f64, w: f64, h: f64) -> (f64, f64, f64, f64) {
    let ty = 0f64;

}