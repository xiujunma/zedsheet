//! Types shared by the clipboard serializers and parsers.

/// One cell parsed from clipboard content, placed in a dense row-major grid.
///
/// `row_span`/`col_span` are `1` for an ordinary cell. The top-left anchor of a
/// merged region carries the full span; the positions it covers are emitted as
/// empty `1×1` placeholders so the grid stays rectangular.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedCell {
    pub text: String,
    pub row_span: usize,
    pub col_span: usize,
}

impl ParsedCell {
    /// A plain unmerged cell holding `text`.
    pub fn text(text: impl Into<String>) -> Self {
        ParsedCell {
            text: text.into(),
            row_span: 1,
            col_span: 1,
        }
    }

    /// An empty unmerged placeholder.
    pub fn empty() -> Self {
        ParsedCell::text(String::new())
    }

    /// Whether this cell spans more than one grid position.
    pub fn is_merged(&self) -> bool {
        self.row_span > 1 || self.col_span > 1
    }
}

/// A rectangular block of cells parsed from the clipboard, row-major.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ParsedGrid {
    pub cells: Vec<Vec<ParsedCell>>,
}

impl ParsedGrid {
    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|row| row.is_empty())
    }

    pub fn rows(&self) -> usize {
        self.cells.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsed_cell_constructors() {
        assert_eq!(ParsedCell::text("a"), ParsedCell { text: "a".into(), row_span: 1, col_span: 1 });
        assert_eq!(ParsedCell::empty().text, "");
        assert!(!ParsedCell::text("a").is_merged());
        assert!(ParsedCell { text: "a".into(), row_span: 2, col_span: 1 }.is_merged());
    }

    #[test]
    fn empty_grid_detection() {
        assert!(ParsedGrid::default().is_empty());
        assert!(ParsedGrid { cells: vec![vec![]] }.is_empty());
        assert!(!ParsedGrid { cells: vec![vec![ParsedCell::text("x")]] }.is_empty());
    }
}
