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

pub struct PitchDisplacement {
    // NOTE: Horizontal offset (-1.0: shift left ~ +1.0: shift right)
    pub horizontal: f64,
    // NOTE: Vertical offset (-1.0: shift down ~ +1.0: shift up)
    pub vertical: f64,
}

/// Timing when the batter initiates the swing (e.g. 12m from the pitcher / approx. 0.15s before impact)
pub fn calculate_late_break_displacement(
    ball: &PitchedBall,
    flight_time: f64, // Total flight time
) -> PitchDisplacement {
    // Point at which the batter commits to the swing (t = 60% of total flight time elapsed)
    let decision_time = flight_time * 0.6;

    // 1. Movement at the decision point (batter's brain says "this is the trajectory")
    let pos_at_decision = 0.5 * ball.get_side_accel() * decision_time.powi(2);
    // Predict the position over home plate assuming a linear trajectory (linear extrapolation)
    let predicted_final_pos = pos_at_decision * (1.0 / 0.6);

    // 2. Actual movement at the arrival point (result of t² growth)
    let actual_final_pos = 0.5 * ball.get_side_accel() * flight_time.powi(2);

    // 3. Difference between predicted and actual landing point (the abrupt late break!)
    let late_break_amount = actual_final_pos - predicted_final_pos;

    PitchDisplacement {
        horizontal: late_break_amount,
        vertical: 0.0, // Vertical direction can be calculated similarly
    }
}
