use web_sys::Element;
pub struct ElementEx {
    tag: String,
    el: Element
}

impl ElementEx {
    pub fn new(tag:String) -> Self {
        let window = web_sys::window().expect("global window does not exists");
		let document = window.document().expect("expecting a document on window");
        let el = document.create_element(&tag).unwrap();
        let r = document.append_child(&el);
        match r {
            Ok(v) => println!("working with version: {v:?}"),
            Err(e) => println!("error parsing header: {e:?}"),
        }
        Self {
            tag,
            el
        }
    }
}