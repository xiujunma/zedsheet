use std::fmt::Display;
use crate::renderer::alphabets::{ exp2xy, xy2expr };

#[derive(Debug, Clone)]
pub struct CellRange {
    pub sri: usize,
    pub sci: usize,
    pub eri: usize,
    pub eci: usize,
    pub w: f64,
    pub h: f64,
}

impl Display for CellRange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {

        let mut cell_ref = xy2expr(self.sri, self.sci);

        if self.multiple() {
            cell_ref = format!("{}:{}", cell_ref, xy2expr(self.eri, self.eci));
        }

        write!(f, "{}", cell_ref)
    }
}

impl CellRange {
    pub fn new(sri: usize, sci: usize, eri: usize, eci: usize) -> Self {
        CellRange {
            sri,
            sci,
            eri,
            eci,
            w: 0f64,
            h: 0f64,
        }
    }

    pub fn set(&mut self, sri: usize, sci: usize, eri: usize, eci: usize) {
        self.sri = sri;
        self.sci = sci;
        self.eri = eri;
        self.eci = eci;
    }

    pub fn multiple(&self) -> bool {
        self.sri != self.eri || self.sci != self.eci
    }

    pub fn includes(&self, ri: usize, ci: usize) -> bool {
        self.sri <= ri && ri <= self.eri && self.sci <= ci && ci <= self.eci
    }

    pub fn includes_cell_ref(&self, cell_ref: &str) -> bool {
        let (ri, ci) = exp2xy(cell_ref);
        return self.sri < ri && self.eri > ri && self.sci < ci && self.eci > ci;
    }

    pub fn size(&self) -> (usize, usize) {
        (self.eri - self.sri + 1, self.eci - self.sci + 1)
    }

    pub fn each(&self, f: impl Fn(usize, usize)) {
        for ri in self.sri..=self.eri {
            for ci in self.sci..=self.eci {
                f(ri, ci);
            }
        }
    }

    pub fn contains(&self, range: &Self) -> bool {
        self.sri <= range.sri && self.sci <= range.sci && self.eri >= range.eri && self.eci >= range.eci
    }

    pub fn within(&self, range: &Self) -> bool {
        self.sri >= range.sri && self.sci >= range.sci && self.eri <= range.eri && self.eci <= range.eci
    }

    pub fn disjoint(&self, range: &Self) -> bool {
        self.sri > range.eri || self.sci > range.eci || self.eri < range.sri || self.eci < range.sci
    }

    pub fn intersects(&self, range: &Self) -> bool {
        !self.disjoint(range)
    }

    pub fn union(&self, range: &Self) -> Self {
        let mut sri = self.sri;
        let mut sci = self.sci;
        let mut eri = self.eri;
        let mut eci = self.eci;

        if range.sri < sri {
            sri = range.sri;
        }

        if range.sci < sci {
            sci = range.sci;
        }

        if range.eri > eri {
            eri = range.eri;
        }

        if range.eci > eci {
            eci = range.eci;
        }

        CellRange::new(sri, sci, eri, eci)
    }

    pub fn difference(&self, range: &Self) -> Vec<Self> {
        vec![]
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    // Ported from x-spreadsheet test/core/cell_range_test.js
    #[test]
    fn constructor_and_set() {
        let cr = CellRange::new(1, 2, 3, 4);
        assert_eq!((cr.sri, cr.sci, cr.eri, cr.eci), (1, 2, 3, 4));
        assert_eq!(cr.w, 0.0);
        assert_eq!(cr.h, 0.0);
        let mut cr = CellRange::new(0, 0, 0, 0);
        cr.set(1, 2, 3, 4);
        assert_eq!((cr.sri, cr.sci, cr.eri, cr.eci), (1, 2, 3, 4));
    }

    #[test]
    fn multiple() {
        assert!(CellRange::new(1, 2, 1, 3).multiple());
        assert!(CellRange::new(1, 1, 2, 1).multiple());
        assert!(!CellRange::new(1, 1, 1, 1).multiple());
    }

    #[test]
    fn includes() {
        let cr = CellRange::new(0, 0, 9, 1);
        assert!(cr.includes(9, 0)); // A10
        assert!(cr.includes(0, 1)); // (0,1)
        assert!(!cr.includes(10, 0)); // A11
    }

    #[test]
    fn contains() {
        let cr = CellRange::new(0, 0, 5, 5);
        assert!(cr.contains(&CellRange::new(2, 2, 2, 2)));
        assert!(cr.contains(&CellRange::new(5, 5, 5, 5)));
        assert!(!cr.contains(&CellRange::new(5, 6, 5, 6)));
        assert!(!CellRange::new(2, 2, 5, 5).contains(&CellRange::new(1, 1, 3, 3)));
    }

    #[test]
    fn within() {
        assert!(!CellRange::new(0, 0, 5, 5).within(&CellRange::new(2, 2, 2, 2)));
        assert!(!CellRange::new(1, 1, 1, 6).within(&CellRange::new(2, 2, 5, 5)));
        assert!(!CellRange::new(6, 3, 6, 4).within(&CellRange::new(2, 2, 5, 5)));
        assert!(CellRange::new(2, 2, 2, 2).within(&CellRange::new(2, 2, 5, 5)));
    }

    #[test]
    fn disjoint() {
        let cr = CellRange::new(4, 4, 6, 8);
        assert!(cr.disjoint(&CellRange::new(2, 2, 2, 2)));
        assert!(cr.disjoint(&CellRange::new(2, 2, 3, 2)));
        assert!(!cr.disjoint(&CellRange::new(4, 4, 4, 4)));
        assert!(!cr.disjoint(&CellRange::new(5, 2, 5, 9)));
    }

    #[test]
    fn intersects() {
        let cr = CellRange::new(3, 3, 8, 8);
        // intersecting cases
        for o in [
            CellRange::new(5, 5, 5, 5),
            CellRange::new(4, 2, 4, 9),
            CellRange::new(2, 4, 9, 4),
            CellRange::new(1, 5, 3, 9),
            CellRange::new(8, 5, 10, 9),
            CellRange::new(3, 1, 4, 5),
            CellRange::new(3, 8, 4, 10),
            CellRange::new(3, 3, 3, 3),
            CellRange::new(8, 8, 8, 8),
        ] {
            assert!(cr.intersects(&o), "expected intersect");
        }
        // non-intersecting cases
        for o in [
            CellRange::new(2, 4, 2, 7),
            CellRange::new(9, 4, 9, 7),
            CellRange::new(4, 1, 4, 2),
            CellRange::new(4, 9, 4, 10),
        ] {
            assert!(!cr.intersects(&o), "expected disjoint");
        }
    }

    #[test]
    fn union() {
        let ret = CellRange::new(1, 1, 3, 3).union(&CellRange::new(3, 3, 5, 5));
        assert_eq!((ret.sri, ret.sci, ret.eri, ret.eci), (1, 1, 5, 5));
        let ret = CellRange::new(2, 1, 5, 5).union(&CellRange::new(1, 3, 2, 7));
        assert_eq!((ret.sri, ret.sci, ret.eri, ret.eci), (1, 1, 5, 7));
        let ret = CellRange::new(1, 1, 5, 5).union(&CellRange::new(3, 1, 7, 1));
        assert_eq!((ret.sri, ret.sci, ret.eri, ret.eci), (1, 1, 7, 5));
    }
}
