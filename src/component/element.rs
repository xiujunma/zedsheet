use std::collections::HashMap;
use web_sys::Element;
pub struct ElementEx {
    tag: String,
    el: Element
}

impl ElementEx {
    pub fn new(tag:&String) -> Self {
        // let document = web_sys::window().unwrap().document().unwrap();
        // self.el = documnet.create_element(tag).unwrap();

        let window = web_sys::window().expect("global window does not exists");    
		let document = window.document().expect("expecting a document on window");
        let el = document.create_element(tag).unwrap();
        document.append_child(&el);
        Self {
            tag: tag.to_string(),
            el
        }
    }
}