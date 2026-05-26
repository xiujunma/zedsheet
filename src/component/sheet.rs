// Sheet component - basic structure
// Full implementation will be in lib.rs / main entry point

use crate::core::data_proxy::DataProxy;

#[derive(Debug, Clone)]
pub struct SheetState {
    pub width: f64,
    pub height: f64,
    pub scroll_x: f64,
    pub scroll_y: f64,
}

impl Default for SheetState {
    fn default() -> Self {
        SheetState {
            width: 800f64,
            height: 600f64,
            scroll_x: 0f64,
            scroll_y: 0f64,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Sheet {
    pub state: SheetState,
    pub data: DataProxy,
}

impl Sheet {
    pub fn new(width: f64, height: f64) -> Self {
        Sheet {
            state: SheetState { width, height, scroll_x: 0f64, scroll_y: 0f64 },
            data: DataProxy::new("sheet1"),
        }
    }

    pub fn with_data(data: DataProxy, width: f64, height: f64) -> Self {
        Sheet {
            state: SheetState { width, height, scroll_x: 0f64, scroll_y: 0f64 },
            data,
        }
    }
}