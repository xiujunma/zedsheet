use std::collections::HashMap;
use serde::{Serialize, Deserialize};
use crate::core::cell::Cell;
use crate::core::row::Row;
use crate::core::col::Col;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TableData {
    pub rows: HashMap<usize, Row>,
    pub cols: HashMap<usize, Col>,
    pub row_count: usize,
    pub col_count: usize,
}

impl Default for TableData {
    fn default() -> Self {
        let mut rows = HashMap::new();
        let mut cols = HashMap::new();

        for i in 0..100 {
            rows.insert(i, Row::default());
        }
        for i in 0..26 {
            cols.insert(i, Col::default());
        }

        TableData {
            rows,
            cols,
            row_count: 100,
            col_count: 26,
        }
    }
}

impl TableData {
    pub fn new(row_count: usize, col_count: usize) -> Self {
        let mut rows = HashMap::new();
        let mut cols = HashMap::new();

        for i in 0..row_count {
            rows.insert(i, Row::default());
        }
        for i in 0..col_count {
            cols.insert(i, Col::default());
        }

        TableData { rows, cols, row_count, col_count }
    }

    pub fn set_cell(&mut self, row: usize, col: usize, cell: Cell) {
        let row_data = self.rows.entry(row).or_insert_with(Row::default);
        row_data.set_cell(col, cell);
    }

    pub fn get_cell(&self, row: usize, col: usize) -> Option<Cell> {
        self.rows.get(&row)?.get_cell(col).cloned()
    }

    pub fn get_row(&self, row: usize) -> Option<&Row> {
        self.rows.get(&row)
    }

    pub fn get_col(&self, col: usize) -> Option<&Col> {
        self.cols.get(&col)
    }

    pub fn get_row_height(&self, row: usize) -> f64 {
        self.rows.get(&row).map(|r| r.get_height()).unwrap_or(25.0)
    }

    pub fn get_col_width(&self, col: usize) -> f64 {
        self.cols.get(&col).map(|c| c.get_width()).unwrap_or(100.0)
    }

    pub fn set_row_height(&mut self, row: usize, height: f64) {
        let row_data = self.rows.entry(row).or_insert_with(Row::default);
        row_data.set_height(height);
    }

    pub fn set_col_width(&mut self, col: usize, width: f64) {
        let col_data = self.cols.entry(col).or_insert_with(Col::default);
        col_data.set_width(width);
    }
}