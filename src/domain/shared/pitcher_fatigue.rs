use serde::{Deserialize, Serialize};

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
        (100.0 + 15.0 * stamina).clamp(60.0, 140.0)
    }

    pub(crate) fn recovered_load(pitches: u32, days_elapsed: i64) -> f64 {
        // One game-calendar day halves the residual load.
        f64::from(pitches) * (-days_elapsed as f64).exp2()
    }

    pub fn entering_ratio(&self) -> f64 {
        self.entering_load / self.reference_pitches
    }

    pub fn leaving_ratio(&self) -> f64 {
        (self.entering_load + f64::from(self.pitches)) / self.reference_pitches
    }
}
