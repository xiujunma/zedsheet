use std::collections::HashMap;
use std::fmt::Debug;
use regex::Regex;
use crate::core::cell_range::CellRange;
use crate::renderer::alphabets::xy2expr;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct Validator {
    pub required: bool,
    pub value: String,
    pub type_: String,
    pub operator: String,
}

impl Validator {
    pub fn new(type_: &str, required: bool, value: &str, operator: &str) -> Self {
        Validator {
            required,
            value: value.to_string(),
            type_: type_.to_string(),
            operator: operator.to_string(),
        }
    }

    pub fn validate(&self, v: &str) -> (bool, String) {
        if self.required && v.trim().is_empty() {
            return (false, "Required field".to_string());
        }
        if v.trim().is_empty() {
            return (true, String::new());
        }

        let phone_regex = Regex::new(r"^[1-9]\d{10}$").unwrap();
        let email_regex = Regex::new(r"^\w+([-+.]\w+)*@\w+([-.]\w+)*\.\w+([-.]\w+)*$").unwrap();

        if self.type_ == "phone" && !phone_regex.is_match(v) {
            return (false, "Invalid phone format".to_string());
        }
        if self.type_ == "email" && !email_regex.is_match(v) {
            return (false, "Invalid email format".to_string());
        }

        if self.type_ == "list" {
            // Trim + lowercase both sides. Excel's list-validity dropdown is
            // case-insensitive and ignores surrounding whitespace.
            let needle = v.trim().to_lowercase();
            let mut found = false;
            for raw in self.value.split(',') {
                if raw.trim().to_lowercase() == needle {
                    found = true;
                    break;
                }
            }
            return if found {
                (true, String::new())
            } else {
                (false, "Value not in list".to_string())
            };
        }

        if !self.operator.is_empty() {
            // Parse the value(s) to f64. Post-fix: non-numeric input or
            // non-numeric configuration now fails closed instead of
            // silently coercing to 0.0 (which caused `eq 5` against
            // `"abc"` to be a false positive).
            let parsed_v = v.trim().parse::<f64>().ok();
            if parsed_v.is_none() {
                return (false, "Must be a number".to_string());
            }
            let v1 = parsed_v.unwrap();

            match self.operator.as_str() {
                "be" => {
                    let parts: Vec<&str> = self.value.split(',').collect();
                    if parts.len() == 2 {
                        let min = parts[0].trim().parse::<f64>().ok();
                        let max = parts[1].trim().parse::<f64>().ok();
                        if let (Some(min), Some(max)) = (min, max) {
                            let in_range = v1 >= min && v1 <= max;
                            return (
                                in_range,
                                if in_range { String::new() } else { format!("Between {} and {}", min, max) },
                            );
                        }
                    }
                    return (false, "Invalid validator value".to_string());
                }
                "nbe" => {
                    let parts: Vec<&str> = self.value.split(',').collect();
                    if parts.len() == 2 {
                        let min = parts[0].trim().parse::<f64>().ok();
                        let max = parts[1].trim().parse::<f64>().ok();
                        if let (Some(min), Some(max)) = (min, max) {
                            let out_of_range = v1 < min || v1 > max;
                            return (
                                out_of_range,
                                if out_of_range { String::new() } else { format!("Not between {} and {}", min, max) },
                            );
                        }
                    }
                    return (false, "Invalid validator value".to_string());
                }
                "eq" => match self.value.trim().parse::<f64>().ok() {
                    Some(val) => return (v1 == val, if v1 == val { String::new() } else { format!("Must equal {}", self.value) }),
                    None => return (false, "Invalid validator value".to_string()),
                },
                "neq" => match self.value.trim().parse::<f64>().ok() {
                    Some(val) => return (v1 != val, if v1 != val { String::new() } else { format!("Must not equal {}", self.value) }),
                    None => return (false, "Invalid validator value".to_string()),
                },
                "lt" => match self.value.trim().parse::<f64>().ok() {
                    Some(val) => return (v1 < val, if v1 < val { String::new() } else { format!("Must be less than {}", self.value) }),
                    None => return (false, "Invalid validator value".to_string()),
                },
                "lte" => match self.value.trim().parse::<f64>().ok() {
                    Some(val) => return (v1 <= val, if v1 <= val { String::new() } else { format!("Must be less than or equal to {}", self.value) }),
                    None => return (false, "Invalid validator value".to_string()),
                },
                "gt" => match self.value.trim().parse::<f64>().ok() {
                    Some(val) => return (v1 > val, if v1 > val { String::new() } else { format!("Must be greater than {}", self.value) }),
                    None => return (false, "Invalid validator value".to_string()),
                },
                "gte" => match self.value.trim().parse::<f64>().ok() {
                    Some(val) => return (v1 >= val, if v1 >= val { String::new() } else { format!("Must be greater than or equal to {}", self.value) }),
                    None => return (false, "Invalid validator value".to_string()),
                },
                _ => {}
            }
        }

        (true, String::new())
    }

