pub struct Viewpoint {
    pub areas: Vec<Area>,
    pub header_areas: Vec<Area>,
    render: TableRenderer
}

impl Viewpoint {
    fn new(render: TableRenderer) -> Self {
        return Self {
            areas: Vec::new(),
            header_areas: Vec::new(),
            render
        }
    }
    // TODO
}