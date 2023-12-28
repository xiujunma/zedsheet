
#[derive(Debug, Clone)]
pub struct Options {
    pub show_bottom_bar: bool
}

impl Default for Options {
    fn default() -> Self {
        Self {
            show_bottom_bar: true
        }
    }
}