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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Standing {
    pub team: Team,
    pub games: i16,
    pub wins: i16,
    pub losses: i16,
    pub draws: i16,
    pub pct: f32,
    pub gb: f32,
    pub r: i16,
    pub ra: i16,
}
