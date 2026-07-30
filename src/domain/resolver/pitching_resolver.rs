use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::PitchedBall;
use crate::domain::shared::player::PitcherInfo;
use crate::domain::shared::player::RL;
use crate::error::AppError;

const BASE_FOUR_SEAM_SPEED: f64 = 150.0;

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

    let speed = pitch_skill.velocity * rng.normal_factor_std_1percent();

    // Speed-based correction (slower pitches have lower spin rate)
    let speed_factor = speed / BASE_FOUR_SEAM_SPEED;

    let raw_spin_rate = pitch_skill.spin_rate * rng.normal_factor_std_1percent() * speed_factor;

    Ok(PitchedBall {
        speed: speed,
        spin_rate: raw_spin_rate,
        spin_angle: final_spin_angle,
        spin_efficiency: pitch_skill.spin_efficiency,
        release_point: pitcher.calculate_release_point(),
    })
}
