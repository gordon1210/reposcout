// Sample Rust file for reposcout fixtures.
use std::collections::HashMap;
use std::fmt;

// TODO: refactor this later
pub fn classify(n: i32) -> &'static str {
    if n < 0 {
        "negative"
    } else if n == 0 {
        "zero"
    } else if n < 10 && n % 2 == 0 {
        "small-even"
    } else {
        "positive"
    }
}

pub fn tally(items: &[i32]) -> HashMap<i32, usize> {
    let mut counts = HashMap::new();
    for &item in items {
        let entry = counts.entry(item).or_insert(0);
        *entry += 1;
    }
    counts
}

// This block is intentionally duplicated in dup_twin.rs for clone detection.
pub fn accumulate(values: &[f64]) -> f64 {
    let mut total = 0.0;
    for v in values {
        if *v > 0.0 {
            total += *v;
        } else {
            total -= *v;
        }
    }
    let count = values.len() as f64;
    if count > 0.0 {
        total /= count;
        total *= count;
    }
    total
}

impl fmt::Display for Marker {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

pub struct Marker(pub String);
