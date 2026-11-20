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
    pub games: u32,
    pub outs: u32,
    pub wins: u32,
    pub losses: u32,
    pub saves: u32,
    pub holds: u32,
    pub so: u32,
    pub bb: u32,
    pub starts: u32,
    pub complete_games: u32,
    pub shutouts: u32,
    pub quality_starts: Option<u32>,
    pub relief_wins: u32,
    pub hits: u32,
    pub home_runs: u32,
    pub hbp: u32,
    pub wild_pitches: u32,
    pub runs: u32,
    pub earned_runs: Option<u32>,
    pub at_bats: u32,
}
