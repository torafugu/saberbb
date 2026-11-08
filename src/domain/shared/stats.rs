use super::player::Player;
use super::team::Team;
use validator::Validate;

#[derive(Debug, Validate)]
pub struct Standing {
    pub team: Team,
    pub games: u16,
    pub wins: u16,
    pub losses: u16,
    pub draws: u16,
    pub pct: f32,
    pub gb: f32,
    pub r: u32,
    pub ra: u32,
    pub remaining: u32,
    pub homeruns: u32,
    pub steals: u32,
    pub ba: Option<f64>,
    pub era: Option<f64>,
    pub errors: u32,
}

#[derive(Debug, Validate)]
pub struct BattingStats {
    pub batter: Player,
    pub ab: u16,
    pub single: u16,
    pub double: u16,
    pub triple: u16,
    pub homerun: u16,
    pub ba: f32,
    pub rbi: f32,
}

#[derive(Debug, Validate)]
pub struct PitchingStats {
    pub batter: Player,
    pub games: u16,
    pub innings: u16,
    pub wins: u16,
    pub losses: u16,
    pub saves: u16,
    pub holds: u16,
    pub era: f32,
    pub so: u16,
    pub bb: u16,
}
