use std::sync::Arc;

#[derive(Clone)]
pub struct Team {
    name: Arc<str>,
}

impl Team {
    pub fn new(name: &str) -> Team {
        Team {
            name: Arc::from(name),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }
}
