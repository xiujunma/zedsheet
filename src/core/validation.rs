use std::collections::HashMap;
use std::fmt::Debug;
use regex::Regex;
use crate::core::cell_range::CellRange;
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize)]
pub struct Validator {
    pub required: bool,
    pub value: String,
    pub type_: String,
    pub operator: String,
}

impl Validator {
    pub fn new(type_: &str, required: bool, value: &str, operator: &str) -> Self {
        Validator {
            required,
            value: value.to_string(),
            type_: type_.to_string(),
            operator: operator.to_string(),
        }
    }

    pub fn validate(&self, v: &str) -> (bool, String) {
        if self.required && v.trim().is_empty() {
            return (false, "Required field".to_string());
        }
        if v.trim().is_empty() {
            return (true, String::new());
        }

        let phone_regex = Regex::new(r"^[1-9]\d{10}$").unwrap();
        let email_regex = Regex::new(r"^\w+([-+.]\w+)*@\w+([-.]\w+)*\.\w+([-.]\w+)*$").unwrap();

        if self.type_ == "phone" && !phone_regex.is_match(v) {
            return (false, "Invalid phone format".to_string());
        }
        if self.type_ == "email" && !email_regex.is_match(v) {
            return (false, "Invalid email format".to_string());
        }

        if self.type_ == "list" {
            let values: Vec<&str> = self.value.split(',').collect();
            return if values.contains(&v) {
                (true, String::new())
            } else {
                (false, "Value not in list".to_string())
            };
        }

        if !self.operator.is_empty() {
            let parse_value = |s: &str| -> f64 {
                s.parse().unwrap_or(0.0)
            };

            let v1 = parse_value(v);
            let val = parse_value(&self.value);

            match self.operator.as_str() {
                "be" => {
                    let parts: Vec<&str> = self.value.split(',').collect();
                    if parts.len() == 2 {
                        let min = parse_value(parts[0]);
                        let max = parse_value(parts[1]);
                        return (v1 >= min && v1 <= max, format!("Between {} and {}", min, max));
                    }
                }
                "nbe" => {
                    let parts: Vec<&str> = self.value.split(',').collect();
                    if parts.len() == 2 {
                        let min = parse_value(parts[0]);
                        let max = parse_value(parts[1]);
                        return (v1 < min || v1 > max, format!("Not between {} and {}", min, max));
                    }
                }
                "eq" => return (v1 == val, format!("Must equal {}", self.value)),
                "neq" => return (v1 != val, format!("Must not equal {}", self.value)),
                "lt" => return (v1 < val, format!("Must be less than {}", self.value)),
                "lte" => return (v1 <= val, format!("Must be less than or equal to {}", self.value)),
                "gt" => return (v1 > val, format!("Must be greater than {}", self.value)),
                "gte" => return (v1 >= val, format!("Must be greater than or equal to {}", self.value)),
                _ => {}
            }
        }

        (true, String::new())
    }

    pub fn equals(&self, other: &Validator) -> bool {
        self.type_ == other.type_
            && self.required == other.required
            && self.operator == other.operator
            && self.value == other.value
    }
}

impl Clone for Validator {
    fn clone(&self) -> Self {
        Validator {
            required: self.required,
            value: self.value.clone(),
            type_: self.type_.clone(),
            operator: self.operator.clone(),
        }
    }
}

impl Debug for Validator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Validator")
            .field("required", &self.required)
            .field("value", &self.value)
            .field("type_", &self.type_)
            .field("operator", &self.operator)
            .finish()
    }
}

#[derive(Serialize, Deserialize)]
pub struct Validation {
    pub refs: Vec<String>,
    pub mode: String,
    pub validator: Validator,
}

impl Clone for Validation {
    fn clone(&self) -> Self {
        Validation {
            refs: self.refs.clone(),
            mode: self.mode.clone(),
            validator: self.validator.clone(),
        }
    }
}

impl Debug for Validation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Validation")
            .field("refs", &self.refs)
            .field("mode", &self.mode)
            .field("validator", &self.validator)
            .finish()
    }
}

impl Validation {
    pub fn new(mode: &str, refs: Vec<String>, validator: Validator) -> Self {
        Validation {
            mode: mode.to_string(),
            refs,
            validator,
        }
    }

    pub fn includes(&self, ri: usize, ci: usize) -> bool {
        for ref_ in &self.refs {
            if let Ok(cr) = CellRange::from_str(ref_) {
                if cr.includes(ri, ci) {
                    return true;
                }
            }
        }
        false
    }

    pub fn add_ref(&mut self, ref_: &str) {
        // Remove any existing refs that intersect with this new ref
        if let Ok(new_cr) = CellRange::from_str(ref_) {
            self.remove_ref(&new_cr);
        }
        self.refs.push(ref_.to_string());
    }

    pub fn remove_ref(&mut self, cell_range: &CellRange) {
        let mut new_refs = Vec::new();
        for ref_ in &self.refs {
            if let Ok(cr) = CellRange::from_str(ref_) {
                if !cr.intersects(cell_range) {
                    new_refs.push(ref_.clone());
                }
            } else {
                new_refs.push(ref_.clone());
            }
        }
        self.refs = new_refs;
    }
}

#[derive(Serialize, Deserialize)]
pub struct Validations {
    #[serde(rename = "_")]
    validations: Vec<Validation>,
    #[serde(skip)]
    errors: HashMap<String, String>,
}

impl Clone for Validations {
    fn clone(&self) -> Self {
        Validations {
            validations: self.validations.clone(),
            errors: HashMap::new(),
        }
    }
}

impl Debug for Validations {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Validations")
            .field("validations", &self.validations)
            .finish()
    }
}

impl Default for Validations {
    fn default() -> Self {
        Validations {
            validations: Vec::new(),
            errors: HashMap::new(),
        }
    }
}

impl Validations {
    pub fn new() -> Self {
        Validations::default()
    }

    pub fn get_error(&self, ri: usize, ci: usize) -> Option<&String> {
        self.errors.get(&format!("{}_{}", ri, ci))
    }

    pub fn validate(&mut self, ri: usize, ci: usize, text: &str) -> bool {
        if let Some(v) = self.get(ri, ci) {
            let key = format!("{}_{}", ri, ci);
            let (flag, message) = v.validator.validate(text);
            if !flag {
                self.errors.insert(key, message);
            } else {
                self.errors.remove(&key);
            }
        } else {
            self.errors.remove(&format!("{}_{}", ri, ci));
        }
        true
    }

    pub fn add(&mut self, mode: &str, ref_: &str, validator: Validator) {
        if let Some(v) = self.get_by_validator(&validator) {
            v.add_ref(ref_);
        } else {
            self.validations.push(Validation::new(mode, vec![ref_.to_string()], validator));
        }
    }

    pub fn get_by_validator(&mut self, validator: &Validator) -> Option<&mut Validation> {
        self.validations.iter_mut().find(|v| v.validator.equals(validator))
    }

    pub fn get(&self, ri: usize, ci: usize) -> Option<&Validation> {
        self.validations.iter().find(|v| v.includes(ri, ci))
    }

    pub fn remove(&mut self, range: &CellRange) {
        for v in &mut self.validations {
            v.remove_ref(range);
        }
    }

    pub fn set_data(&mut self, data: Vec<Validation>) {
        self.validations = data;
    }

    pub fn get_data(&self) -> Vec<Validation> {
        self.validations.iter().filter(|v| !v.refs.is_empty()).cloned().collect()
    }
}