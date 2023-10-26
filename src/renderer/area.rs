#![allow(dead_code)]
use super::table_renderer::GetRowHeightColWidth;
use super::range::Range;

pub struct Area {
    pub range: Range,
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
    pub get_size: Box<dyn GetRowHeightColWidth>,
}

impl Area {
    pub fn new(range: Range, x: usize, y: usize, width: usize, height: usize, get_size: Box<dyn GetRowHeightColWidth>) -> Self {
        return Area {
            range,
            x,
            y,
            width,
            height,
            get_size,
        }
    }

    pub fn get_row_height(&self, index: usize) -> f64 {
        return self.get_size.get_row_height(index)
    }

    pub fn get_col_width(&self, index: usize) -> f64 {
        return self.get_size.get_col_width(index)
    }

    pub fn contains_x(&self, x: usize) -> bool {
        return x >= self.x && x <= self.x + self.width
    }

    pub fn contains_y(&self, y: usize) -> bool {
        return y >= self.y && y <= self.y + self.height
    }
    pub fn contains(&self, x: usize, y: usize) -> bool {
        return self.contains_x(x) && self.contains_y(y)
    }

    pub fn each_row<F>(&self, mut f: F)
    where
        F: FnMut(usize, usize, f64),
    {

    }
}