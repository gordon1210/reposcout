// Intentional near/exact duplicate of `accumulate` in math.rs
// used to exercise clone detection.
pub fn summate(values: &[f64]) -> f64 {
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
