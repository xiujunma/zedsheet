use crate::core::cell_range::CellRange;
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
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() == 2 {
            let (sci, sri) = exp2xy(parts[0].trim());
            let (eci, eri) = exp2xy(parts[1].trim());
            Ok(CellRange::new(sri, sci, eri, eci))
        } else if parts.len() == 1 {
            let (ci, ri) = exp2xy(parts[0].trim());
            Ok(CellRange::new(ri, ci, ri, ci))
        } else {
            Err(())
        }
    }
}