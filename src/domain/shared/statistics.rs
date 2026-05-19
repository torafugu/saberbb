use super::player::Player;

pub struct BattingStats {
    pub batter: Player,
    pub ab: i16,
    pub single: i16,
    pub double: i16,
    pub triple: i16,
    pub homerun: i16,
    pub ba: f32,
    pub rbi: f32,
}