    pub fn equals(&self, other: &Validator) -> bool {
        self.type_ == other.type_
            && self.required == other.required
            && self.operator == other.operator
            && self.value == other.value
    }
}

impl Clone for Validator {
    fn clone(&self) -> Self {
        Validator {
            required: self.required,
            value: self.value.clone(),
            type_: self.type_.clone(),
            operator: self.operator.clone(),
        }
    }
}

impl Debug for Validator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Validator")
            .field("required", &self.required)
            .field("value", &self.value)
            .field("type_", &self.type_)
            .field("operator", &self.operator)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
pub struct Validation {
    pub refs: Vec<String>,
    pub mode: String,
    pub validator: Validator,
}

impl Clone for Validation {
    fn clone(&self) -> Self {
        Validation {
            refs: self.refs.clone(),
            mode: self.mode.clone(),
            validator: self.validator.clone(),
        }
    }
}

impl Debug for Validation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Validation")
            .field("refs", &self.refs)
            .field("mode", &self.mode)
            .field("validator", &self.validator)
            .finish()
    }
}

impl Validation {
    pub fn new(mode: &str, refs: Vec<String>, validator: Validator) -> Self {
        Validation {
            mode: mode.to_string(),
            refs,
            validator,
        }
    }

    pub fn includes(&self, ri: usize, ci: usize) -> bool {
        for ref_ in &self.refs {
            if let Ok(cr) = CellRange::from_str(ref_) {
                if cr.includes(ri, ci) {
                    return true;
                }
            }
        }
        false
    }

    pub fn add_ref(&mut self, ref_: &str) {
        // Remove any existing refs that intersect with this new ref
        if let Ok(new_cr) = CellRange::from_str(ref_) {
            self.remove_ref(&new_cr);
        }
        self.refs.push(ref_.to_string());
    }

    pub fn remove_ref(&mut self, cell_range: &CellRange) {
        // Excel-style semantic: subtract `cell_range` from each ref. The
        // subtraction of two rectangles can produce up to 4 pieces; we
        // keep the at-most-4 resulting pieces (dropping any empty ones).
        // If the ref does not intersect the hole, keep it as is.
        let mut new_refs = Vec::new();
        for ref_ in &self.refs {
            if let Ok(cr) = CellRange::from_str(ref_) {
                for piece in subtract_range(&cr, cell_range) {
                    new_refs.push(piece);
                }
            } else {
                new_refs.push(ref_.clone());
            }
        }
        self.refs = new_refs;
    }
}

