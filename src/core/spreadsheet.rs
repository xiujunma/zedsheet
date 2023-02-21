#![allow(dead_code)]
#![allow(unused_variables)]

use crate::component::sheet::Sheet;
use crate::core::data::Data;
use crate::core::options::Options;

pub struct Spreadsheet {
    target_el: web_sys::Element,
    options: Options,
    index: u8,
    datas: Vec<Data>,
    bottom_bar: String,
    sheet: Sheet
}

impl Spreadsheet {
    pub fn new(target_el: web_sys::Element, options: Options) -> Spreadsheet {
        Spreadsheet {
            target_el,
            options,
            index: 0,
            datas: vec![],
            bottom_bar: String::from(""),
            sheet: Sheet::new()
        }
    }
}