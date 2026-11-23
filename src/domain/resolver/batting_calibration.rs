use crate::domain::util::sigmoid;
use std::cell::Cell;

pub const BAT_CONTROL_CORRECTION_MIDPOINT: f64 = 0.5;
pub const BATTING_EYE_TIMING_MIDPOINT: f64 = 0.5;

thread_local! {
    static CALIBRATED: Cell<bool> = const { Cell::new(false) };
}

/// Enable the regular-game batting response coefficients for synchronous play.
/// Independent diagnostics use the original model unless they opt in.
pub fn with_calibrated_batting<T>(run: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            CALIBRATED.set(self.0);
        }
    }
    let _restore = Restore(CALIBRATED.replace(true));
    run()
}

pub(super) fn uncorrected_fraction(ability: f64, calibrated_midpoint: f64) -> f64 {
    let midpoint = if CALIBRATED.get() {
        calibrated_midpoint
    } else {
        0.0
    };
    1.0 - sigmoid(ability - midpoint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recalibrates_only_the_scoped_response() {
        let ability = 0.5;
        let original = uncorrected_fraction(ability, BAT_CONTROL_CORRECTION_MIDPOINT);
        assert!(original < 0.5);
        with_calibrated_batting(|| {
            assert_eq!(
                uncorrected_fraction(ability, BAT_CONTROL_CORRECTION_MIDPOINT),
                0.5
            );
            with_calibrated_batting(|| {
                assert_eq!(
                    uncorrected_fraction(ability, BATTING_EYE_TIMING_MIDPOINT),
                    0.5
                )
            });
            assert_eq!(
                uncorrected_fraction(ability, BATTING_EYE_TIMING_MIDPOINT),
                0.5
            );
        });
        assert_eq!(
            uncorrected_fraction(ability, BAT_CONTROL_CORRECTION_MIDPOINT),
            original
        );
    }
}
