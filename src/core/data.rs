#![allow(dead_code)]
#![allow(unused_variables)]

#[derive(Clone)]
pub struct Rows {
}

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