/// Subtract `hole` from `whole`, returning the remaining pieces as
/// A1-style ref strings. Returns an empty Vec when `hole` fully contains
/// `whole`. Returns `vec![whole.to_string()]` when they don't intersect.
/// The maximum number of pieces is 4 (the four edge strips).
fn subtract_range(whole: &CellRange, hole: &CellRange) -> Vec<String> {
    if !whole.intersects(hole) {
        return vec![whole.to_string()];
    }
    if hole.includes(whole.sri, whole.sci) && hole.includes(whole.eri, whole.eci) {
        // hole fully contains whole — nothing remains.
        return Vec::new();
    }
    let mut pieces: Vec<(usize, usize, usize, usize)> = Vec::new();
    // top strip: rows above the hole, full width of whole
    if whole.sri < hole.sri {
        pieces.push((whole.sri, whole.sci, hole.sri - 1, whole.eci));
    }
    // bottom strip
    if whole.eri > hole.eri {
        pieces.push((hole.eri + 1, whole.sci, whole.eri, whole.eci));
    }
    // middle band (rows overlapping the hole): left strip + right strip
    let mid_sri = whole.sri.max(hole.sri);
    let mid_eri = whole.eri.min(hole.eri);
    // left strip
    if whole.sci < hole.sci {
        pieces.push((mid_sri, whole.sci, mid_eri, hole.sci - 1));
    }
    // right strip
    if whole.eci > hole.eci {
        pieces.push((mid_sri, hole.eci + 1, mid_eri, whole.eci));
    }
    pieces
        .into_iter()
        .map(|(sri, sci, eri, eci)| {
            // CellRange pieces are (start_row, start_col, end_row, end_col).
            // `xy2expr` takes (col, row) and must be called with column first.
            let left = xy2expr(sci, sri);
            if sri == eri && sci == eci {
                left
            } else {
                format!("{}:{}", left, xy2expr(eci, eri))
            }
        })
        .collect()
}

#[derive(Serialize, Deserialize)]
pub struct Validations {
    #[serde(rename = "_")]
    validations: Vec<Validation>,
    #[serde(skip)]
    errors: HashMap<String, String>,
}

impl Clone for Validations {
    fn clone(&self) -> Self {
        Validations {
            validations: self.validations.clone(),
            errors: HashMap::new(),
        }
    }
}

impl Debug for Validations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Validations")
            .field("validations", &self.validations)
            .finish()
    }
}

impl Default for Validations {
    fn default() -> Self {
        Validations {
            validations: Vec::new(),
            errors: HashMap::new(),
        }
    }
}

impl Validations {
    pub fn new() -> Self {
        Validations::default()
    }

    pub fn get_error(&self, ri: usize, ci: usize) -> Option<&String> {
        self.errors.get(&format!("{}_{}", ri, ci))
    }

    pub fn validate(&mut self, ri: usize, ci: usize, text: &str) -> bool {
        // Returns true iff the value passes the validator (or there is no
        // validator on this cell). Post-fix: the return value now matches
        // the side effect on `errors` (previously it always returned true,
        // forcing callers to inspect `get_error`).
        let key = format!("{}_{}", ri, ci);
        if let Some(v) = self.get(ri, ci) {
            let (flag, message) = v.validator.validate(text);
            if !flag {
                self.errors.insert(key, message);
            } else {
                self.errors.remove(&key);
            }
            flag
        } else {
            self.errors.remove(&key);
            true
        }
    }

    pub fn add(&mut self, mode: &str, ref_: &str, validator: Validator) {
        if let Some(v) = self.get_by_validator(&validator) {
            v.add_ref(ref_);
        } else {
            self.validations.push(Validation::new(mode, vec![ref_.to_string()], validator));
        }
    }

    pub fn get_by_validator(&mut self, validator: &Validator) -> Option<&mut Validation> {
        self.validations.iter_mut().find(|v| v.validator.equals(validator))
    }

    pub fn get(&self, ri: usize, ci: usize) -> Option<&Validation> {
        self.validations.iter().find(|v| v.includes(ri, ci))
    }

    pub fn remove(&mut self, range: &CellRange) {
        for v in &mut self.validations {
            v.remove_ref(range);
        }
    }

    pub fn set_data(&mut self, data: Vec<Validation>) {
        self.validations = data;
    }

    pub fn get_data(&self) -> Vec<Validation> {
        self.validations.iter().filter(|v| !v.refs.is_empty()).cloned().collect()
    }
}

#[cfg(test)]
mod tests {
        use super::*;

