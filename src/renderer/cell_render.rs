#![allow(dead_code)]

use crate::renderer::table_renderer::Align;

pub fn textx(align: Align, width: f64, padding: f64) -> f64 {
    match align {
        Align::Left => {
            return padding
        },
        Align::Center => {
            return width / 2_f64
        },
        Align::Right => {
            return width - padding
        }
    }
}