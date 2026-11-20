use super::{DEFAULT_LINE_DRIVE_LIFT_OFFSET_MM, GameError, GameState, SwingReplay};
use crate::domain::{
    resolver::{batting_resolver::SwingExecutionError, pitching_resolver::PitchDisplacement},
    shared::{ball::PitchedBall, player::BatterInfo},
    strategy::batting_strategy::BattedBallIntent,
};
use serde::Serialize;
use std::cell::RefCell;

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
    pub plays: Vec<SwingReplay>,
}

struct Audit {
    arms: Vec<IntentAimOffsets>,
    swings: Vec<IntentAimSwing>,
}

thread_local! {
    static AUDIT: RefCell<Option<Audit>> = const { RefCell::new(None) };
}

/// Replay every live swing at the supplied absolute aim offsets in order.
pub fn with_intent_aim_audit<T>(
    arms: &[IntentAimOffsets],
    run: impl FnOnce() -> T,
) -> (T, Vec<IntentAimSwing>) {
    struct Restore(Option<Audit>);
    impl Drop for Restore {
        fn drop(&mut self) {
            AUDIT.replace(self.0.take());
        }
    }
    let _restore = Restore(AUDIT.replace(Some(Audit {
        arms: arms.to_vec(),
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
            for offsets in &audit.arms {
                let aim = offsets.offset_m(intent);
                if let Some(&(_, play)) = unique.iter().find(|(seen, _)| *seen == aim) {
                    plays.push(play);
                    continue;
                }
                let mut error = *swing_error;
                if aim != 0.0 {
                    error.additional_z_m += aim;
                }
                let play =
                    self.replay_swing_variant(active.id, batter, pitch, displacement, error)?;
                unique.push((aim, play));
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
