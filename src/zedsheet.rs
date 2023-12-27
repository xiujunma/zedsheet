use crate::component::options::Options;
use crate::component::sheet::Sheet;
use gloo::utils::document;

#[derive(Debug, Clone)]
pub struct ZedSheet {
    pub options: Options,
    pub sheet_index: usize,
    pub sheet: Sheet
}

impl ZedSheet {
    fn new(selector: &str, options: Options) -> Self {
        let targetEl = document().query_selector(selector).unwrap().unwrap();

        Self {
            options,
            sheet_index: 0,
            sheet: Sheet {}
        }
    }
}