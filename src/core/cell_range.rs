#[derive(Debug, Clone)]
pub struct CellRange {
    pub sri: usize,
    pub sci: usize,
    pub eri: usize,
    pub eci: usize,
    pub w: f64,
    pub h: f64,
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
}