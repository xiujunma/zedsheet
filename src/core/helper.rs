use std::collections::HashMap;

pub fn clone_deep<T: serde::Serialize + for<'de> serde::Deserialize<'de>>(obj: &T) -> T {
    let json = serde_json::to_string(obj).unwrap();
    serde_json::from_str(&json).unwrap()
}

pub fn merge<T: Clone + serde::Serialize + serde::de::DeserializeOwned>(target: &mut T, sources: &[T]) {
    let json = serde_json::to_string(target).unwrap();
    let mut map: serde_json::Value = serde_json::from_str(&json).unwrap();

    for source in sources {
        let source_json = serde_json::to_value(source).unwrap();
        if let serde_json::Value::Object(source_map) = source_json {
            if let serde_json::Value::Object(target_map) = &mut map {
                for (key, value) in source_map {
                    target_map.insert(key, value);
                }
            }
        }
    }

    if let Ok(result) = serde_json::from_value(map.clone()) {
        *target = result;
    }
}

pub fn equals<T: serde::Serialize>(obj1: &T, obj2: &T) -> bool {
    let json1 = serde_json::to_string(obj1).unwrap();
    let json2 = serde_json::to_string(obj2).unwrap();
    json1 == json2
}

pub fn range_sum(min: usize, max: usize, getv: impl Fn(usize) -> f64) -> f64 {
    (min..max).map(getv).sum()
}

pub fn range_reduce_if(
    min: usize,
    max: usize,
    inits: f64,
    initv: f64,
    ifv: f64,
    getv: impl Fn(usize) -> f64,
) -> (usize, f64, f64) {
    let mut s = inits;
    let mut v = initv;
    let mut i = min;
    while i < max {
        if s > ifv {
            break;
        }
        v = getv(i);
        s += v;
        i += 1;
    }
    (i, s - v, v)
}

pub fn number_calc(type_: &str, a1: f64, a2: f64) -> String {
    if a1.is_nan() || a2.is_nan() {
        return format!("{}{}{}", a1, type_, a2);
    }
    let result = match type_ {
        "-" => a1 - a2,
        "+" => a1 + a2,
        "*" => a1 * a2,
        "/" => a1 / a2,
        _ => 0.0,
    };
    format!("{}", result)
}