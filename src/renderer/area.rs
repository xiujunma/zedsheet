pub struct Area {
    range: Range,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    row_height: impl Fn(usize) -> usize,
    col_width: impl Fn(usize) -> usize,
}