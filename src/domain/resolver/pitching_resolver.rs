use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::PitchedBall;
use crate::domain::shared::player::PitcherInfo;
use crate::domain::shared::player::RL;
use crate::error::AppError;

pub fn create_pitch(
    rng: &mut dyn RandomProvider,
    pitcher: &PitcherInfo,
) -> Result<PitchedBall, AppError> {
    // delivery form
    let base_spin_angle = pitcher.base_spin_angle();

    let pitch_skill = pitcher.select_pitch_skill(rng)?;

    let final_spin_angle = if pitcher.throw_side == RL::Left {
        (base_spin_angle - pitch_skill.spin_angle + 360.0) % 360.0
    } else {
        (base_spin_angle + pitch_skill.spin_angle + 360.0) % 360.0
    };

    Ok(PitchedBall {
        speed: 150.0,
        spin_rate: 2300.0,
        spin_angle: final_spin_angle,
        spin_efficiency: 0.95,
        release_point: pitcher.calculate_release_point(),
    })
}
