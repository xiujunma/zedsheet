//! Row/column outline groups (issue #30): adjacent rows or columns grouped
//! into collapsible ranges, with Excel-style nesting. Pure data + level math —
//! `DataProxy` owns the group lists and applies collapse state through the
//! existing row/col hide flags (#14); the renderer draws the gutter.

use serde::{Deserialize, Serialize};

/// One outline group over rows `start..=end` (or columns, in the col list).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutlineGroup {
    pub start: usize,
    pub end: usize,
    #[serde(default)]
    pub collapsed: bool,
}

impl OutlineGroup {
    pub fn contains(&self, i: usize) -> bool {
        i >= self.start && i <= self.end
    }

    /// Covers `other` and is strictly larger (defines outline nesting).
    fn properly_contains(&self, other: &OutlineGroup) -> bool {
        self.start <= other.start
            && self.end >= other.end
            && (self.end - self.start) > (other.end - other.start)
    }
}

/// Nesting level per group: 1 + the number of groups properly containing it,
/// matching Excel's outline levels (outermost = 1).
pub fn group_levels(groups: &[OutlineGroup]) -> Vec<usize> {
    groups
        .iter()
        .map(|g| 1 + groups.iter().filter(|o| o.properly_contains(g)).count())
        .collect()
}

/// The deepest nesting level present, or 0 with no groups.
pub fn max_level(groups: &[OutlineGroup]) -> usize {
    group_levels(groups).into_iter().max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn g(start: usize, end: usize) -> OutlineGroup {
        OutlineGroup {
            start,
            end,
            collapsed: false,
        }
    }

    #[test]
    fn levels_count_proper_containment() {
        // 1..=8 contains 2..=4 and 6..=7; 2..=4 contains 3..=3.
        let groups = [g(1, 8), g(2, 4), g(6, 7), g(3, 3)];
        assert_eq!(group_levels(&groups), vec![1, 2, 2, 3]);
        assert_eq!(max_level(&groups), 3);
        // An identical range does not nest (no strict containment).
        let twins = [g(1, 4), g(1, 4)];
        assert_eq!(group_levels(&twins), vec![1, 1]);
        assert_eq!(max_level(&[]), 0);
    }
}
