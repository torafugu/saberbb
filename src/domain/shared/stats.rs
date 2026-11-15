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
    pub games: u32,
    pub pa: u32,
    pub ab: u32,
    pub single: u32,
    pub double: u32,
    pub triple: u32,
    pub homerun: u32,
    pub ba: f32,
    pub rbi: u32,
    pub runs: u32,
    pub so: u32,
    pub bb: u32,
    pub hbp: u32,
    pub sf: u32,
    pub sb: u32,
    pub cs: u32,
    pub gidp: u32,
    pub risp_hits: u32,
    pub risp_ab: u32,
    pub errors: u32,
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