        // --- Validator basics ---

        #[test]
        fn validator_new_round_trips() {
            let v = Validator::new("list", true, "a,b,c", "eq");
            assert_eq!(v.type_, "list");
            assert_eq!(v.value, "a,b,c");
            assert!(v.required);
            assert_eq!(v.operator, "eq");
        }

        #[test]
        fn validator_equals_distinguishes_every_field() {
            let base = Validator::new("list", true, "a,b", "eq");
            assert!(base.equals(&Validator::new("list", true, "a,b", "eq")));
            assert!(!base.equals(&Validator::new("number", true, "a,b", "eq")), "type differs");
            assert!(!base.equals(&Validator::new("list", false, "a,b", "eq")), "required differs");
            assert!(!base.equals(&Validator::new("list", true, "a", "eq")), "value differs");
            assert!(!base.equals(&Validator::new("list", true, "a,b", "neq")), "operator differs");
        }

        // --- required ---

        #[test]
        fn validate_required_empty() {
            let v = Validator::new("list", true, "a", "");
            assert_eq!(v.validate(""), (false, "Required field".to_string()));
        }

        #[test]
        fn validate_required_whitespace_only() {
            let v = Validator::new("list", true, "a", "");
            assert_eq!(v.validate("   "), (false, "Required field".to_string()));
        }

        #[test]
        fn validate_optional_empty_passes() {
            let v = Validator::new("list", false, "a", "");
            assert_eq!(v.validate(""), (true, String::new()));
        }

        // --- list ---

        #[test]
        fn validate_list_exact_match() {
            let v = Validator::new("list", false, "a,b,c", "");
            assert_eq!(v.validate("b"), (true, String::new()));
        }

        #[test]
        fn validate_list_trimmed_match() {
            // CSV item has surrounding whitespace; input also has surrounding whitespace.
            // Both are trimmed before comparison (Excel behavior).
            let v = Validator::new("list", false, "a, b ,c", "");
            assert_eq!(v.validate("  b  "), (true, String::new()));
        }

        #[test]
        fn validate_list_case_insensitive() {
            let v = Validator::new("list", false, "Yes,No,Maybe", "");
            assert_eq!(v.validate("yes"), (true, String::new()));
            assert_eq!(v.validate("MAYBE"), (true, String::new()));
        }

        #[test]
        fn validate_list_miss() {
            let v = Validator::new("list", false, "a,b,c", "");
            assert_eq!(v.validate("d"), (false, "Value not in list".to_string()));
        }

        #[test]
        fn validate_list_single_value() {
            let v = Validator::new("list", false, "only", "");
            assert_eq!(v.validate("only"), (true, String::new()));
            assert_eq!(v.validate("other"), (false, "Value not in list".to_string()));
        }

        // --- phone / email ---

        #[test]
        fn validate_phone_valid() {
            let v = Validator::new("phone", false, "", "");
            assert_eq!(v.validate("13800000000"), (true, String::new()));
        }

        #[test]
        fn validate_phone_too_short() {
            let v = Validator::new("phone", false, "", "");
            assert_eq!(v.validate("12345"), (false, "Invalid phone format".to_string()));
        }

        #[test]
        fn validate_phone_starts_with_zero() {
            let v = Validator::new("phone", false, "", "");
            assert_eq!(v.validate("01234567890"), (false, "Invalid phone format".to_string()));
        }

        #[test]
        fn validate_email_valid() {
            let v = Validator::new("email", false, "", "");
            assert_eq!(v.validate("a.b+c@sub.example.co"), (true, String::new()));
        }

        #[test]
        fn validate_email_invalid() {
            let v = Validator::new("email", false, "", "");
            assert_eq!(v.validate("not-an-email"), (false, "Invalid email format".to_string()));
        }

        // --- numeric operators ---

        #[test]
        fn validate_eq_numeric_match() {
            let v = Validator::new("number", false, "5", "eq");
            assert_eq!(v.validate("5"), (true, String::new()));
            assert_eq!(v.validate("4"), (false, "Must equal 5".to_string()));
        }

