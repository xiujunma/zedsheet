use crate::component::bottombar::{BottomBar, self};
use crate::component::options::Options;
use crate::component::sheet::Sheet;
use gloo::utils::document;
use web_sys::Event;

use crate::config::css_prefix;
use crate::component::element::h;

#[derive(Debug, Clone)]
pub struct ZedSheet {
    options: Options,
    sheet_index: usize,
    bottom_bar: Option<BottomBar>,
    sheet: Sheet
}

impl ZedSheet {
    pub fn new(selector: &str, options: Options) -> Self {
        let target_el = document().query_selector(selector).unwrap().unwrap();

        let bottom_bar = if options.show_bottom_bar {
            Some(BottomBar{})
        } else {
            None
        };
        
        let mut root_el = h("div", Some(css_prefix));

        root_el.add_event_listener("contextmenu", |event: Event| event.prevent_default());

        target_el.append_child(&root_el.el.unwrap()).unwrap();

        Self {
            options,
            sheet_index: 0,
            bottom_bar,
            sheet: Sheet {}
        }
    }
}