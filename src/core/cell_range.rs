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

    pub fn includes(&self, cell_ref: &str) -> bool {
        let (ri, ci) = exp2xy(cell_ref);
        return self.sri < ri && self.eri > ri && self.sci < ci && self.eci > ci;
    }

    pub fn each(&self, f: impl Fn(usize, usize)) {
        for ri in self.sri..=self.eri {
            for ci in self.sci..=self.eci {
                f(ri, ci);
            }
        }
    }

    pub fn contains(&self, range: Self) -> bool {
        self.sri <= range.sri && self.sci <= range.sci && self.eri >= range.eri && self.eci >= range.eci
    }

    pub fn within(&self, range: Self) -> bool {
        self.sri >= range.sri && self.sci >= range.sci && self.eri <= range.eri && self.eci <= range.eci
    }

    pub fn disjoint(&self, range: Self) -> bool {
        self.sri > range.eri || self.sci > range.eci || self.eri < range.sri || self.eci < range.sci
    }

    pub fn intersects(&self, range: Self) -> bool {
        !self.disjoint(range)
    }

    pub fn union(&self, range: Self) -> Self {
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

    pub fn difference(&self, range: Self) -> Vec<Self> {
        vec![]
    }
}