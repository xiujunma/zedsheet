//! Conditional formatting rules (issue #11): cell-value comparisons, text
//! containment, and 2-color scales. Pure data + matching logic — `DataProxy`
//! owns the rule list and the renderer asks it for per-cell style overrides.

use serde::{Deserialize, Serialize};
use crate::core::cell_range::CellRange;
use crate::core::data_proxy::cmp_cell_values;

/// One conditional-formatting rule. For comparison ops, `v1` (and `v2` for
/// `between`) hold the comparison operands and `bgcolor`/`color`/`bold` the
/// style applied on a match. For `scale2`, `v1`/`v2` hold the min/max hex
/// colors and the fill is interpolated across the range's numeric values.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CondRule {
    /// Target range expression, e.g. `"B2:B10"`.
    pub range: String,
    /// `"gt" | "ge" | "lt" | "le" | "eq" | "between" | "contains" | "scale2"`.
    pub op: String,
    #[serde(default)]
    pub v1: String,
    #[serde(default)]
    pub v2: String,
    #[serde(default)]
    pub bgcolor: Option<String>,
    #[serde(default)]
    pub color: Option<String>,
    #[serde(default)]
    pub bold: bool,
}

impl CondRule {
    /// The rule's target bounds as `(sri, sci, eri, eci)`, if the range
    /// expression is valid.
    pub fn bounds(&self) -> Option<(usize, usize, usize, usize)> {
        CellRange::from_str(&self.range)
            .ok()
            .map(|r| (r.sri, r.sci, r.eri, r.eci))
    }

    /// Whether a (raw, unformatted) cell value satisfies a comparison rule.
    /// Numbers compare numerically, text case-insensitively — the same
    /// ordering the sort/filter machinery uses. Not meaningful for `scale2`.
    pub fn matches_value(&self, v: &str) -> bool {
        use std::cmp::Ordering::*;
        // Blank cells never match (cmp_cell_values sorts blanks last, which
        // would otherwise make them "greater than" everything).
        if v.trim().is_empty() {
            return false;
        }
        match self.op.as_str() {
            "contains" => v.to_lowercase().contains(&self.v1.to_lowercase()),
            "between" => {
                let lo = cmp_cell_values(v, &self.v1, true);
                let hi = cmp_cell_values(v, &self.v2, true);
                matches!(lo, Equal | Greater) && matches!(hi, Equal | Less)
            }
            op => {
                let ord = cmp_cell_values(v, &self.v1, true);
                match op {
                    "gt" => ord == Greater,
                    "ge" => matches!(ord, Greater | Equal),
                    "lt" => ord == Less,
                    "le" => matches!(ord, Less | Equal),
                    "eq" => ord == Equal,
                    _ => false,
                }
            }
        }
    }
}

/// Linearly interpolate between two `#rrggbb` colors. `t` is clamped to 0..=1.
pub fn lerp_hex(a: &str, b: &str, t: f64) -> Option<String> {
    let pa = parse_hex(a)?;
    let pb = parse_hex(b)?;
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 { (x as f64 + (y as f64 - x as f64) * t).round() as u8 };
    Some(format!(
        "#{:02x}{:02x}{:02x}",
        mix(pa.0, pb.0),
        mix(pa.1, pb.1),
        mix(pa.2, pb.2)
    ))
}

fn parse_hex(s: &str) -> Option<(u8, u8, u8)> {
    let h = s.trim().strip_prefix('#')?;
    if h.len() != 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&h[0..2], 16).ok()?,
        u8::from_str_radix(&h[2..4], 16).ok()?,
        u8::from_str_radix(&h[4..6], 16).ok()?,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(op: &str, v1: &str, v2: &str) -> CondRule {
        CondRule {
            range: "A1:B10".into(),
            op: op.into(),
            v1: v1.into(),
            v2: v2.into(),
            bgcolor: Some("#ffc7ce".into()),
            color: None,
            bold: false,
        }
    }

    #[test]
    fn comparison_ops_are_numeric_aware() {
        assert!(rule("gt", "150", "").matches_value("200"));
        assert!(!rule("gt", "150", "").matches_value("100"));
        assert!(rule("gt", "9", "").matches_value("10")); // numeric, not lexicographic
        assert!(rule("le", "150", "").matches_value("150"));
        assert!(rule("eq", "Apple", "").matches_value("apple")); // case-insensitive
        assert!(rule("between", "10", "20").matches_value("15"));
        assert!(rule("between", "10", "20").matches_value("10")); // inclusive
        assert!(!rule("between", "10", "20").matches_value("25"));
        assert!(rule("contains", "err", "").matches_value("ERROR log"));
        assert!(!rule("contains", "err", "").matches_value("ok"));
        assert!(!rule("gt", "150", "").matches_value(""), "blanks never match");
        assert!(!rule("gt", "150", "").matches_value("  "));
    }

    #[test]
    fn bounds_parse_and_reject() {
        assert_eq!(rule("gt", "1", "").bounds(), Some((0, 0, 9, 1))); // A1:B10
        let mut bad = rule("gt", "1", "");
        bad.range = "nonsense".into();
        assert_eq!(bad.bounds(), None);
    }

    #[test]
    fn lerp_hex_endpoints_and_midpoint() {
        assert_eq!(lerp_hex("#000000", "#ffffff", 0.0).as_deref(), Some("#000000"));
        assert_eq!(lerp_hex("#000000", "#ffffff", 1.0).as_deref(), Some("#ffffff"));
        assert_eq!(lerp_hex("#000000", "#ffffff", 0.5).as_deref(), Some("#808080"));
        assert_eq!(lerp_hex("#000000", "#ffffff", 7.0).as_deref(), Some("#ffffff")); // clamped
        assert_eq!(lerp_hex("nope", "#ffffff", 0.5), None);
    }
}
