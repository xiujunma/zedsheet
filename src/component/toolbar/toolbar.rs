use crate::{component::element::{Element, h}, config::css_prefix};

pub struct Toolbar {
}

impl Toolbar {
    pub fn new(mut target_el: Element) -> Self {

        let mut el = h("div", Some(format!("{}-sheet", css_prefix).as_str()));
        
        target_el.append_child(&mut el);
        Toolbar {
        }
    }
}