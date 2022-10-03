use crate::core::evaluable::Evaluable;

pub struct Cell {
    pub text: String
}

impl Evaluable for Cell {
    fn evalute(&self) -> String {
        self.text.clone()
    }
}