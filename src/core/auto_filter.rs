use std::collections::{HashMap, HashSet};
use crate::core::cell_range::CellRange;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Filter {
    pub ci: usize,
    pub operator: String,
    pub value: Vec<String>,
}

impl Filter {
    pub fn new(ci: usize, operator: &str, value: Vec<String>) -> Self {
        Filter {
            ci,
            operator: operator.to_string(),
            value,
        }
    }

    pub fn includes(&self, v: &str) -> bool {
        if self.operator == "all" {
            return true;
        }
        if self.operator == "in" {
            return self.value.contains(&v.to_string());
        }
        false
    }

    pub fn set(&mut self, operator: &str, value: Vec<String>) {
        self.operator = operator.to_string();
        self.value = value;
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sort {
    pub ci: usize,
    pub order: String, // "asc" or "desc"
}

impl Sort {
    pub fn new(ci: usize, order: &str) -> Self {
        Sort {
            ci,
            order: order.to_string(),
        }
    }

    pub fn asc(&self) -> bool {
        self.order == "asc"
    }

    pub fn desc(&self) -> bool {
        self.order == "desc"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoFilter {
    #[serde(rename = "ref", skip_serializing_if = "Option::is_none")]
    pub ref_: Option<String>,
    pub filters: Vec<Filter>,
    pub sort: Option<Sort>,
}

impl Default for AutoFilter {
    fn default() -> Self {
        AutoFilter {
            ref_: None,
            filters: Vec::new(),
            sort: None,
        }
    }
}

impl AutoFilter {
    pub fn new() -> Self {
        AutoFilter::default()
    }

    pub fn active(&self) -> bool {
        self.ref_.is_some()
    }

    pub fn range(&self) -> Option<CellRange> {
        self.ref_.as_ref().and_then(|r| CellRange::from_str(r).ok())
    }

    pub fn hrange(&self) -> Option<CellRange> {
        self.range().map(|mut r| {
            r.eri = r.sri;
            r
        })
    }

    pub fn includes(&self, ri: usize, ci: usize) -> bool {
        if self.active() {
            if let Some(ref range) = self.range() {
                return range.includes(ri, ci);
            }
        }
        false
    }

    pub fn add_filter(&mut self, ci: usize, operator: &str, value: Vec<String>) {
        if let Some(filter) = self.filters.iter_mut().find(|f| f.ci == ci) {
            filter.set(operator, value);
        } else {
            self.filters.push(Filter::new(ci, operator, value));
        }
    }

    pub fn get_filter(&self, ci: usize) -> Option<&Filter> {
        self.filters.iter().find(|f| f.ci == ci)
    }

    pub fn get_filter_mut(&mut self, ci: usize) -> Option<&mut Filter> {
        self.filters.iter_mut().find(|f| f.ci == ci)
    }

    pub fn set_sort(&mut self, ci: usize, order: Option<&str>) {
        self.sort = order.map(|o| Sort::new(ci, o));
    }

    pub fn get_sort(&self, ci: usize) -> Option<&Sort> {
        self.sort.as_ref().filter(|s| s.ci == ci)
    }

    pub fn filtered_rows<F>(&self, get_cell: F) -> (HashSet<usize>, HashSet<usize>)
    where
        F: Fn(usize, usize) -> Option<String>,
    {
        let mut rset = HashSet::new();
        let mut fset = HashSet::new();

        if self.active() {
            if let Some(range) = self.range() {
                let sri = range.sri;
                let eri = range.eri;

                for ri in (sri + 1)..=eri {
                    let mut filtered = false;
                    for filter in &self.filters {
                        if let Some(text) = get_cell(ri, filter.ci) {
                            if !filter.includes(&text) {
                                rset.insert(ri);
                                filtered = true;
                                break;
                            }
                        }
                    }
                    if !filtered {
                        fset.insert(ri);
                    }
                }
            }
        }

        (rset, fset)
    }

    pub fn items<F>(&self, ci: usize, get_cell: F) -> HashMap<String, usize>
    where
        F: Fn(usize, usize) -> Option<String>,
    {
        let mut m = HashMap::new();

        if self.active() {
            if let Some(range) = self.range() {
                let sri = range.sri;
                let eri = range.eri;

                for ri in (sri + 1)..=eri {
                    if let Some(text) = get_cell(ri, ci) {
                        if text.trim().is_empty() {
                            *m.entry(String::new()).or_insert(0) += 1;
                        } else {
                            *m.entry(text).or_insert(0) += 1;
                        }
                    } else {
                        *m.entry(String::new()).or_insert(0) += 1;
                    }
                }
            }
        }

        m
    }

    pub fn clear(&mut self) {
        self.ref_ = None;
        self.filters.clear();
        self.sort = None;
    }

    pub fn get_data(&self) -> serde_json::Value {
        if self.active() {
            serde_json::json!({
                "ref": self.ref_,
                "filters": self.filters.iter().map(|f| {
                    serde_json::json!({
                        "ci": f.ci,
                        "operator": f.operator,
                        "value": f.value
                    })
                }).collect::<Vec<_>>(),
                "sort": self.sort.as_ref().map(|s| {
                    serde_json::json!({
                        "ci": s.ci,
                        "order": s.order
                    })
                })
            })
        } else {
            serde_json::json!({})
        }
    }

    pub fn set_data(&mut self, data: &serde_json::Value) {
        if let Some(ref_) = data.get("ref").and_then(|v| v.as_str()) {
            self.ref_ = Some(ref_.to_string());
        }
        if let Some(filters) = data.get("filters").and_then(|v| v.as_array()) {
            self.filters = filters.iter().filter_map(|f| {
                let ci = f.get("ci")?.as_u64()? as usize;
                let operator = f.get("operator")?.as_str()?.to_string();
                let value: Vec<String> = f.get("value")?.as_array()?
                    .iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect();
                Some(Filter::new(ci, &operator, value))
            }).collect();
        }
        if let Some(sort) = data.get("sort").and_then(|v| v.as_object()) {
            if let (Some(ci), Some(order)) = (
                sort.get("ci").and_then(|v| v.as_u64()).map(|v| v as usize),
                sort.get("order").and_then(|v| v.as_str())
            ) {
                self.sort = Some(Sort::new(ci, order));
            }
        }
    }
}