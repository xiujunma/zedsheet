#![allow(dead_code)]
#![allow(unused_variables)]

use crate::core::rows::Rows;

#[derive(Clone)]
pub struct Cols {

}

#[derive(Clone)]
pub struct Data {
    name: String,
    styles: Vec<String>,
    rows: Rows,
    cols: Cols
}