use crate::core::cell_range::CellRange;
use crate::formula::parser::looks_like_cell_ref;
use crate::renderer::alphabets::exp2xy;

#[derive(Debug, Clone)]
pub struct Merges {
    ranges: Vec<CellRange>,
}

impl Default for Merges {
    fn default() -> Self {
        Merges {
            ranges: Vec::new(),
        }
    }
}

impl Merges {
    pub fn new() -> Self {
        Merges::default()
    }

    pub fn add(&mut self, range: CellRange) {
        self.delete_within(&range);
        self.ranges.push(range);
    }

    pub fn delete(&mut self, ri: usize, ci: usize) {
        self.ranges.retain(|r| !r.includes(ri, ci));
    }

    pub fn delete_within(&mut self, range: &CellRange) {
        self.ranges.retain(|r| !r.within(range));
    }

    /// Drop every merge overlapping `range`. Used by cell insert/delete, which
    /// can't keep a merge straddling the shifted band (issue #14).
    pub fn delete_intersecting(&mut self, range: &CellRange) {
        self.ranges.retain(|r| !r.intersects(range));
    }

    pub fn get_first_includes(&self, ri: usize, ci: usize) -> Option<&CellRange> {
        self.ranges.iter().find(|r| r.includes(ri, ci))
    }

    pub fn intersects(&self, range: &CellRange) -> bool {
        self.ranges.iter().any(|r| r.intersects(range))
    }

    pub fn filter_intersects(&self, range: &CellRange) -> Self {
        Merges {
            ranges: self.ranges.iter().filter(|r| r.intersects(range)).cloned().collect(),
        }
    }

    pub fn union(&self, range: CellRange) -> CellRange {
        let mut result = range;
        for r in &self.ranges {
            if r.intersects(&result) {
                result = r.union(&result);
            }
        }
        result
    }

    // type: "row" | "column"
    // n: positive for add, negative for delete
    pub fn shift(&mut self, type_: &str, index: usize, n: isize, cb: impl Fn(usize, usize, isize, isize)) {
        for range in &mut self.ranges {
            let sri = range.sri;
            let sci = range.sci;
            let eri = range.eri;
            let eci = range.eci;

            if type_ == "row" {
                if sri >= index {
                    range.sri = (sri as isize + n as isize) as usize;
                    range.eri = (eri as isize + n as isize) as usize;
                } else if sri < index && index <= eri {
                    range.eri = (eri as isize + n as isize) as usize;
                    cb(sri, sci, n, 0);
                }
            } else if type_ == "column" {
                if sci >= index {
                    range.sci = (sci as isize + n as isize) as usize;
                    range.eci = (eci as isize + n as isize) as usize;
                } else if sci < index && index <= eci {
                    range.eci = (eci as isize + n as isize) as usize;
                    cb(sri, sci, 0, n);
                }
            }
        }
    }

    pub fn move_(&mut self, range: &CellRange, rn: isize, cn: isize) {
        for r in &mut self.ranges {
            if r.within(range) {
                r.eri = (r.eri as isize + rn) as usize;
                r.sri = (r.sri as isize + rn) as usize;
                r.sci = (r.sci as isize + cn) as usize;
                r.eci = (r.eci as isize + cn) as usize;
            }
        }
    }

    pub fn set_data(&mut self, data: Vec<String>) {
        self.ranges = data
            .iter()
            .filter_map(|s| CellRange::from_str(s).ok())
            .collect();
    }

    pub fn get_data(&self) -> Vec<String> {
        self.ranges.iter().map(|r| r.to_string()).collect()
    }

    pub fn for_each<F>(&self, f: F)
    where
        F: FnMut(&CellRange),
    {
        self.ranges.iter().for_each(f)
    }

    pub fn len(&self) -> usize {
        self.ranges.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty()
    }
}

impl CellRange {
    pub fn from_str(s: &str) -> Result<CellRange, ()> {
        // Use the project-wide 0-indexed `exp2xy` (consistent with
        // `data_proxy` / `alphabets`). The previous local `parse_cell_ref`
        // returned 1-indexed columns which made `from_str("A1")` produce
        // `(1, 0)` instead of `(0, 0)` and broke every `includes` call.
        // Validate each part is a real cell ref BEFORE calling exp2xy, which
        // `.unwrap()`s on its `parse::<usize>()` and would otherwise panic (and
        // abort the WASM module) on malformed input like `"A"`, `":"` or `""`.
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 2 {
            let (a, b) = (parts[0].trim(), parts[1].trim());
            if !looks_like_cell_ref(a) || !looks_like_cell_ref(b) {
                return Err(());
            }
            let (sci, sri) = exp2xy(a);
            let (eci, eri) = exp2xy(b);
            Ok(CellRange::new(sri, sci, eri, eci))
        } else if parts.len() == 1 {
            let a = parts[0].trim();
            if !looks_like_cell_ref(a) {
                return Err(());
            }
            let (ci, ri) = exp2xy(a);
            Ok(CellRange::new(ri, ci, ri, ci))
        } else {
            Err(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_str_parses_valid_refs() {
        assert!(CellRange::from_str("A1").is_ok());
        assert!(CellRange::from_str("A1:B2").is_ok());
        assert!(CellRange::from_str("AA10:BC99").is_ok());
    }

    #[test]
    fn from_str_rejects_malformed_input_without_panicking() {
        // These previously reached exp2xy's `.parse().unwrap()` and panicked
        // (aborting the WASM module) when typed into the DV "Apply to" field.
        assert!(CellRange::from_str("A").is_err(), "no row digit");
        assert!(CellRange::from_str("1").is_err(), "no column letter");
        assert!(CellRange::from_str("").is_err(), "empty");
        assert!(CellRange::from_str(":").is_err(), "empty parts");
        assert!(CellRange::from_str("A1:").is_err(), "empty end");
        assert!(CellRange::from_str(":B2").is_err(), "empty start");
    }
}