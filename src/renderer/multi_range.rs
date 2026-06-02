//! Multi-range selection bookkeeping (issue #19).
//!
//! Excel-style non-contiguous selection: Ctrl/Cmd-click adds a disjoint range,
//! Ctrl/Cmd-drag extends the most-recently-added range. Style / border / clear /
//! paste operations then apply to **every** range.
//!
//! Kept as a standalone struct (no `TableRenderer`, no canvas) so the logic is
//! directly unit-testable from native `cargo test` without a `HtmlCanvasElement`.

use crate::core::cell_range::CellRange;

/// Bookkeeping for Ctrl/Cmd multi-range selection.
///
/// `ranges` is in insertion order. The last entry is the "active" range — its
/// anchor is the active cell that the formula bar / name box / toggle-reads
/// surface. When `ranges.is_empty()`, the renderer is in single-rect mode and
/// every consumer falls back to the renderer's `SelectorRect`.
#[derive(Debug, Clone, Default)]
pub struct MultiRangeState {
    pub ranges: Vec<CellRange>,
    /// Top-left of the most-recently-added range, normalized.
    pub anchor: (usize, usize),
}

impl MultiRangeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when there's at least one Ctrl/Cmd-added range.
    pub fn is_active(&self) -> bool {
        !self.ranges.is_empty()
    }

    /// Push a new range. Bounds are normalized (top-left/ bottom-right) and
    /// `anchor` is rebound to the new top-left.
    pub fn add(&mut self, r0: usize, c0: usize, r1: usize, c1: usize) {
        let (r0, c0, r1, c1) = normalize(r0, c0, r1, c1);
        self.ranges.push(CellRange::new(r0, c0, r1, c1));
        self.anchor = (r0, c0);
    }

    /// Replace the most-recently-added range with a new one whose anchor is the
    /// existing `anchor` and whose bottom-right is `(ri, ci)`. No-op when there
    /// are no Ctrl/Cmd ranges yet.
    pub fn extend_last(&mut self, ri: usize, ci: usize) {
        if self.ranges.is_empty() {
            return;
        }
        let (ar, ac) = self.anchor;
        let (r0, c0, r1, c1) = normalize(ar, ac, ri, ci);
        self.ranges.pop();
        self.ranges.push(CellRange::new(r0, c0, r1, c1));
    }

    /// Empty the multi-range. `anchor` is left at its current value (caller
    /// may reset it to the active single-rect's top-left).
    pub fn clear(&mut self) {
        self.ranges.clear();
    }

    /// All ranges as normalized `(r0, c0, r1, c1)` tuples. When empty, the
    /// caller should fall back to the single-rect `SelectorRect`.
    pub fn normalized(&self) -> Vec<(usize, usize, usize, usize)> {
        self.ranges
            .iter()
            .map(|r| (r.sri, r.sci, r.eri, r.eci))
            .collect()
    }

    /// Bounding box of all ranges, or `None` when empty.
    pub fn union(&self) -> Option<(usize, usize, usize, usize)> {
        if self.ranges.is_empty() {
            return None;
        }
        let mut r0 = usize::MAX;
        let mut c0 = usize::MAX;
        let mut r1 = 0usize;
        let mut c1 = 0usize;
        for r in &self.ranges {
            if r.sri < r0 { r0 = r.sri; }
            if r.sci < c0 { c0 = r.sci; }
            if r.eri > r1 { r1 = r.eri; }
            if r.eci > c1 { c1 = r.eci; }
        }
        Some((r0, c0, r1, c1))
    }

    /// True if `(ri, ci)` is inside any range.
    pub fn contains(&self, ri: usize, ci: usize) -> bool {
        self.ranges.iter().any(|r| r.includes(ri, ci))
    }

    /// Walk every cell of every range, calling `f` for each `(ri, ci)`.
    pub fn for_each_cell<F: FnMut(usize, usize)>(&self, mut f: F) {
        for r in &self.ranges {
            for ri in r.sri..=r.eri {
                for ci in r.sci..=r.eci {
                    f(ri, ci);
                }
            }
        }
    }
}

