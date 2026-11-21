use super::{ActiveFielder, DEFAULT_LINE_DRIVE_LIFT_OFFSET_MM, GameError, GameState, SwingReplay};
use crate::domain::{
    resolver::{
        batting_resolver::SwingExecutionError, fielding_physics::FieldPlayPlayResult,
        pitching_resolver::PitchDisplacement,
    },
    shared::{
        ball::{BattedBall, PitchedBall},
        infield_opportunity_audit::{IntentAimInfield, intent_aim_infield},
        player::BatterInfo,
    },
    strategy::batting_strategy::BattedBallIntent,
};
use serde::Serialize;
use std::cell::{Cell, RefCell};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct IntentAimOffsets {
    pub contact_first_mm: i32,
    pub line_drive_mm: i32,
}

impl Default for IntentAimOffsets {
    fn default() -> Self {
        Self {
            contact_first_mm: 0,
            line_drive_mm: i32::from(DEFAULT_LINE_DRIVE_LIFT_OFFSET_MM),
        }
    }
}

impl IntentAimOffsets {
    pub(super) fn offset_m(self, intent: BattedBallIntent) -> f64 {
        let mm = match intent {
            BattedBallIntent::ContactFirst => self.contact_first_mm,
            BattedBallIntent::LineDrive => self.line_drive_mm,
        };
        f64::from(mm) / 1000.0
    }
}

pub struct IntentAimSwing {
    pub count_seq: u16,
    pub intent: BattedBallIntent,
    pub plays: Vec<IntentAimPlay>,
}

#[derive(Clone, Copy, Debug)]
pub struct IntentAimPlay {
    pub batting: SwingReplay,
    pub infield: Option<IntentAimInfield>,
}

#[derive(Clone, Copy)]
enum Trial {
    Aim(IntentAimOffsets),
    ContactFirstSpeed(f64),
    LengthLoss(f64),
}

impl Trial {
    fn inputs(self, intent: BattedBallIntent) -> (f64, f64, Option<f64>) {
        match self {
            Self::Aim(offsets) => (offsets.offset_m(intent), 1.0, None),
            Self::ContactFirstSpeed(factor) => (
                super::intent_aim_offset_m(intent),
                if intent == BattedBallIntent::ContactFirst {
                    factor
                } else {
                    1.0
                },
                None,
            ),
            Self::LengthLoss(loss) => (super::intent_aim_offset_m(intent), 1.0, Some(loss)),
        }
    }
}

struct Audit {
    arms: Vec<Trial>,
    swings: Vec<IntentAimSwing>,
}

thread_local! {
    static AUDIT: RefCell<Option<Audit>> = const { RefCell::new(None) };
    static INFIELD_CAPTURE: Cell<InfieldCapture> = const { Cell::new(InfieldCapture::Inactive) };
}

#[derive(Clone, Copy)]
enum InfieldCapture {
    Inactive,
    Pending,
    Observed(IntentAimInfield),
}

fn capture_infield(
    run: impl FnOnce() -> Result<SwingReplay, GameError>,
) -> Result<IntentAimPlay, GameError> {
    struct Restore(InfieldCapture);
    impl Drop for Restore {
        fn drop(&mut self) {
            INFIELD_CAPTURE.set(self.0);
        }
    }
    let _restore = Restore(INFIELD_CAPTURE.replace(InfieldCapture::Pending));
    let batting = run()?;
    let infield = match INFIELD_CAPTURE.get() {
        InfieldCapture::Observed(observation) => Some(observation),
        _ => None,
    };
    Ok(IntentAimPlay { batting, infield })
}

pub(super) fn record_infield(
    ball: &BattedBall,
    fielders: &[ActiveFielder],
    play: &FieldPlayPlayResult<'_>,
) {
    if matches!(INFIELD_CAPTURE.get(), InfieldCapture::Pending)
        && let Some(observation) = intent_aim_infield(ball, fielders, play)
    {
        INFIELD_CAPTURE.set(InfieldCapture::Observed(observation));
    }
}

/// Replay every live swing at the supplied absolute aim offsets in order.
pub fn with_intent_aim_audit<T>(
    arms: &[IntentAimOffsets],
    run: impl FnOnce() -> T,
) -> (T, Vec<IntentAimSwing>) {
    with_trials(arms.iter().copied().map(Trial::Aim).collect(), run)
}

/// Replay the same swing decisions with a speed multiplier on ContactFirst.
pub fn with_contact_first_speed_audit<T>(
    factors: &[f64],
    run: impl FnOnce() -> T,
) -> (T, Vec<IntentAimSwing>) {
    with_trials(
        factors
            .iter()
            .copied()
            .map(Trial::ContactFirstSpeed)
            .collect(),
        run,
    )
}

/// Replay both intents with the original contact geometry and alternate length attenuation.
pub fn with_contact_length_audit<T>(
    losses: &[f64],
    run: impl FnOnce() -> T,
) -> (T, Vec<IntentAimSwing>) {
    with_trials(losses.iter().copied().map(Trial::LengthLoss).collect(), run)
}

fn with_trials<T>(arms: Vec<Trial>, run: impl FnOnce() -> T) -> (T, Vec<IntentAimSwing>) {
    struct Restore(Option<Audit>);
    impl Drop for Restore {
        fn drop(&mut self) {
            AUDIT.replace(self.0.take());
        }
    }
    let _restore = Restore(AUDIT.replace(Some(Audit {
        arms,
        swings: Vec::new(),
    })));
    let result = run();
    (
        result,
        AUDIT.take().expect("intent aim audit is active").swings,
    )
}

impl GameState {
    pub(super) fn record_intent_aim_trials(
        &self,
        intent: BattedBallIntent,
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
            let mut unique = Vec::new();
            let mut plays = Vec::with_capacity(audit.arms.len());
            for arm in &audit.arms {
                let inputs @ (aim, factor, loss) = arm.inputs(intent);
                if let Some(&(_, play)) = unique.iter().find(|(seen, _)| *seen == inputs) {
                    plays.push(play);
                    continue;
                }
                let mut error = *swing_error;
                if aim != 0.0 {
                    error.additional_z_m += aim;
                }
                let trial_batter = BatterInfo {
                    swing_speed: batter.swing_speed * factor,
                    ..*batter
                };
                let play = capture_infield(|| {
                    self.replay_swing_variant(
                        active.id,
                        &trial_batter,
                        pitch,
                        displacement,
                        error,
                        loss,
                    )
                })?;
                unique.push((inputs, play));
                plays.push(play);
            }
            audit.swings.push(IntentAimSwing {
                count_seq: self.count_seq,
                intent,
                plays,
            });
            Ok(())
        })
    }
}
