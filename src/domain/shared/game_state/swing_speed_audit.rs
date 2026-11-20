use super::{
    GameError, GameState, launch_swing, pre_trial_exit_speed_ms, vertical_contact_policy_offset_m,
};
use crate::domain::{
    resolver::{
        batting_resolver::{SwingExecutionError, evaluate_swing_contact},
        pitching_resolver::PitchDisplacement,
    },
    shared::{
        ball::{OutboundResult, PitchedBall},
        game::BattingResult,
        player::BatterInfo,
    },
};
use std::cell::RefCell;

#[derive(Clone, Copy)]
pub struct SwingSpeedPlay {
    pub outcome: Option<BattingResult>,
    pub flight: Option<(OutboundResult, f64, f64)>,
}

struct Audit {
    batter_id: i64,
    alternatives: Vec<f64>,
    swings: Vec<(u16, Vec<SwingSpeedPlay>)>,
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
) -> (T, Vec<(u16, Vec<SwingSpeedPlay>)>) {
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
            let pitcher_id = self.current_pitcher()?.id;
            let mut arms = Vec::with_capacity(audit.alternatives.len() + 1);
            for speed in
                std::iter::once(batter.swing_speed).chain(audit.alternatives.iter().copied())
            {
                let alternative = BatterInfo {
                    swing_speed: speed,
                    ..*batter
                };
                let mut rng = self.rng.fork().ok_or(GameError::PairedContactRng)?;
                let mut error = *swing_error;
                let baseline_z_m = displacement.vertical_offset_m + error.additional_z_m;
                error.additional_z_m += vertical_contact_policy_offset_m(baseline_z_m, || {
                    pre_trial_exit_speed_ms(&alternative, pitch.speed, displacement, &error)
                });
                let contact =
                    evaluate_swing_contact(rng.as_mut(), &alternative, displacement, &error);
                let flight =
                    launch_swing(rng.as_mut(), &alternative, pitch, &contact, &self.stadium)?;
                let replay = self.replay_swing(
                    pitcher_id,
                    active.id,
                    alternative.batting_side,
                    &contact,
                    flight,
                    rng,
                )?;
                arms.push(SwingSpeedPlay {
                    outcome: replay.batting_result,
                    flight: replay
                        .ball
                        .map(|ball| (ball.outbound_result, ball.launch_speed, ball.launch_angle)),
                });
            }
            audit.swings.push((self.count_seq, arms));
            Ok(())
        })
    }
}