        #[test]
        fn validate_neq_numeric() {
            let v = Validator::new("number", false, "5", "neq");
            assert_eq!(v.validate("4"), (true, String::new()));
            assert_eq!(v.validate("5"), (false, "Must not equal 5".to_string()));
        }

        #[test]
        fn validate_lt_lte_gt_gte() {
            let lt = Validator::new("number", false, "10", "lt");
            assert_eq!(lt.validate("9"), (true, String::new()));
            assert_eq!(lt.validate("10"), (false, "Must be less than 10".to_string()));

            let lte = Validator::new("number", false, "10", "lte");
            assert_eq!(lte.validate("10"), (true, String::new()));
            assert_eq!(lte.validate("11"), (false, "Must be less than or equal to 10".to_string()));

            let gt = Validator::new("number", false, "10", "gt");
            assert_eq!(gt.validate("11"), (true, String::new()));
            assert_eq!(gt.validate("10"), (false, "Must be greater than 10".to_string()));

            let gte = Validator::new("number", false, "10", "gte");
            assert_eq!(gte.validate("10"), (true, String::new()));
            assert_eq!(gte.validate("9"), (false, "Must be greater than or equal to 10".to_string()));
        }

        #[test]
        fn validate_be_inclusive() {
            let v = Validator::new("number", false, "1,10", "be");
            assert_eq!(v.validate("1"), (true, String::new()));
            assert_eq!(v.validate("5"), (true, String::new()));
            assert_eq!(v.validate("10"), (true, String::new()));
            assert_eq!(v.validate("0"), (false, "Between 1 and 10".to_string()));
            assert_eq!(v.validate("11"), (false, "Between 1 and 10".to_string()));
        }

        #[test]
        fn validate_nbe_exclusive() {
            let v = Validator::new("number", false, "1,10", "nbe");
            assert_eq!(v.validate("0"), (true, String::new()));
            assert_eq!(v.validate("11"), (true, String::new()));
            assert_eq!(v.validate("5"), (false, "Not between 1 and 10".to_string()));
        }

        // --- post-fix: numeric validation rejects non-numeric input ---

        #[test]
        fn validate_eq_rejects_non_numeric_input() {
            // Pre-fix bug: parse_value silently coerced to 0.0 and "abc" == 0 was
            // a false positive. Post-fix, non-numeric input fails.
            let v = Validator::new("number", false, "5", "eq");
            let (ok, msg) = v.validate("abc");
            assert!(!ok, "non-numeric input must fail");
            assert!(!msg.is_empty(), "must surface an error message");
        }

        #[test]
        fn validate_be_rejects_non_numeric_input() {
            let v = Validator::new("number", false, "1,10", "be");
            let (ok, _) = v.validate("not a number");
            assert!(!ok);
        }

        // --- Validation::includes / add_ref / remove_ref ---

        #[test]
        fn validation_includes_single_cell_and_range() {
            let v = Validation::new("cell", vec!["A1".into(), "C3:E5".into()], Validator::new("list", false, "a", ""));
            assert!(v.includes(0, 0));
            assert!(!v.includes(0, 1));
            assert!(v.includes(2, 2));
            assert!(v.includes(4, 4));
            assert!(!v.includes(5, 5));
        }

        #[test]
        fn validation_add_ref_intersect_strip() {
            // add_ref("A1") then add_ref("A1:B2") leaves a single ref A1:B2.
            let mut v = Validation::new("cell", vec!["A1".into()], Validator::new("list", false, "a", ""));
            v.add_ref("A1:B2");
            assert_eq!(v.refs, vec!["A1:B2".to_string()]);
        }

        #[test]
        fn validation_remove_ref_drops_overlapping() {
            let mut v = Validation::new("cell", vec!["A1:B3".into(), "C1".into()], Validator::new("list", false, "a", ""));
            let cr = CellRange::from_str("A1:B3").unwrap();
            v.remove_ref(&cr);
            assert_eq!(v.refs, vec!["C1".to_string()]);
        }

