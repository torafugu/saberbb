use super::types::InningType;

pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

pub fn next_tb(tb: InningType) -> InningType {
    if matches!(tb, InningType::BOTTOM) {
        InningType::TOP
    } else {
        InningType::BOTTOM
    }
}
