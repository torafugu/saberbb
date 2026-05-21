use super::player::Player;

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
