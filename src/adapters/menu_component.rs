#[derive(Debug, Clone)]
pub struct MenuItem<T> {
    pub label: String,
    pub value: T,
}

impl<T> std::fmt::Display for MenuItem<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label)
    }
}
