#![allow(dead_code)]
#![allow(unused_variables)]
use std::collections::HashMap;

use wasm_bindgen::{prelude::Closure, JsCast};

pub struct Element {
    pub tag: String,
    pub el: web_sys::Element,
    pub data: HashMap<String, String>
}

pub struct Box {
    pub top: i32,
    pub left: i32,
    pub width: i32,
    pub height: i32
}

impl Element {
    pub fn new(tag: &str, class_name: &str) -> Self {
        let window = web_sys::window().expect("global window does not exists");
		let document = window.document().expect("expecting a document on window");
        let el = document.create_element(&tag).unwrap();
        let r = document.body().unwrap().append_child(&el);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }

        el.set_class_name(class_name);
        Self {
            tag: tag.to_string(),
            el,
            data: HashMap::new()
        }
    }

    pub fn set_data(&mut self, key: &str, value: &str) -> &Self {
        self.data.insert(key.to_string(), value.to_string());
        self
    }

    pub fn get_data(&self, key: &str) -> Option<String> {
        self.data.get(key).cloned()
    }

    pub fn on(&self, event_names: &str, handler: fn(event: web_sys::Event)) -> &Self {
        let splits: Vec<String> = vec!(event_names.split(".").collect());
        let event_name = &splits[0];


        let closure = Closure::<dyn FnMut(_)>::new(move |evt: web_sys::Event| {
            handler(evt);
        });
        let r = self.el.add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref());

        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }

        closure.forget();
        self
    }

    pub fn get_offset(&self) -> Box {
        Box {
            top: self.el.client_top(),
            left: self.el.client_left(),
            width: self.el.client_width(),
            height: self.el.client_height()
        }
    }

    pub fn set_offset(&self, offset: Box) -> &Self {
        if offset.top > 0 {
            self.set_css("top", &offset.top.to_string());
        }
        if offset.left > 0 {
            self.set_css("left", &offset.left.to_string());
        }
        if offset.width > 0 {
            self.set_css("width", &offset.width.to_string());
        }
        if offset.height > 0 {
            self.set_css("height", &offset.height.to_string());
        }
        self
    }

    pub fn set_scroll(&self, value: Box) -> &Self {
        self.el.set_scroll_top(value.top);
        self.el.set_scroll_left(value.left);
        self
    }

    pub fn get_scroll(&self) -> Box {
        Box {
            top: self.el.scroll_top(),
            left: self.el.scroll_left(),
            width: 0,
            height: 0
        }
    }
    
    pub fn get_box(&self) -> Box {
        let rect = self.el.get_bounding_client_rect();
        Box {
            top: rect.top() as i32,
            left: rect.left() as i32,
            width: rect.width() as i32,
            height: rect.height() as i32
        }
    }

    pub fn get_parent(&self) -> Element {
        let parent = self.el.parent_element().unwrap();
        Element {
            tag: parent.tag_name().to_lowercase(),
            el: parent,
            data: HashMap::new()
        }
    }

    pub fn get_children(&self) -> web_sys::NodeList {
        return self.el.child_nodes();
    }

    pub fn set_children(&self, children: Vec<web_sys::Element>) -> &Self {
        children.iter().for_each(|child| {
            self.set_child(child.clone());
        });
        self
    }

    pub fn set_child(&self, child: web_sys::Element) -> &Self {
        let r = self.el.append_child(&child);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        self
    }

    pub fn contains(&self, el: web_sys::Element) -> bool {
        // TODO
        false
    }

    pub fn get_class_name(&self) -> String {
        self.get_class_name()
    }

    pub fn set_class_name(&self, class_name: &str) -> &Self {
        self.el.set_class_name(class_name);
        self
    }

    pub fn add_class(&self, class_name: &str) -> &Self {
        let r = self.el.class_list().add_1(class_name);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        self
    }

    pub fn has_class(&self, class_name: &str) -> bool {
        self.el.class_list().contains(class_name)
    }

    pub fn remove_class(&self, class_name: &str) -> &Self {
        let r = self.el.class_list().remove_1(class_name);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        self
    }

    pub fn toggle_class(&self, class_name: &str) -> &Self {
        let r = self.el.class_list().toggle(class_name);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        self
    }

    pub fn remove_child(&self, el: web_sys::Element) -> &Self {
        let r = self.el.remove_child(&el);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        self
    }

    pub fn set_css(&self, name: &str, value: &str) -> &Self {
        let r = self.el.set_attribute("style", &format!("{}: {}", name, value));
                match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        self
    }

    pub fn show(&self) -> &Self {
        self.set_css("display", "none");
        self
    }

    pub fn hide(&self) -> &Self {
        self.set_css("display", "block");
        self
    }
}