use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::PitchedBall;
use crate::domain::shared::player::PitcherInfo;

pub fn calculate_pitched_ball(rng: &mut dyn RandomProvider, pitcher: &PitcherInfo) -> PitchedBall {
    PitchedBall {
        speed: 150.0,
        spin_rate: 2300.0,
        spin_angle: 0.0,
    }
}
