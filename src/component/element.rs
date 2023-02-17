use std::collections::HashMap;

use wasm_bindgen::{prelude::Closure, JsCast};

pub struct Element {
    pub tag: String,
    pub el: web_sys::Element,
    pub data: HashMap<String, String>
}

impl Element {
    pub fn new(tag: String, class_name: String) -> Self {
        let window = web_sys::window().expect("global window does not exists");
		let document = window.document().expect("expecting a document on window");
        let el = document.create_element(&tag).unwrap();
        let r = document.body().unwrap().append_child(&el);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }

        el.set_class_name(&class_name);
        Self {
            tag,
            el,
            data: HashMap::new()
        }
    }

    pub fn set_data(&mut self, key: String, value: String) -> &Self {
        self.data.insert(key, value);
        self
    }

    pub fn get_data(&self, key: String) -> Option<String> {
        self.data.get(&key).cloned()
    }

    pub fn on(&self, event_names: String, handler: fn(event: web_sys::Event)) -> &Self {
        let splits: Vec<String> = vec!(event_names.split(".").collect());
        let event_name = &splits[0];


        let closure = Closure::<dyn FnMut(_)>::new(move |evt: web_sys::Event| {
            handler(evt);
        });
        self.el.add_event_listener_with_callback(event_name, closure.as_ref().unchecked_ref());
        closure.forget();
        self
    }

    pub fn offset(&self, value: String) -> &Self {
        self
    }

    pub fn scroll(&self, value: String) -> &Self {
        self
    }
    
    pub fn get_box(&self) -> &Self {
        self
    }

    pub fn get_parent(&self) -> Element {
        let parent = self.el.parent_element().unwrap();
        Element {
            tag: parent.tag_name().to_lowercase(),
            el: parent,
            data: HashMap::new()
        }
    }
}