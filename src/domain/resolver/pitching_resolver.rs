use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::PitchedBall;
use crate::domain::shared::player::PitcherInfo;

pub fn create_pitch(rng: &mut dyn RandomProvider, pitcher: &PitcherInfo) -> PitchedBall {
    // delivery form
    let base_spin_dir = pitcher.base_spin_angle();

    PitchedBall {
        speed: 150.0,
        spin_rate: 2300.0,
        spin_angle: 0.0,
        spin_efficiency: 0.95,
        release_point: pitcher.calculate_release_point(),
    }
}
