//! Excel-style Tables (ListObjects) — issue #34.
//!
//! A `Table` is a named rectangular region with a header row, banded data
//! rows, and an optional totals row. The cells themselves stay ordinary grid
//! cells; the table is a layer over them that drives render-time styling
//! (header / banding — see `DataProxy::apply_table_style`), the header
//! autofilter, auto-expansion on adjacent edits, and structured references
//! in formulas (`=SUM(Sales[Amount])`, `[@Amount]` — resolved in
//! `DataProxy::resolve_struct_ref`).

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Table {
    /// Unique (workbook-wide, case-insensitive) table name, e.g. `Table1`.
    pub name: String,
    /// Full extent INCLUDING the header row and, when `totals_row` is set,
    /// the totals row.
    pub sri: usize,
    pub sci: usize,
    pub eri: usize,
    pub eci: usize,
    /// When `true`, the last row of the range is the totals row.
    #[serde(default)]
    pub totals_row: bool,
    /// Banded (zebra-striped) data rows; on by default like Excel.
    #[serde(default = "default_true")]
    pub banded: bool,
}

fn default_true() -> bool {
    true
}

impl Table {
    pub fn new(name: &str, sri: usize, sci: usize, eri: usize, eci: usize) -> Self {
        Table {
            name: name.to_string(),
            sri,
            sci,
            eri,
            eci,
            totals_row: false,
            banded: true,
        }
    }

    pub fn contains(&self, ri: usize, ci: usize) -> bool {
        ri >= self.sri && ri <= self.eri && ci >= self.sci && ci <= self.eci
    }

    /// The header row index (always the table's first row).
    pub fn header_row(&self) -> usize {
        self.sri
    }

    /// The totals row index, when enabled.
    pub fn totals_row_index(&self) -> Option<usize> {
        self.totals_row.then_some(self.eri)
    }

    /// First and last data-body row. The body sits between the header and
    /// the totals row; a header-only table yields `None`.
    pub fn data_rows(&self) -> Option<(usize, usize)> {
        let last = if self.totals_row {
            self.eri.checked_sub(1)?
        } else {
            self.eri
        };
        (last > self.sri).then_some((self.sri + 1, last))
    }

    /// The last row typing below which should auto-expand the table (the
    /// data body's end — a totals row doesn't move the growth edge).
    pub fn growth_row(&self) -> usize {
        if self.totals_row {
            self.eri.saturating_sub(1)
        } else {
            self.eri
        }
    }
}

/// Shift table extents for `n` rows/cols inserted at `at` (mirrors how merges
/// and outline groups shift). An insert inside a table grows it.
pub fn shift_tables_for_insert(tables: &mut [Table], is_row: bool, at: usize, n: usize) {
    for t in tables.iter_mut() {
        let (start, end) = if is_row {
            (&mut t.sri, &mut t.eri)
        } else {
            (&mut t.sci, &mut t.eci)
        };
        if at <= *start {
            *start += n;
            *end += n;
        } else if at <= *end {
            *end += n;
        }
    }
}

/// Shift table extents for one deleted row/col at `at`. A table whose data
/// disappears entirely is removed.
pub fn shift_tables_for_delete(tables: &mut Vec<Table>, is_row: bool, at: usize) {
    tables.retain_mut(|t| {
        let (start, end) = if is_row {
            (&mut t.sri, &mut t.eri)
        } else {
            (&mut t.sci, &mut t.eci)
        };
        if at < *start {
            *start -= 1;
            *end -= 1;
        } else if at <= *end {
            if *end == *start {
                return false; // last row/col of the table deleted
            }
            *end -= 1;
            // Deleting the totals row turns the flag off.
            if is_row && t.totals_row && at == *end + 1 {
                t.totals_row = false;
            }
        }
        // A table reduced to its header row alone is dropped too: with no
        // data body left, structured references would all be #REF!.
        if is_row && t.eri == t.sri {
            return false;
        }
        true
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn data_rows_account_for_header_and_totals() {
        let mut t = Table::new("T", 1, 1, 4, 3);
        assert_eq!(t.data_rows(), Some((2, 4)));
        t.totals_row = true;
        assert_eq!(t.data_rows(), Some((2, 3)));
        assert_eq!(t.totals_row_index(), Some(4));
        assert_eq!(t.growth_row(), 3);
    }

    #[test]
    fn insert_before_inside_and_after() {
        let mut ts = vec![Table::new("T", 2, 0, 5, 2)];
        shift_tables_for_insert(&mut ts, true, 0, 2); // before: whole table moves
        assert_eq!((ts[0].sri, ts[0].eri), (4, 7));
        shift_tables_for_insert(&mut ts, true, 5, 1); // inside: grows
        assert_eq!((ts[0].sri, ts[0].eri), (4, 8));
        shift_tables_for_insert(&mut ts, true, 20, 1); // after: untouched
        assert_eq!((ts[0].sri, ts[0].eri), (4, 8));
    }

    #[test]
    fn delete_shrinks_and_removes() {
        let mut ts = vec![Table::new("T", 2, 0, 4, 2)];
        shift_tables_for_delete(&mut ts, true, 0); // before
        assert_eq!((ts[0].sri, ts[0].eri), (1, 3));
        shift_tables_for_delete(&mut ts, true, 2); // inside: shrinks
        assert_eq!((ts[0].sri, ts[0].eri), (1, 2));
        shift_tables_for_delete(&mut ts, true, 2); // only data row gone → dropped
        assert!(ts.is_empty());
    }
}
