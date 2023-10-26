#![allow(dead_code)]

use crate::renderer::table_renderer::Align;

pub fn textx(align: Align, width: f64, padding: f64) -> f64 {
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