//! An attribute-based prior, independent of opponents, fatigue and game results.
use super::pitching_strategy::{Margin, PitchCall, TargetZone};
use crate::domain::{
    random_provider::FixedRng,
    resolver::pitching_resolver::{
        calculate_ball_movement, calculate_flight_time, create_pitch_for_call,
    },
    shared::{
        ball::{MAGNUS_COEFF, PitchedBall},
        player::{PitchType, PitcherInfo},
        stadium::MOUND_DISTANCE,
    },
    util::{GRAVITY, sigmoid},
};

const REFERENCE_EXTENSION_M: f64 = 1.8;
const REFERENCE_SPIN_RPM: f64 = 2300.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PitcherAttributeRating {
    pub stuff: f64,
    pub command: f64,
    pub arsenal: f64,
}

/// Relative emphasis within the currently available attribute-based evaluation.
#[derive(Clone, Copy, Debug)]
pub struct PitcherAttributeWeights {
    pub stuff: f64,
    pub command: f64,
    pub arsenal: f64,
}

impl Default for PitcherAttributeWeights {
    fn default() -> Self {
        Self {
            stuff: 1.0,
            command: 1.0,
            arsenal: 1.0,
        }
    }
}

impl PitcherAttributeWeights {
    pub fn score(self, rating: PitcherAttributeRating) -> f64 {
        let weights = [self.stuff, self.command, self.arsenal];
        let total: f64 = weights.iter().sum();
        if !total.is_finite() || total <= 0.0 || weights.iter().any(|w| *w < 0.0) {
            return (rating.stuff + rating.command + rating.arsenal) / 3.0;
        }
        (self.stuff / total) * rating.stuff
            + (self.command / total) * rating.command
            + (self.arsenal / total) * rating.arsenal
    }
}

struct PitchProfile {
    ball: PitchedBall,
    horizontal_break: f64,
    vertical_break: f64,
    usage: f64,
}

impl PitcherAttributeRating {
    pub fn measure(pitcher: &PitcherInfo, base_four_seam_speed: f64) -> Option<Self> {
        if !base_four_seam_speed.is_finite()
            || base_four_seam_speed <= 0.0
            || !pitcher.control.is_finite()
        {
            return None;
        }

        let mut rng = FixedRng::new(0.5);
        let mut profiles = Vec::new();
        for pitch in pitcher.pitch_skill_distribution() {
            let call = PitchCall {
                pitch_type: pitch.name.pitch_type,
                target_zone: TargetZone::Center,
                margin: Margin::Wide,
            };
            let ball =
                create_pitch_for_call(&mut rng, pitcher, call, Some(1.0), base_four_seam_speed)
                    .ok()?;
            let movement = calculate_ball_movement(&ball);
            let vertical_break = movement.z_m + 0.5 * GRAVITY * ball.flight_time.powi(2);
            if !ball.flight_time.is_finite()
                || ball.flight_time <= 0.0
                || !movement.x_m.is_finite()
                || !vertical_break.is_finite()
                || !pitch.weight.is_finite()
            {
                return None;
            }
            profiles.push(PitchProfile {
                ball,
                horizontal_break: movement.x_m,
                vertical_break,
                usage: pitch.weight,
            });
        }

        let primary = profiles
            .iter()
            .filter(|p| {
                matches!(
                    p.ball.pitch_type,
                    PitchType::FourSeamFastball | PitchType::Cutter
                )
            })
            .max_by(|a, b| {
                a.usage
                    .total_cmp(&b.usage)
                    .then_with(|| a.ball.speed.total_cmp(&b.ball.speed))
            })
            .or_else(|| {
                profiles
                    .iter()
                    .max_by(|a, b| a.ball.speed.total_cmp(&b.ball.speed))
            })?;
        let reference_time =
            calculate_flight_time(base_four_seam_speed, MOUND_DISTANCE - REFERENCE_EXTENSION_M);
        let reference_break =
            0.5 * MAGNUS_COEFF * REFERENCE_SPIN_RPM * base_four_seam_speed * reference_time.powi(2);
        let pace = sigmoid((reference_time / primary.ball.flight_time - 1.0) / 0.05);
        let primary_break = primary.horizontal_break.hypot(primary.vertical_break);
        let movement = primary_break / (primary_break + reference_break);

        // Compare the other shapes and arrival times to this pitcher's own primary pitch.
        let arsenal = profiles
            .iter()
            .map(|pitch| {
                let shape_gap = (pitch.horizontal_break - primary.horizontal_break)
                    .hypot(pitch.vertical_break - primary.vertical_break)
                    / reference_break;
                let time_gap = (pitch.ball.flight_time - primary.ball.flight_time).abs()
                    / (0.10 * reference_time);
                let separation = shape_gap.hypot(time_gap);
                pitch.usage * separation / (1.0 + separation)
            })
            .sum::<f64>();

        let stuff = (pace + movement) / 2.0;
        (stuff.is_finite() && arsenal.is_finite()).then_some(Self {
            stuff,
            // The flight model's location spread is proportional to 1 - sigmoid(control).
            command: sigmoid(pitcher.control),
            arsenal,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::test_support::pitcher_info;

    #[test]
    fn ratings_use_physical_attributes_and_allow_different_preferences() {
        let mut fast = pitcher_info();
        fast.pitch_skills.truncate(1);
        fast.pitch_skills[0].velocity = 1.0;
        fast.pitch_skills[0].spin_rate = 1.0;
        fast.velocity = 44.0;
        fast.control = -1.0;
        let mut precise = fast.clone();
        precise.velocity = 39.0;
        precise.control = 1.0;

        let fast_rating = PitcherAttributeRating::measure(&fast, 40.833).unwrap();
        let precise_rating = PitcherAttributeRating::measure(&precise, 40.833).unwrap();
        let stuff_first = PitcherAttributeWeights {
            stuff: 1.0,
            command: 0.0,
            arsenal: 0.0,
        };
        let command_first = PitcherAttributeWeights {
            stuff: 0.0,
            command: 1.0,
            arsenal: 0.0,
        };
        assert!(stuff_first.score(fast_rating) > stuff_first.score(precise_rating));
        assert!(command_first.score(fast_rating) < command_first.score(precise_rating));

        fast.stamina += 2.0;
        assert_eq!(
            PitcherAttributeRating::measure(&fast, 40.833),
            Some(fast_rating)
        );
        let mut breaking = fast.pitch_skills[0];
        breaking.pitch_type = PitchType::Curveball;
        breaking.spin_angle = 150.0;
        breaking.velocity = 0.8;
        fast.pitch_skills.push(breaking);
        assert!(
            PitcherAttributeRating::measure(&fast, 40.833)
                .unwrap()
                .arsenal
                > fast_rating.arsenal
        );
    }
}
