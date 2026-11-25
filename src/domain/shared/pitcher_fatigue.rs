use serde::{Deserialize, Serialize};

const BASE_REFERENCE_PITCHES: f64 = 100.0;

/// Accumulated workload, measured in pitch equivalents.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct PitcherFatigue {
    pub entering_load: f64,
    pub pitches: u32,
    pub reference_pitches: f64,
}

impl PitcherFatigue {
    /// The generated stamina scale is centered on zero.
    pub(crate) fn reference_pitches(stamina: f64) -> f64 {
        (BASE_REFERENCE_PITCHES + 15.0 * stamina).clamp(60.0, 140.0)
    }

    pub(crate) fn remaining_load(
        pitches: u32,
        days_elapsed: i64,
        appearance_reference: f64,
        current_reference: f64,
    ) -> f64 {
        // Baseline stamina halves fatigue each calendar day. The reference
        // recorded at the appearance controls both its cost and decay rate.
        let recovery_rate = appearance_reference / BASE_REFERENCE_PITCHES;
        let remaining_fraction = (-(days_elapsed as f64) * recovery_rate).exp2();
        let pitch_equivalents = f64::from(pitches) * (current_reference / appearance_reference);
        pitch_equivalents * remaining_fraction
    }

    pub fn entering_ratio(&self) -> f64 {
        self.entering_load / self.reference_pitches
    }

    pub fn leaving_ratio(&self) -> f64 {
        // One pitch consumes 1 / reference_pitches of a fresh pitcher's capacity.
        (self.entering_load + f64::from(self.pitches)) / self.reference_pitches
    }
}

/// Transient pitch execution penalties based on the load before the pitch.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PitchExecutionFatigue {
    severity: f64,
}

impl PitchExecutionFatigue {
    pub(crate) fn from_load_ratio(load_ratio: f64) -> Self {
        // A fresh pitcher has no penalty. Cap the effects if no reliever is available.
        let ramp = ((load_ratio - 0.5) / 0.5).clamp(0.0, 1.5);
        Self {
            severity: ramp * ramp,
        }
    }

    pub(crate) fn speed_loss_ms(self) -> f64 {
        self.severity
    }

    pub(crate) fn location_error_scale(self) -> f64 {
        1.0 + 0.25 * self.severity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_effect_uses_entering_load_and_pitches_already_thrown() {
        let fatigue = |entering_load, pitches, reference_pitches| {
            PitchExecutionFatigue::from_load_ratio(
                PitcherFatigue {
                    entering_load,
                    pitches,
                    reference_pitches,
                }
                .leaving_ratio(),
            )
        };
        let rested = fatigue(0.0, 50, 100.0);
        assert_eq!(rested.speed_loss_ms(), 0.0);
        assert_eq!(rested.location_error_scale(), 1.0);
        let midway = fatigue(0.0, 75, 100.0);
        assert_eq!(midway.speed_loss_ms(), 0.25);
        assert_eq!(midway.location_error_scale(), 1.0625);
        let tired = fatigue(25.0, 75, 100.0);
        assert_eq!(tired.speed_loss_ms(), 1.0);
        assert_eq!(tired.location_error_scale(), 1.25);
        assert!(fatigue(0.0, 75, 120.0).speed_loss_ms() < midway.speed_loss_ms());
        assert_eq!(fatigue(0.0, 160, 100.0).speed_loss_ms(), 2.25);
        assert_eq!(
            fatigue(75.0, 0, 100.0).speed_loss_ms(),
            midway.speed_loss_ms()
        );
    }
}
