struct Icon {
  name: String,
  iconNameEl: Element
}

impl Icon {
  pub fn new(name: &str) -> Self {
    let wrapper = Element::new("div", "icon");
    let iconNameEl = Element::new("i", "material-icons");

    wrapper.append_child(&iconNameEl);
    Self {
      name,
      iconNameEl
    }
  }

  pub fn set_name(&self, name: &str) {
    self.iconNameEl.set_class_name(name);
  }
}