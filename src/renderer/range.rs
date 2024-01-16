#![allow(dead_code)]

use super::alphabets::xy2expr;

#[derive(Clone, PartialEq, Debug, Copy)]
pub struct Range {
    pub start_row: usize,
    pub start_col: usize,
    pub end_row: usize,
    pub end_col: usize,
}

pub enum Position {
    Left,
    Right,
    Up,
    Down,
    None,
}

impl Range {
    fn get_start(&self) -> (usize, usize) {
        return (self.start_row, self.start_col)
    }

    fn get_end(&self) -> (usize, usize) {
        return (self.end_row, self.end_col)
    }

    fn get_rows(&self) -> usize {
        return self.end_row - self.start_row
    }

    fn get_cols(&self) -> usize {
        return self.end_col - self.start_col
    }

    fn get_multiple(&self) -> bool {
        return self.get_rows() > 0 || self.get_cols() > 0
    }

    fn contains_row(&self, index: usize) -> bool {
        return index >= self.start_row && index <= self.end_row
    }

    fn contains_col(&self, index: usize) -> bool {
        return index >= self.start_col && index <= self.end_col
    }

    pub fn contains(&self, row: usize, col: usize) -> bool {
        return self.contains_row(row) && self.contains_col(col)
    }

    fn within(&self, other: &Range) -> bool {
        return self.contains(other.start_row, other.start_col) && self.contains(other.end_row, other.end_col)
    }

    fn position(&self, other: &Range) -> Position {
        if self.start_row <= other.start_row && self.end_row >= other.end_row {
            if other.start_col > self.end_col {
                return Position::Right
            } else if other.end_col < self.start_col {
                return Position::Left
            }
        } else if self.start_col <= other.start_col && self.end_col >= other.end_col {
            if other.start_row > self.end_row {
                return Position::Down
            } else if other.end_row < self.start_row {
                return Position::Up
            }
        }
        return Position::None
    }

    fn intersects_row(&self, start_row: usize, end_row: usize) -> bool {
        return self.start_row <= end_row && self.end_row >= start_row
    }

    fn intersects_col(&self, start_col: usize, end_col: usize) -> bool {
        return self.start_col <= end_col && self.end_col >= start_col
    }

    pub fn intersects(&self, other: &Range) -> bool {
        return self.intersects_row(other.start_row, other.end_row) && self.intersects_col(other.start_col, other.end_col)
    }

    fn intersection(&self, other: &Range) -> Range {
        let start_row = if self.start_row > other.start_row { self.start_row } else { other.start_row };
        let start_col = if self.start_col > other.start_col { self.start_col } else { other.start_col };
        let end_row = if self.end_row < other.end_row { self.end_row } else { other.end_row };
        let end_col = if self.end_col < other.end_col { self.end_col } else { other.end_col };
        return Range {
            start_row,
            start_col,
            end_row,
            end_col,
        }
    }

    fn union(&self, other: &Range) -> Range {
        let start_row = if self.start_row < other.start_row { self.start_row } else { other.start_row };
        let start_col = if self.start_col < other.start_col { self.start_col } else { other.start_col };
        let end_row = if self.end_row > other.end_row { self.end_row } else { other.end_row };
        let end_col = if self.end_col > other.end_col { self.end_col } else { other.end_col };
        return Range {
            start_row,
            start_col,
            end_row,
            end_col,
        }
    }

    fn difference(&self, other: &Range) -> Vec<Range> {
        let mut ranges:Vec<Range> = Vec::new();
        if !self.intersects(other) {
            return ranges;
        }
        let n_other = self.intersection(other);
        ranges.push(Range {
            start_row: self.start_row,
            start_col: self.start_col,
            end_row: n_other.start_row - 1,
            end_col: self.end_col,
        });
        ranges.push(Range {
            start_row: n_other.end_row + 1,
            start_col: self.start_col,
            end_row: self.end_row,
            end_col: self.end_col,
        });
        ranges.push(Range {
            start_row: n_other.start_row,
            start_col: self.start_col,
            end_row: n_other.end_row,
            end_col: n_other.start_col - 1,
        });
        ranges.push(Range {
            start_row: n_other.start_row,
            start_col: n_other.end_col + 1,
            end_row: n_other.end_row,
            end_col: self.end_col,
        });
        // return ranges.iter().filter(|r| r.get_rows() >= 0 && r.get_cols() >= 0).map(|r| r.clone()).collect();
        return ranges;
    }
    fn touches(&self, other: &Range) -> bool {
        return
            (
                other.start_row == self.start_row && other.end_row == self.end_row && (
                    other.start_col == self.end_col + 1 || other.end_col == self.start_col - 1
                )
            ) || (
                other.start_col == self.start_col && other.end_col == self.end_col && (
                    other.start_row == self.end_row + 1 || other.end_row == self.start_row - 1
                )
            )
    }

    pub fn each_row(&self, cb: impl Fn(usize), max: Option<usize>) -> &Self {
        let mut end_row = self.end_row;
        if max.is_some() && max.unwrap() < end_row {
            end_row = max.unwrap();
        }

        for row in self.start_row..end_row {
            cb(row);
        }

        return self
    }

    pub fn each_col(&self, cb: impl Fn(usize), max: Option<usize>) -> &Self {
        let mut end_col = self.end_col;
        if max.is_some() && max.unwrap() < end_col {
            end_col = max.unwrap();
        }

        for col in self.start_col..end_col {
            cb(col);
        }

        return self
    }

    fn each(&self, cb: impl Fn(usize, usize)) -> &Self {
        self.each_row(|row| {
            self.each_col(|col| {
                cb(row, col);
            }, None);
        }, None);
        return self
    }
    fn to_string(&self) -> String {
        let mut expr = xy2expr(self.start_col, self.start_row);
        if self.get_multiple() {
            expr = format!("{}:{}", expr, xy2expr(self.end_col, self.end_row));
        }
        return expr
    }

    fn equals(&self, other: &Range) -> bool {
        return self.start_row == other.start_row 
            && self.start_col == other.start_col 
            && self.end_row == other.end_row 
            && self.end_col == other.end_col
    }

    pub fn new(row: usize, col: usize, row1: usize, col1: usize) -> Self {
        let mut start_row = row;
        let mut start_col = col;
        let mut end_row = row1;
        let mut end_col = col1;

        if row > row1 {
            start_row = row1;
            end_row = row;
        }

        if col > col1 {
            start_col = col1;
            end_col = col;
        }
        return Range {
            start_row,
            start_col,
            end_row,
            end_col,
        }
    }
}