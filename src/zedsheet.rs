use crate::component::bottombar::{BottomBar, self};
use crate::component::options::Options;
use crate::component::sheet::Sheet;
use gloo::utils::document;

#[derive(Debug, Clone)]
pub struct ZedSheet {
    options: Options,
    sheet_index: usize,
    bottom_bar: Option<BottomBar>,
    sheet: Sheet
}

impl ZedSheet {
    pub fn new(selector: &str, options: Options) -> Self {
        let targetEl = document().query_selector(selector).unwrap().unwrap();

        let bottom_bar = if options.show_bottom_bar {
            Some(BottomBar{})
        } else {
            None
        };
        
        Self {
            options,
            sheet_index: 0,
            bottom_bar,
            sheet: Sheet {}
        }
    }
}