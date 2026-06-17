use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Col {
    pub width: f64,
    pub hide: bool,
    pub auto_fit: bool,
    pub style: Option<usize>,
}

impl Default for Col {
    fn default() -> Self {
        Col {
            width: 100.0,
            hide: false,
            auto_fit: false,
            style: None,
        }
    }
}

impl Col {
    pub fn new() -> Self {
        Col::default()
    }

    pub fn get_width(&self) -> f64 {
        if self.hide {
            0.0
        } else {
            self.width
        }
    }

    pub fn set_width(&mut self, width: f64) {
        self.width = width;
    }

    pub fn set_hide(&mut self, hide: bool) {
        self.hide = hide;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Cols {
    pub len: usize,
    pub default_width: f64,
    pub index_width: f64,
    pub data: HashMap<usize, Col>,
}

impl Default for Cols {
    fn default() -> Self {
        Cols {
            len: 26,
            default_width: 100.0,
            index_width: 60.0,
            data: HashMap::new(),
        }
    }
}

impl Cols {
    pub fn new(len: usize, width: f64) -> Self {
        Cols {
            len,
            default_width: width,
            index_width: 60.0,
            data: HashMap::new(),
        }
    }

    pub fn get(&self, ci: usize) -> Option<&Col> {
        self.data.get(&ci)
    }

    pub fn get_or_new(&mut self, ci: usize) -> &mut Col {
        self.data.entry(ci).or_insert_with(Col::default)
    }

    pub fn get_width(&self, ci: usize) -> f64 {
        self.get(ci)
            .map(|c| c.get_width())
            .unwrap_or(self.default_width)
    }

    pub fn set_width(&mut self, ci: usize, width: f64) {
        let col = self.get_or_new(ci);
        col.width = width;
    }

    pub fn set_hide(&mut self, ci: usize, hide: bool) {
        self.get_or_new(ci).set_hide(hide);
    }

    pub fn set_style(&mut self, ci: usize, style_idx: usize) {
        self.get_or_new(ci).style = Some(style_idx);
    }

    pub fn sum_width(&self, start: usize, end: usize) -> f64 {
        (start..end).map(|i| self.get_width(i)).sum()
    }

    pub fn total_width(&self) -> f64 {
        self.sum_width(0, self.len)
    }
}
