#![allow(dead_code)]

const ALPHABETS: &'static str = "ABCDEFGHIJKLMNOPQRSTUVWXYZ";

fn alphabet(index: usize) -> char {
    return ALPHABETS.chars().nth(index).unwrap()
}

pub fn string_at(index: usize) -> String {
    let mut i = index as isize;
    let mut vec: Vec<char> = Vec::new();
    while i >= 0 {
        vec.push(alphabet(index));
        i = i / ALPHABETS.len() as isize - 1;
    }
    return String::from_iter(vec.into_iter().rev())
}

pub fn index_at(str: &str) -> usize {
    let mut index = 0;
    for c in str.chars() {
        index = index * ALPHABETS.len() + ALPHABETS.find(c).unwrap();
    }
    return index
}

pub fn exp2xy(expr: &str) -> (usize, usize) {
    let mut x_vec:Vec<char> = Vec::new();
    let mut y_vec:Vec<char>  = Vec::new();

    for c in expr.chars() {
        if c.is_digit(10) {
            y_vec.push(c);
        } else {
            let uc = c.to_uppercase().next().unwrap();
            x_vec.push(uc);
        }
    }

    let x = index_at(&String::from_iter(x_vec.into_iter()));
    let y = String::from_iter(y_vec.into_iter()).parse::<usize>().unwrap();
    return (x, y - 1)
}

pub fn xy2expr(x: usize, y: usize) -> String {
    return format!("{}{}", string_at(x), y + 1)
}

pub fn expr2expr(expr: &str, xn: usize, yn: usize) -> String {
    let (x, y) = exp2xy(expr);
    return xy2expr(x + xn, y+ yn)
}