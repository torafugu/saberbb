use super::{GameError, GameState, pre_trial_exit_speed_ms, vertical_contact_policy_offset_m};
use crate::domain::{
    resolver::{
        batting_resolver::{
            SwingContactType, SwingExecutionError, calculate_batted_ball_with_length_loss,
            evaluate_swing_contact,
        },
        fielding_physics::CatchType,
        pitching_resolver::PitchDisplacement,
    },
    shared::{
        ball::{OutboundResult, PitchedBall},
        game::BattingResult,
        player::BatterInfo,
    },
};
use std::cell::RefCell;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SwingReplay {
    pub outcome: Option<BattingResult>,
    pub flight: Option<(OutboundResult, f64, f64)>,
    pub exit_speed_retention: Option<f64>,
    pub outfield_direct_out: bool,
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
        max_length_loss: Option<f64>,
    ) -> Result<SwingReplay, GameError> {
        let mut rng = self.rng.fork().ok_or(GameError::PairedContactRng)?;
        let baseline_z_m = displacement.vertical_offset_m + error.additional_z_m;
        error.additional_z_m += vertical_contact_policy_offset_m(baseline_z_m, || {
            pre_trial_exit_speed_ms(batter, pitch.speed, displacement, &error)
        });
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
        Ok(SwingReplay {
            outcome: replay.batting_result,
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
                    None,
                )?);
            }
            audit.swings.push((self.count_seq, arms));
            Ok(())
        })
    }
}
