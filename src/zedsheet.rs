use crate::component::bottombar::{BottomBar, self};
use crate::component::options::Options;
use crate::component::sheet::Sheet;
use gloo::utils::document;

use crate::config::CSS_PREFIX;
use crate::component::element::h;

#[derive(Debug, Clone)]
pub struct ZedSheet {
    options: Options,
    sheet_index: usize,
    bottom_bar: Option<BottomBar>,
    sheet: Sheet,
}

impl ZedSheet {
    pub fn new(selector: &str, options: Options) -> Self {
        let target_el = document().query_selector(selector).unwrap().unwrap();

        let bottom_bar = if options.show_bottom_bar {
            Some(BottomBar{})
        } else {
            None
        };

        let mut root_el = h("div", Some(CSS_PREFIX));

        root_el.add_event_listener("contextmenu", |_event: web_sys::Event| {});

        target_el.append_child(&mut root_el.el.take().unwrap()).unwrap();

        Self {
            options,
            sheet_index: 0,
            bottom_bar,
            sheet: Sheet::new(800f64, 600f64),
        }
    }
}