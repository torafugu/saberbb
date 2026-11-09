//! Optional paired replay of the live contact-position timing scale.
use super::{
    ball::{BattedBall, OutboundResult, PitchedBall},
    game_stats::PlayerGameBattingMetrics,
    player::BatterInfo,
    stadium::Stadium,
};
use crate::domain::{
    random_provider::{FixedRng, RealRng},
    resolver::{
        batting_resolver::{
            BattedBallAngles, CONTACT_TIMING_POSITION_SCALE, SwingContactResult, SwingContactType,
            SwingExecutionError, calculate_batted_ball, calculate_launch_angles,
            calculate_launch_speed_with_power, contact_normal_hla_deg,
            evaluate_swing_contact_with_timing_scale, wrap_hla_deg,
        },
        pitching_resolver::PitchDisplacement,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::{cell::RefCell, collections::BTreeMap};

const COMPARISON_SCALE: f64 = 1.0;
const HARD_HIT_MS: f64 = 95.0 * 0.44704;
const NEAR_LENGTH_M: f64 = 0.05;
const NEAR_THICKNESS_M: f64 = 0.025;

#[derive(Default, Serialize)]
struct OutcomeCounts {
    misses: u64,
    contacts: u64,
    near_center_contacts: u64,
    hard_hits: u64,
    geometric_fouls: u64,
    fair_flights: u64,
    fair_hard_hits: u64,
    flight_errors: u64,
}

impl OutcomeCounts {
    fn add(
        &mut self,
        contact: &SwingContactResult,
        speed: f64,
        flight: Option<&BattedBall>,
    ) -> &'static str {
        if contact.adjusted_contact_type == SwingContactType::SwungAndMiss {
            self.misses += 1;
            return "miss";
        }
        self.contacts += 1;
        self.near_center_contacts += u64::from(
            contact.length_offset_m < NEAR_LENGTH_M
                && contact.thickness_offset_m < NEAR_THICKNESS_M,
        );
        let hard = speed >= HARD_HIT_MS;
        self.hard_hits += u64::from(hard);
        let Some(ball) = flight else {
            self.flight_errors += 1;
            return "flight_error";
        };
        if ball.outbound_result == OutboundResult::Foul
            || (ball.outbound_result == OutboundResult::InField && ball.is_foul())
        {
            self.geometric_fouls += 1;
            "foul"
        } else {
            self.fair_flights += 1;
            self.fair_hard_hits += u64::from(hard);
            "fair"
        }
    }

    fn json(&self, swings: u64) -> Value {
        let mut report = serde_json::to_value(self).unwrap();
        for (key, count) in [
            ("miss_rate_per_swing", self.misses),
            ("near_center_rate_per_swing", self.near_center_contacts),
            ("hard_hit_rate_per_swing", self.hard_hits),
            ("geometric_foul_rate_per_swing", self.geometric_fouls),
            ("fair_hard_hit_rate_per_swing", self.fair_hard_hits),
        ] {
            report[key] = json!((swings > 0).then(|| count as f64 / swings as f64));
        }
        report
    }
}

#[derive(Default)]
pub struct TimingScaleAudit {
    swings: u64,
    baseline: OutcomeCounts,
    comparison: OutcomeCounts,
    transitions: BTreeMap<String, u64>,
    comparison_errors: BTreeMap<String, u64>,
    new_contacts_with_sampled_direction: u64,
}

struct ActiveAudit {
    stats: TimingScaleAudit,
    game_seed: u64,
    swing_index: u64,
}

thread_local! {
    static ACTIVE: RefCell<Option<ActiveAudit>> = const { RefCell::new(None) };
}

impl TimingScaleAudit {
    /// Accumulate diagnostics for a synchronous game using its independent seed.
    pub fn observe_game<T>(&mut self, game_seed: u64, run: impl FnOnce() -> T) -> T {
        struct Restore<'a> {
            previous: Option<ActiveAudit>,
            target: &'a mut TimingScaleAudit,
        }
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                *self.target = ACTIVE.replace(self.previous.take()).unwrap().stats;
            }
        }

        let previous = ACTIVE.replace(Some(ActiveAudit {
            stats: std::mem::take(self),
            game_seed,
            swing_index: 0,
        }));
        let _restore = Restore {
            previous,
            target: self,
        };
        run()
    }

    pub fn json(&self) -> Value {
        json!({
            "source": "paired_timing_position_scale_v1",
            "baseline_scale": CONTACT_TIMING_POSITION_SCALE,
            "comparison_scale": COMPARISON_SCALE,
            "hard_hit_threshold_ms": HARD_HIT_MS,
            "near_center_length_lt_m": NEAR_LENGTH_M,
            "near_center_thickness_lt_m": NEAR_THICKNESS_M,
            "swings": self.swings,
            "baseline": self.baseline.json(self.swings),
            "comparison": self.comparison.json(self.swings),
            "baseline_to_comparison": self.transitions,
            "comparison_flight_errors": self.comparison_errors,
            "new_contacts_with_sampled_direction": self.new_contacts_with_sampled_direction,
        })
    }
}

