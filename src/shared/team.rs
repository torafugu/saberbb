use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct League {
    pub id: i16,
    pub name: Arc<str>,
    pub teams: Vec<Team>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Team {
    pub id: i16,
    pub name: Arc<str>,
}

impl Team {
    pub fn new(id: i16, name: &str) -> Team {
        Team {
            id: id,
            name: Arc::from(name),
        }
    }
}
