use super::{GameError, GameState, pre_trial_exit_speed_ms, vertical_contact_policy_offset_m};
use crate::domain::{
    resolver::{
        batting_resolver::{
            ContactProtectionTrial, SwingContactType, SwingExecutionError,
            apply_contact_protection_trial, calculate_batted_ball_with_length_loss,
            evaluate_swing_contact, replay_fence_flight,
        },
        fielding_physics::CatchType,
        pitching_resolver::PitchDisplacement,
    },
    shared::{
        ball::{BattedBall, OutboundResult, PitchedBall},
        game::BattingResult,
        player::BatterInfo,
        stadium::Stadium,
    },
};
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingReplay {
    pub outcome: Option<BattingResult>,
    pub contact_type: SwingContactType,
    pub flight: Option<(OutboundResult, f64, f64)>,
    pub launch_hla_deg: Option<f64>,
    pub contact_z_m: f64,
    pub launch_spin: Option<(f64, f64)>,
    pub exit_speed_retention: Option<f64>,
    pub length_offset_m: f64,
    pub thickness_offset_m: f64,
    pub outfield_direct_out: bool,
    pub double_fence: Option<DoubleFenceObservation>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DoubleFenceObservation {
    pub category: &'static str,
    pub airborne_wall_shortfall_m: Option<f64>,
    pub first_bounce_fence_gap_m: Option<f64>,
    pub pickup_fence_gap_m: Option<f64>,
}

fn observe_double_fence(
    ball: &BattedBall,
    hla_deg: f64,
    stadium: &Stadium,
    clean_fielding_time_sec: Option<f64>,
    pickup: Option<crate::domain::util::PolarPosition>,
) -> Result<DoubleFenceObservation, GameError> {
    let mut airborne_wall_shortfall_m = None;
    let category = if ball.outbound_result == OutboundResult::GroundRuleDouble {
        "ground_rule_double"
    } else if ball.fence_impact_time.is_none() {
        "no_projected_fence_contact"
    } else {
        let events = replay_fence_flight(ball, hla_deg, stadium, |_| {})?;
        match events.first() {
            Some(event) if clean_fielding_time_sec.is_some_and(|t| t < event.time_sec) => {
                "fielded_before_projected_wall"
            }
            Some(event) if event.bounce_count == 0 => {
                airborne_wall_shortfall_m = Some(-event.height_above_fence_m);
                "airborne_wall"
            }
            Some(_) => "wall_after_bounce",
            None => "no_projected_fence_contact",
        }
    };
    let fence_gap = |point: crate::domain::util::PolarPosition| {
        stadium
            .fence_distance_at_angle(point.angle)
            .map(|distance| distance - point.distance)
    };
    Ok(DoubleFenceObservation {
        category,
        airborne_wall_shortfall_m,
        first_bounce_fence_gap_m: ball.first_bounce_position.map(fence_gap).transpose()?,
        pickup_fence_gap_m: pickup.map(fence_gap).transpose()?,
    })
}

struct Audit {
    batter_id: i64,
    alternatives: Vec<f64>,
    swings: Vec<(u16, Vec<SwingReplay>)>,
}

thread_local! {
    static AUDIT: RefCell<Option<Audit>> = const { RefCell::new(None) };
}

/// Record each selected batter's realized swing and counterfactuals in order.
/// Trials fork the live RNG and discard all changes to the game state.
pub fn with_swing_speed_audit<T>(
    batter_id: i64,
    alternatives: &[f64],
    run: impl FnOnce() -> T,
) -> (T, Vec<(u16, Vec<SwingReplay>)>) {
    struct Restore(Option<Audit>);
    impl Drop for Restore {
        fn drop(&mut self) {
            AUDIT.replace(self.0.take());
        }
    }
    let _restore = Restore(AUDIT.replace(Some(Audit {
        batter_id,
        alternatives: alternatives.to_vec(),
        swings: Vec::new(),
    })));
    let result = run();
    let swings = AUDIT.take().expect("swing speed audit is active").swings;
    (result, swings)
}

impl GameState {
    /// Replay contact, flight, and the ruling from the live pre-contact RNG position.
    pub(super) fn replay_swing_variant(
        &self,
        batter_id: i64,
        batter: &BatterInfo,
        pitch: PitchedBall,
        displacement: &PitchDisplacement,
        mut error: SwingExecutionError,
        (max_length_loss, audit_doubles, protection): (
            Option<f64>,
            bool,
            Option<ContactProtectionTrial>,
        ),
    ) -> Result<SwingReplay, GameError> {
        let mut rng = self.rng.fork().ok_or(GameError::PairedContactRng)?;
        let baseline_z_m = displacement.vertical_offset_m + error.additional_z_m;
        error.additional_z_m += vertical_contact_policy_offset_m(baseline_z_m, || {
            pre_trial_exit_speed_ms(batter, pitch.speed, displacement, &error)
        });
        if let Some(trial) = protection {
            apply_contact_protection_trial(batter, displacement, &mut error, trial);
        }
        let contact = evaluate_swing_contact(rng.as_mut(), batter, displacement, &error);
        let (flight, exit_speed_retention) =
            if contact.adjusted_contact_type == SwingContactType::SwungAndMiss {
                (None, None)
            } else {
                let (ball, metrics, retention) = calculate_batted_ball_with_length_loss(
                    rng.as_mut(),
                    batter,
                    pitch,
                    &contact,
                    &self.stadium,
                    max_length_loss.unwrap_or(0.0),
                )?;
                (Some((ball, metrics)), max_length_loss.map(|_| retention))
            };
        let replay = self.replay_swing(
            self.current_pitcher()?.id,
            batter_id,
            batter.batting_side,
            &contact,
            flight,
            rng,
        )?;
        let double_fence = if audit_doubles
            && replay.batting_result == Some(BattingResult::Double)
            && let Some((ball, metrics)) = flight
        {
            let catch = replay.fielding.and_then(|f| f.catch_diagnostics);
            let pickup = catch.and_then(|d| d.pickup);
            let clean_fielding_time = catch
                .filter(|d| d.error_type.is_none())
                .map(|d| pickup.map_or(d.catch_time_sec, |p| p.pickup_time_sec));
            Some(observe_double_fence(
                &ball,
                metrics.final_hla_deg,
                &self.stadium,
                clean_fielding_time,
                pickup.map(|p| p.pickup_position),
            )?)
        } else {
            None
        };
        Ok(SwingReplay {
            outcome: replay.batting_result,
            contact_type: contact.adjusted_contact_type,
            double_fence,
            outfield_direct_out: replay.batting_result == Some(BattingResult::Out)
                && replay
                    .fielding
                    .and_then(|f| f.catch_diagnostics)
                    .is_some_and(|catch| {
                        catch.fielder_position.is_outfielder()
                            && catch.catch_type == CatchType::DirectFly
                            && catch.error_type.is_none()
                    }),
            exit_speed_retention,
            length_offset_m: contact.length_offset_m,
            thickness_offset_m: contact.thickness_offset_m,
            launch_hla_deg: flight.map(|(_, metrics)| metrics.final_hla_deg),
            contact_z_m: contact.offset_z_m,
            launch_spin: replay.ball.map(|ball| (ball.spin_rate, ball.spin_angle)),
            flight: replay
                .ball
                .map(|ball| (ball.outbound_result, ball.launch_speed, ball.launch_angle)),
        })
    }

    pub(super) fn record_swing_speed_trials(
        &self,
        batter: &BatterInfo,
        pitch: PitchedBall,
        displacement: &PitchDisplacement,
        swing_error: &SwingExecutionError,
    ) -> Result<(), GameError> {
        AUDIT.with_borrow_mut(|slot| {
            let Some(audit) = slot.as_mut() else {
                return Ok(());
            };
            let Some(active) = &self.inning_state.active_batter else {
                return Ok(());
            };
            if active.id != audit.batter_id {
                return Ok(());
            }
            let mut arms = Vec::with_capacity(audit.alternatives.len() + 1);
            for speed in
                std::iter::once(batter.swing_speed).chain(audit.alternatives.iter().copied())
            {
                let alternative = BatterInfo {
                    swing_speed: speed,
                    ..*batter
                };
                arms.push(self.replay_swing_variant(
                    active.id,
                    &alternative,
                    pitch,
                    displacement,
                    *swing_error,
                    (None, false, None),
                )?);
            }
            audit.swings.push((self.count_seq, arms));
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::resolver::batting_resolver::calculate_trajectory;

    #[test]
    fn distinguishes_airborne_wall_hit_from_earlier_fielding() {
        let stadium = Stadium::new(1, "Tall outfield fence".into(), 25.0, 30.0, 30.0);
        let ball = calculate_trajectory(40.0, 20.0, 0.0, 0.0, 0.0, &stadium).unwrap();
        let observation = observe_double_fence(&ball, 0.0, &stadium, None, None).unwrap();
        assert_eq!(observation.category, "airborne_wall");
        assert!(observation.airborne_wall_shortfall_m.unwrap() > 0.0);
        let recovered = observe_double_fence(&ball, 0.0, &stadium, Some(0.0), None).unwrap();
        assert_eq!(recovered.category, "fielded_before_projected_wall");
        assert!(recovered.airborne_wall_shortfall_m.is_none());
    }

    #[test]
    fn recognizes_ground_rule_double_after_a_bounce() {
        let stadium = Stadium::new(1, "Low outfield fence".into(), 20.0, 25.0, 0.0);
        let ball = calculate_trajectory(40.0, -10.0, 0.0, 0.0, 0.0, &stadium).unwrap();
        let observation = observe_double_fence(&ball, 0.0, &stadium, None, None).unwrap();
        assert_eq!(observation.category, "ground_rule_double");
        assert!(observation.first_bounce_fence_gap_m.unwrap() > 0.0);
    }
}
