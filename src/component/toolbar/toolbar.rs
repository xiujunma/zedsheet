use crate::{component::element::{Element, h}, config::CSS_PREFIX};

pub struct Toolbar {
}

impl Toolbar {
    pub fn new(mut target_el: Element) -> Self {

        let mut el = h("div", Some(format!("{}-sheet", CSS_PREFIX).as_str()));
        
        target_el.append_child(&mut el);
        Toolbar {
        }
    }
}