use super::cell::Cell;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Row {
    pub height: f64,
    pub hide: bool,
    pub auto_fit: bool,
    pub style: Option<usize>,
    pub cells: HashMap<usize, Cell>,
}

impl Default for Row {
    fn default() -> Self {
        Row {
            height: 25.0,
            hide: false,
            auto_fit: false,
            style: None,
            cells: HashMap::new(),
        }
    }
}

impl Row {
    pub fn new() -> Self {
        Row::default()
    }

    pub fn get_cell(&self, ci: usize) -> Option<&Cell> {
        self.cells.get(&ci)
    }

    pub fn get_cell_mut(&mut self, ci: usize) -> Option<&mut Cell> {
        self.cells.get_mut(&ci)
    }

    pub fn get_cell_or_new(&mut self, ci: usize) -> &mut Cell {
        self.cells.entry(ci).or_insert_with(Cell::default)
    }

    pub fn set_cell(&mut self, ci: usize, cell: Cell) {
        self.cells.insert(ci, cell);
    }

    pub fn delete_cell(&mut self, ci: usize) {
        self.cells.remove(&ci);
    }

    pub fn get_height(&self) -> f64 {
        if self.hide {
            0.0
        } else {
            self.height
        }
    }

    pub fn set_height(&mut self, height: f64) {
        self.height = height;
    }

    pub fn set_hide(&mut self, hide: bool) {
        self.hide = hide;
    }

    pub fn set_style(&mut self, style_idx: usize) {
        self.style = Some(style_idx);
    }

    pub fn cells(&self) -> &HashMap<usize, Cell> {
        &self.cells
    }

    pub fn iter_cells(&self) -> impl Iterator<Item = (&usize, &Cell)> {
        self.cells.iter()
    }
}
