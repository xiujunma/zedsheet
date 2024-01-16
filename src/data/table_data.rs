
use crate::renderer::table_renderer::{Row, Col, Cell};
pub trait Table {
    fn get_row(&self, row: usize) -> Option<Row>;
    fn get_col(&self, col: usize) -> Option<Col>;
    fn get_cell(&self, row: usize, col: usize) -> Option<Cell>;
}

pub struct TableData {
    pub rows: Vec<Row>,
    pub cols: Vec<Col>,
    pub cells: Vec<Cell>,
}

impl Table for TableData {
    fn get_row(&self, row: usize) -> Option<Row> {
        self.rows.get(row).map(|it| it.clone())
    }

    fn get_col(&self, col: usize) -> Option<Col> {
        self.cols.get(col).map(|it| it.clone())
    }

    fn get_cell(&self, row: usize, col: usize) -> Option<Cell> {
        self.cells.get(row * self.cols.len() + col).map(|it| it.clone())
    }
}