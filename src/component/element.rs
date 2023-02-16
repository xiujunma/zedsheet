use std::collections::HashMap;

pub struct Element {
    tag: String,
    el: web_sys::Element,
    data: HashMap<String, String>
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

    pub fn on(&self, event_name: String, handler: fn(event: web_sys::Event)) -> &Self {
        // self.el.add_event_listener_with_callback(type_, listener);
        self
    }

    
}