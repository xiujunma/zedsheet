pub struct Element {
    tag: String,
    el: web_sys::Element
}

impl Element {
    pub fn new(tag:String) -> Self {
        let window = web_sys::window().expect("global window does not exists");
		let document = window.document().expect("expecting a document on window");
        let el = document.create_element(&tag).unwrap();
        let r = document.body().unwrap().append_child(&el);
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