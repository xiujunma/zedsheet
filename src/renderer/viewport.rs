use crate::renderer::table_renderer::TableRenderer;
use crate::renderer::area::Area;

pub struct Viewport {
    pub areas: Vec<Area>,
    pub header_areas: Vec<Area>,
    render: TableRenderer
}

impl Viewport {
    fn new(render: TableRenderer) -> Self {
        return Self {
            areas: Vec::new(),
            header_areas: Vec::new(),
            render
        };
    }
}