        // --- Validations container ---

        #[test]
        fn validations_add_groups_same_validator() {
            let mut vs = Validations::new();
            let v = Validator::new("list", false, "a,b", "");
            vs.add("cell", "A1", v.clone());
            vs.add("cell", "B5", v.clone());
            // Both refs collapse into one Validation entry.
            assert_eq!(vs.validations.len(), 1);
            assert_eq!(vs.validations[0].refs, vec!["A1".to_string(), "B5".to_string()]);
        }

        #[test]
        fn validations_get_returns_matching() {
            let mut vs = Validations::new();
            vs.add("cell", "A1", Validator::new("list", false, "a", ""));
            vs.add("cell", "B5", Validator::new("list", false, "b", ""));
            assert!(vs.get(0, 0).is_some());
            assert!(vs.get(4, 1).is_some());
            assert!(vs.get(10, 10).is_none());
        }

        #[test]
        fn validations_validate_populates_error_then_clears() {
            let mut vs = Validations::new();
            vs.add("cell", "A1", Validator::new("list", false, "a,b", ""));
            // First write: invalid value.
            let ok = vs.validate(0, 0, "z");
            assert!(!ok, "validate() should return false on invalid input");
            assert!(vs.get_error(0, 0).is_some(), "invalid input should set an error");
            // Now a valid value clears it.
            let ok2 = vs.validate(0, 0, "a");
            assert!(ok2, "validate() should return true on valid input");
            assert!(vs.get_error(0, 0).is_none(), "valid input should clear the error");
        }

        #[test]
        fn validations_remove_strips_overlapping_refs() {
            let mut vs = Validations::new();
            let v = Validator::new("list", false, "a,b", "");
            vs.add("cell", "A1:B3", v.clone());
            vs.add("cell", "C1", v.clone());
            // After remove A1:B2, only B3 + C1 remain in the first validation.
            let cr = CellRange::from_str("A1:B2").unwrap();
            vs.remove(&cr);
            assert!(vs.get(0, 0).is_none(), "A1 is in A1:B2 and should be cleared");
            assert!(vs.get(1, 1).is_none(), "B2 is in A1:B2 and should be cleared");
            assert!(vs.get(2, 1).is_some(), "B3 is still covered by A1:B3");
            assert!(vs.get(0, 2).is_some(), "C1 is still covered");
        }

        #[test]
        fn validations_get_data_drops_empty_refs() {
            let mut vs = Validations::new();
            let v = Validator::new("list", false, "a,b", "");
            vs.add("cell", "A1", v.clone());
            // Remove the only ref — the validation becomes empty.
            let cr = CellRange::from_str("A1").unwrap();
            vs.remove(&cr);
            // get_data() filters empty-ref validations so JSON doesn't carry dead entries.
            assert!(vs.get_data().is_empty());
        }

        #[test]
        fn validations_set_data_round_trip() {
            let mut vs = Validations::new();
            vs.add("cell", "A1", Validator::new("list", false, "a,b", ""));
            vs.add("cell", "C3:E5", Validator::new("number", false, "1,10", "be"));
            let serialized = vs.get_data();
            let mut vs2 = Validations::new();
            vs2.set_data(serialized);
            // Order is preserved.
            assert_eq!(vs2.validations.len(), 2);
            assert!(vs2.get(0, 0).is_some());
            assert!(vs2.get(4, 4).is_some());
        }

        #[test]
        fn validations_validate_returns_bool_meaning() {
            // Post-fix: validate() now returns true iff the value is valid (errors cleared)
            // or there is no validator; false iff the value is invalid.
            let mut vs = Validations::new();
            vs.add("cell", "A1", Validator::new("list", false, "a,b", ""));
            assert!(!vs.validate(0, 0, "z"), "invalid input → false");
            assert!(vs.validate(0, 0, "a"), "valid input → true");
            assert!(vs.validate(0, 0, ""), "empty input on optional rule → true");
        }
    }