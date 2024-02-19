
use crate::renderer::table_renderer::{Row, Col, Cell};
pub trait Table: Sized {
    fn get_row(&self, row: usize) -> Option<Row>;
    fn get_col(&self, col: usize) -> Option<Col>;
    fn get_cell(&self, row: usize, col: usize) -> Option<Cell>;
}

#[derive(Clone, Debug)]
pub struct TableData {
    pub rows: Vec<Row>,
    pub cols: Vec<Col>,
    pub cells: Vec<Cell>,
}

impl TableData {
    pub fn get_row(&self, row: usize) -> Option<Row> {
        self.rows.get(row).map(|it| it.clone())
    }

    pub fn get_col(&self, col: usize) -> Option<Col> {
        self.cols.get(col).map(|it| it.clone())
    }

    pub fn get_cell(&self, row: usize, col: usize) -> Option<Cell> {
        self.cells.get(row * self.cols.len() + col).map(|it| it.clone())
    }
}