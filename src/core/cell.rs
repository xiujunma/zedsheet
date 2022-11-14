use crate::core::evaluable::Evaluable;

pub struct Cell {
    pub text: String
}

impl Evaluable for Cell {
    fn evaluate(&self) -> String {
        self.text.clone()
    }
}