/// Capture each realized swing before any fielding decisions. The supplied
/// baseline flight and its metrics are the actual live calculation.
pub(crate) fn record(
    batter: &BatterInfo,
    pitch: PitchedBall,
    live: &SwingContactResult,
    stadium: &Stadium,
    baseline: Option<(&BattedBall, &PlayerGameBattingMetrics)>,
) {
    ACTIVE.with_borrow_mut(|active| {
        let Some(active) = active else { return };
        active.swing_index += 1;
        let stats = &mut active.stats;
        stats.swings += 1;
        let before = stats.baseline.add(
            live,
            baseline.map_or(0.0, |(ball, _)| ball.launch_speed),
            baseline.map(|(ball, _)| ball),
        );

        let bat_angle = live
            .actual_bat_angle_deg
            .expect("live swing has a bat angle");
        let comparison = evaluate_swing_contact_with_timing_scale(
            // Tip-related quality labels do not feed EV, angles, spin or the
            // geometric contact/miss decision. No gameplay RNG is consumed.
            &mut FixedRng::new(0.5),
            batter,
            &PitchDisplacement {
                horizontal_offset_m: live.adapted_horizontal_offset_m,
                vertical_offset_m: live.adapted_vertical_offset_m,
                timing_offset_sec: live.timing_offset_sec,
                ..Default::default()
            },
            &SwingExecutionError {
                additional_x_m: live.swing_error_x_m,
                additional_z_m: live.swing_error_z_m,
                actual_bat_angle_deg: bat_angle,
                actual_attack_angle_deg: live.attack_angle_deg,
                ..Default::default()
            },
            COMPARISON_SCALE,
        );

        let after = if comparison.adjusted_contact_type == SwingContactType::SwungAndMiss {
            stats.comparison.add(&comparison, 0.0, None)
        } else {
            let angles = if let Some((ball, metrics)) = baseline {
                // Preserve the realized face direction. The only angular
                // difference is the contact-normal term at the new offset.
                let normal = |c: &SwingContactResult| {
                    contact_normal_hla_deg(c.signed_thickness_offset_m.unwrap(), bat_angle)
                };
                let rebound_delta = normal(&comparison) - normal(live);
                Ok(BattedBallAngles {
                    vla_deg: ball.launch_angle,
                    hla_deg: wrap_hla_deg(
                        metrics.final_hla_deg
                            + rebound_delta * batter.batting_side.handedness_sign(),
                    ),
                    approach_variation_deg: metrics.approach_variation_deg,
                    pull_bias_deg: metrics.pull_bias_deg,
                    timing_angle_error_deg: metrics.timing_angle_error,
                    angular_velocity_rad_s: metrics.angular_velocity,
                })
            } else {
                stats.new_contacts_with_sampled_direction += 1;
                let seed = active
                    .game_seed
                    .wrapping_add(active.swing_index.wrapping_mul(0x9e3779b97f4a7c15));
                calculate_launch_angles(&mut RealRng::from_seed(seed), &comparison, batter)
            };
            match angles.and_then(|angles| {
                calculate_batted_ball(batter, pitch, &comparison, &angles, stadium)
            }) {
                Ok(ball) => stats
                    .comparison
                    .add(&comparison, ball.launch_speed, Some(&ball)),
                Err(error) => {
                    *stats
                        .comparison_errors
                        .entry(error.to_string())
                        .or_default() += 1;
                    let speed = calculate_launch_speed_with_power(
                        &comparison,
                        pitch.speed,
                        batter.swing_speed,
                        batter.swing_power,
                    );
                    stats.comparison.add(&comparison, speed, None)
                }
            }
        };
        *stats
            .transitions
            .entry(format!("{before}->{after}"))
            .or_default() += 1;
    });
}