/// Normalize bounds so that `(r0, c0)` is the top-left and `(r1, c1)` is the
/// bottom-right. Both points are inclusive.
fn normalize(r0: usize, c0: usize, r1: usize, c1: usize) -> (usize, usize, usize, usize) {
    if r0 <= r1 && c0 <= c1 {
        (r0, c0, r1, c1)
    } else if r0 <= r1 && c0 > c1 {
        (r0, c1, r1, c0)
    } else if r0 > r1 && c0 <= c1 {
        (r1, c0, r0, c1)
    } else {
        (r1, c1, r0, c0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_by_default() {
        let s = MultiRangeState::new();
        assert!(!s.is_active());
        assert!(s.normalized().is_empty());
        assert_eq!(s.union(), None);
        assert!(!s.contains(0, 0));
    }

    #[test]
    fn add_appends_a_single_cell_range() {
        let mut s = MultiRangeState::new();
        s.add(2, 3, 2, 3);
        assert!(s.is_active());
        assert_eq!(s.normalized(), vec![(2, 3, 2, 3)]);
        assert_eq!(s.anchor, (2, 3));
    }

    #[test]
    fn add_two_ranges_preserves_order_and_rebinds_anchor() {
        let mut s = MultiRangeState::new();
        s.add(0, 0, 0, 0);
        s.add(5, 5, 5, 5);
        assert_eq!(s.normalized(), vec![(0, 0, 0, 0), (5, 5, 5, 5)]);
        assert_eq!(s.anchor, (5, 5));
    }

    #[test]
    fn add_normalizes_reversed_bounds() {
        let mut s = MultiRangeState::new();
        s.add(5, 5, 2, 3);
        assert_eq!(s.normalized(), vec![(2, 3, 5, 5)]);
        assert_eq!(s.anchor, (2, 3));
    }

    #[test]
    fn extend_last_grows_only_the_last_range() {
        let mut s = MultiRangeState::new();
        s.add(0, 0, 1, 1);
        s.add(5, 5, 5, 5);
        s.extend_last(7, 7);
        assert_eq!(s.normalized(), vec![(0, 0, 1, 1), (5, 5, 7, 7)]);
        assert_eq!(s.anchor, (5, 5));
    }

    #[test]
    fn extend_last_is_a_noop_when_empty() {
        let mut s = MultiRangeState::new();
        s.extend_last(7, 7);
        assert!(!s.is_active());
    }

    #[test]
    fn extend_last_grows_in_reverse_direction() {
        // Dragging up/left from the anchor must still grow the range (issue
        // #19): the fix routes the raw cursor to extend_last, which normalizes.
        let mut s = MultiRangeState::new();
        s.add(5, 5, 5, 5); // anchor at (5, 5)
        s.extend_last(3, 3); // drag toward the top-left
        assert_eq!(s.normalized().last(), Some(&(3, 3, 5, 5)));
    }

    #[test]
    fn clear_empties_ranges_but_keeps_anchor() {
        let mut s = MultiRangeState::new();
        s.add(0, 0, 1, 1);
        s.add(5, 5, 5, 5);
        s.clear();
        assert!(!s.is_active());
        assert_eq!(s.anchor, (5, 5)); // anchor preserved; caller can reset
    }

    #[test]
    fn union_returns_bounding_box() {
        let mut s = MultiRangeState::new();
        s.add(0, 0, 2, 2);
        s.add(5, 5, 7, 7);
        assert_eq!(s.union(), Some((0, 0, 7, 7)));
    }

    #[test]
    fn contains_finds_cells_in_any_range() {
        let mut s = MultiRangeState::new();
        s.add(0, 0, 1, 1);
        s.add(5, 5, 6, 6);
        assert!(s.contains(0, 0));
        assert!(s.contains(1, 1));
        assert!(s.contains(5, 5));
        assert!(s.contains(6, 6));
        assert!(!s.contains(2, 2));
        assert!(!s.contains(3, 3));
    }

    #[test]
    fn for_each_cell_visits_every_cell_once() {
        let mut s = MultiRangeState::new();
        s.add(0, 0, 1, 1); // 4 cells
        s.add(5, 5, 6, 6); // 4 cells
        let mut cells = Vec::new();
        s.for_each_cell(|ri, ci| cells.push((ri, ci)));
        assert_eq!(cells.len(), 8);
        let unique: std::collections::HashSet<_> = cells.iter().collect();
        assert_eq!(unique.len(), 8, "no duplicates expected");
        assert!(cells.contains(&(0, 0)));
        assert!(cells.contains(&(1, 1)));
        assert!(cells.contains(&(5, 5)));
        assert!(cells.contains(&(6, 6)));
    }

    // --- Fan-out behavior tests (issue #19) ---
    //
    // These exercise the iteration pattern that the renderer's mutators
    // (`update_selection_style`, `clear_format`, `clear_selection_content`,
    // `paste`) all use: collect cells, then operate. We hit a real `DataProxy`
    // so the assertions catch a real regression, not just iteration order.

    use crate::core::data_proxy::DataProxy;

    fn two_ranges() -> MultiRangeState {
        let mut s = MultiRangeState::new();
        s.add(0, 0, 1, 1); // A1:B2
        s.add(5, 5, 6, 6); // F6:G7
        s
    }

    #[test]
    fn fan_out_writes_every_selected_cell() {
        let s = two_ranges();
        let mut d = DataProxy::new("t");
        // Simulate `update_selection_style(|st| st.bold = true)`: collect cells
        // then apply a write to each.
        let cells: Vec<(usize, usize)> = {
            let mut v = Vec::new();
            s.for_each_cell(|ri, ci| v.push((ri, ci)));
            v
        };
        for (ri, ci) in cells {
            let mut st = d.get_cell_style(ri, ci);
            st.bold = true;
            let idx = d.add_style(st);
            d.set_cell_style(ri, ci, idx);
        }
        // All 8 cells should now be bold.
        for ri in 0..=1 {
            for ci in 0..=1 {
                assert!(d.get_cell_style(ri, ci).bold, "({ri},{ci}) should be bold");
            }
        }
        for ri in 5..=6 {
            for ci in 5..=6 {
                assert!(d.get_cell_style(ri, ci).bold, "({ri},{ci}) should be bold");
            }
        }
        // And the gap (2,2)..(4,4) untouched.
        for ri in 2..=4 {
            for ci in 2..=4 {
                assert!(!d.get_cell_style(ri, ci).bold, "({ri},{ci}) must be untouched");
            }
        }
    }

    #[test]
    fn fan_out_clears_content_everywhere() {
        let s = two_ranges();
        let mut d = DataProxy::new("t");
        // Seed every cell of both ranges with text.
        s.for_each_cell(|ri, ci| d.set_cell_text(ri, ci, "x"));
        // Confirm seeded.
        assert_eq!(d.get_cell_text(0, 0), "x");
        assert_eq!(d.get_cell_text(6, 6), "x");
        // Simulate `clear_selection_content`: collect, then `delete_cell` each.
        let cells: Vec<(usize, usize)> = {
            let mut v = Vec::new();
            s.for_each_cell(|ri, ci| v.push((ri, ci)));
            v
        };
        for (ri, ci) in cells {
            d.delete_cell(ri, ci);
        }
        for ri in 0..=1 {
            for ci in 0..=1 {
                assert_eq!(d.get_cell_text(ri, ci), "", "({ri},{ci}) should be empty");
            }
        }
        for ri in 5..=6 {
            for ci in 5..=6 {
                assert_eq!(d.get_cell_text(ri, ci), "", "({ri},{ci}) should be empty");
            }
        }
    }

    #[test]
    fn fan_out_paste_lands_at_every_top_left() {
        let s = two_ranges();
        let mut d = DataProxy::new("t");
        // The clipboard payload is a 1×1 with "X". In multi-range paste, it
        // lands at the top-left of every range. We simulate that by collecting
        // every range's top-left and writing "X" to each.
        let destinations: Vec<(usize, usize)> = s
            .normalized()
            .into_iter()
            .map(|(r0, c0, _, _)| (r0, c0))
            .collect();
        assert_eq!(destinations, vec![(0, 0), (5, 5)]);
        for (r, c) in destinations {
            d.set_cell_text(r, c, "X");
        }
        assert_eq!(d.get_cell_text(0, 0), "X");
        assert_eq!(d.get_cell_text(5, 5), "X");
    }

    #[test]
    fn union_bbox_covers_all_ranges() {
        // `set_borders("outer")` uses the union bounding box to decide which
        // cells are on the outer edge. With two disjoint ranges, the union is
        // the rect that encloses both.
        let mut s = MultiRangeState::new();
        s.add(0, 0, 1, 1);
        s.add(4, 4, 5, 5);
        assert_eq!(s.union(), Some((0, 0, 5, 5)));
    }

    #[test]
    fn extend_last_preserves_earlier_ranges() {
        // Ctrl+drag extends only the most-recent range; earlier ones stay put.
        let mut s = MultiRangeState::new();
        s.add(0, 0, 1, 1);
        s.add(3, 3, 3, 3);
        let before = s.normalized();
        s.extend_last(7, 7);
        let after = s.normalized();
        assert_eq!(before[0], after[0], "first range must not move");
        assert_eq!(after[1], (3, 3, 7, 7), "last range grew from (3,3,3,3) to (3,3,7,7)");
    }

    #[test]
    fn promote_selector_adds_range_and_sets_anchor() {
        // Mirrors `TableRenderer::promote_selector_to_range` so the test
        // exercises the same pattern: take the current single-rect, push
        // it as the first multi-range entry, and set the anchor.
        let mut s = MultiRangeState::new();
        let selector = (0, 0, 0, 0);
        s.add(selector.0, selector.1, selector.2, selector.3);
        assert_eq!(s.normalized(), vec![(0, 0, 0, 0)]);
        assert_eq!(s.anchor, (0, 0));
    }
}
