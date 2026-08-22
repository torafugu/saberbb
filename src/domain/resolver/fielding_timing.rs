use super::fielding_config::{
    BALL_FLIGHT_SPEED_CONTINUE_DISTANCE, CUTOFF_DISTANCE_COEFFICIENT, TOUCH_PENALTY_TIME,
};
use crate::domain::shared::game::BASE_DISTANCE;
use crate::domain::shared::game_state::{ActiveFielder, GameError};
use crate::domain::shared::player::Position;
use crate::domain::shared::stadium::Base;
use crate::domain::util::{PolarPosition, calculate_polar_distance};
use std::f64::consts::SQRT_2;

#[derive(Debug, Clone)]
pub struct DefenseTimeResult {
    pub defense_time: f64,
    pub final_fielder_position: Position,
}

// TODO: case of final fielder failed to catch
#[derive(Debug, Clone, Copy)]
pub struct DefenseTimeCalculator {
    ball_flight_speed_continue_distance: f64,
    cutoff_distance_coefficient: f64,
    touch_penalty_time: f64,
}

impl Default for DefenseTimeCalculator {
    fn default() -> Self {
        Self {
            ball_flight_speed_continue_distance: BALL_FLIGHT_SPEED_CONTINUE_DISTANCE,
            cutoff_distance_coefficient: CUTOFF_DISTANCE_COEFFICIENT,
            touch_penalty_time: TOUCH_PENALTY_TIME,
        }
    }
}

impl DefenseTimeCalculator {
    pub fn ball_flight_time(&self, distance: f64, initial_throw_speed: f64) -> f64 {
        if distance <= self.ball_flight_speed_continue_distance {
            return distance / initial_throw_speed;
        }

        // Beyond 30m, add a mild delay proportional to distance squared (air resistance penalty).
        let base_time = distance / initial_throw_speed;
        let penalty_factor =
            1.0 + (distance - self.ball_flight_speed_continue_distance).powi(2) * 0.0001;

        base_time * penalty_factor
    }

    pub fn direct_throw_time(
        &self,
        thrower: &ActiveFielder,
        from: &PolarPosition,
        to: &PolarPosition,
    ) -> f64 {
        let distance = calculate_polar_distance(from, to);
        thrower.info.prep_time + self.ball_flight_time(distance, thrower.info.throw_speed)
    }

    pub fn run_to_base_time(
        &self,
        fielder: &ActiveFielder,
        from: &PolarPosition,
        to: &PolarPosition,
    ) -> f64 {
        let distance = calculate_polar_distance(from, to);
        distance / fielder.info.running_speed
    }

    pub fn direct_play_time(
        &self,
        thrower: &ActiveFielder,
        from: &PolarPosition,
        target_base: Base,
        requires_touch_penalty: bool,
    ) -> f64 {
        let base_pos = target_base.polar_position();
        let throw_time = self.direct_throw_time(thrower, from, &base_pos);

        if requires_touch_penalty {
            throw_time + self.touch_penalty_time
        } else {
            throw_time
        }
    }

    pub fn infield_play_time(
        &self,
        thrower: &ActiveFielder,
        ball_pos: &PolarPosition,
        target_base: Base,
        is_force_play: bool,
        final_fielder_position: Position,
    ) -> DefenseTimeResult {
        let base_pos = target_base.polar_position();
        let throw_time = self.direct_play_time(thrower, ball_pos, target_base, !is_force_play);

        if is_force_play {
            let run_time = self.run_to_base_time(thrower, ball_pos, &base_pos);

            if run_time < throw_time {
                return DefenseTimeResult {
                    defense_time: run_time,
                    final_fielder_position: thrower.position,
                };
            }
        }

        DefenseTimeResult {
            defense_time: throw_time,
            final_fielder_position,
        }
    }

    pub fn cutoff_position(&self, catch_pos: &PolarPosition, target_base: Base) -> PolarPosition {
        let base_pos = target_base.polar_position();
        let cutoff_distance = base_pos.distance
            + (catch_pos.distance - base_pos.distance) * self.cutoff_distance_coefficient;

        PolarPosition::new(cutoff_distance, catch_pos.angle)
    }

    pub fn relay_throw_time(
        &self,
        thrower: &ActiveFielder,
        catch_pos: &PolarPosition,
        target_base: Base,
        cutoff_fielder: &ActiveFielder,
    ) -> f64 {
        let base_pos = target_base.polar_position();
        let cutoff_pos = self.cutoff_position(catch_pos, target_base);

        let first_throw = self.direct_throw_time(thrower, catch_pos, &cutoff_pos);
        let second_flight_distance = calculate_polar_distance(&cutoff_pos, &base_pos);
        let second_throw = cutoff_fielder.info.prep_time
            + self.ball_flight_time(second_flight_distance, cutoff_fielder.info.throw_speed);

        first_throw + second_throw
    }

    pub fn best_outfield_throw_time(
        &self,
        thrower: &ActiveFielder,
        catch_pos: &PolarPosition,
        target_base: Base,
        requires_touch_penalty: bool,
        cutoff_fielder: Option<&ActiveFielder>,
    ) -> f64 {
        let direct_time =
            self.direct_play_time(thrower, catch_pos, target_base, requires_touch_penalty);

        if let Some(cutoff_fielder) = cutoff_fielder {
            let relay_time = self.relay_throw_time(thrower, catch_pos, target_base, cutoff_fielder);

            relay_time.min(direct_time)
        } else {
            direct_time
        }
    }

    pub fn multi_play_throw_time(
        &self,
        thrower: &ActiveFielder,
        from_base: Base,
        to_base: Base,
    ) -> Result<f64, GameError> {
        let distance = calculate_base_distance(from_base, to_base)?;

        Ok(thrower.info.prep_time + self.ball_flight_time(distance, thrower.info.throw_speed))
    }
}

pub fn calculate_base_distance(from_base: Base, to_base: Base) -> Result<f64, GameError> {
    let distance = match from_base {
        Base::First => match to_base {
            Base::First => {
                return Err(GameError::SameTargetBase);
            }
            Base::Second => BASE_DISTANCE,
            Base::Third => BASE_DISTANCE * SQRT_2,
            Base::Home => BASE_DISTANCE,
        },
        Base::Second => match to_base {
            Base::First => BASE_DISTANCE,
            Base::Second => {
                return Err(GameError::SameTargetBase);
            }
            Base::Third => BASE_DISTANCE,
            Base::Home => BASE_DISTANCE * SQRT_2,
        },
        Base::Third => match to_base {
            Base::First => BASE_DISTANCE * SQRT_2,
            Base::Second => BASE_DISTANCE,
            Base::Third => {
                return Err(GameError::SameTargetBase);
            }
            Base::Home => BASE_DISTANCE,
        },
        Base::Home => match to_base {
            Base::First => BASE_DISTANCE,
            Base::Second => BASE_DISTANCE * SQRT_2,
            Base::Third => BASE_DISTANCE,
            Base::Home => {
                return Err(GameError::SameTargetBase);
            }
        },
    };
    Ok(distance)
}
