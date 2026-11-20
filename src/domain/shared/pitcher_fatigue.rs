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
