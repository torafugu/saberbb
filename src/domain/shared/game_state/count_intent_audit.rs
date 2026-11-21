use crate::domain::{
    random_provider::{RandomProvider, RealRng},
    shared::batting_personnel::BattingSelectionTrace,
    strategy::batting_strategy::{BattedBallIntent, BattingStrategy},
};
use std::cell::RefCell;

const EARLY_LINE_DRIVE_WEIGHT_MULTIPLIER: f64 = 1.5;

#[derive(Clone, Copy)]
pub struct CountIntentSwing {
    pub count_seq: u16,
    pub strikes: u8,
    pub intent: BattedBallIntent,
    pub batter_discretion: bool,
}

struct Audit {
    rng: Option<RealRng>,
    swings: Vec<CountIntentSwing>,
}

thread_local! {
    static AUDIT: RefCell<Option<Audit>> = const { RefCell::new(None) };
}

/// Record live swings, optionally drawing early-count intent from a separate random stream.
pub fn with_count_intent_audit<T>(
    policy_seed: Option<u64>,
    run: impl FnOnce() -> T,
) -> (T, Vec<CountIntentSwing>) {
    struct Restore(Option<Audit>);
    impl Drop for Restore {
        fn drop(&mut self) {
            AUDIT.replace(self.0.take());
        }
    }
    let _restore = Restore(AUDIT.replace(Some(Audit {
        rng: policy_seed.map(RealRng::from_seed),
        swings: Vec::new(),
    })));
    let result = run();
    (
        result,
        AUDIT.take().expect("count intent audit is active").swings,
    )
}

fn early_line_drive_probability(selection: &BattingSelectionTrace) -> Option<f64> {
    if selection.situation.strikes >= 2
        || selection.instruction.strategy != BattingStrategy::LetBatterDecide
    {
        return None;
    }
    let line = selection.preferences.line_drive_weight * EARLY_LINE_DRIVE_WEIGHT_MULTIPLIER;
    Some(line / (line + selection.preferences.contact_weight))
}

pub(super) fn select(count_seq: u16, selection: &mut BattingSelectionTrace) {
    AUDIT.with_borrow_mut(|slot| {
        if let Some(audit) = slot {
            if let Some(rng) = &mut audit.rng
                && let Some(probability) = early_line_drive_probability(selection)
            {
                selection.decision.selected_intent = if rng.random_bool(probability) {
                    BattedBallIntent::LineDrive
                } else {
                    BattedBallIntent::ContactFirst
                };
            }
            audit.swings.push(CountIntentSwing {
                count_seq,
                strikes: selection.situation.strikes,
                intent: selection.decision.selected_intent,
                batter_discretion: selection.instruction.strategy
                    == BattingStrategy::LetBatterDecide,
            });
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::batting_personnel::{
        BatterCharacter, BattingSituation, Manager, ManagerBattingPolicy, select_batting_intent,
    };

    #[test]
    fn early_weights_keep_batter_tendencies_and_explicit_instructions() {
        let situation = BattingSituation {
            balls: 0,
            strikes: 0,
            outs: 0,
            runners: [false; 3],
        };
        let trace = |character, situation, manager| {
            select_batting_intent(1, character, situation, manager).unwrap()
        };
        for (character, expected) in [
            (BatterCharacter::ContactOriented, 3.0 / 11.0),
            (BatterCharacter::Balanced, 0.6),
            (BatterCharacter::LineDriveOriented, 6.0 / 7.0),
        ] {
            let probability =
                early_line_drive_probability(&trace(character, situation, None)).unwrap();
            assert!((probability - expected).abs() < 1e-12);
        }
        assert_eq!(
            early_line_drive_probability(&trace(
                BatterCharacter::Balanced,
                BattingSituation {
                    strikes: 2,
                    ..situation
                },
                None
            )),
            None
        );
        let manager = Manager {
            id: 1,
            name: "Manager".into(),
            batting_policy: ManagerBattingPolicy::PutBallInPlay,
        };
        assert_eq!(
            early_line_drive_probability(&trace(
                BatterCharacter::LineDriveOriented,
                situation,
                Some(&manager)
            )),
            None
        );
    }
}
