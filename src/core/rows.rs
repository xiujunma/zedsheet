#[derive(Clone)]
pub struct Rows {
    length: usize,
    height: usize,
}

impl Rows {
    pub fn new() -> Rows {
        Rows {
            length: 0,
            height: 0,
        }
    }

    fn get_height(&self) -> usize {
        self.height
    }

    fn set_height(&mut self, height: usize) {
        self.height = height;
    }
    
}
