//! Flight drag selection. Legacy is retained only for controlled diagnostics.
use serde::{Deserialize, Serialize};
use std::cell::Cell;
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FlightDragModel {
    Legacy,
    #[default]
    Published2017,
}
thread_local! {
    static MODEL: Cell<FlightDragModel> = const { Cell::new(FlightDragModel::Published2017) };
}
pub fn current_model() -> FlightDragModel {
    MODEL.get()
}
/// Synchronous diagnostic scope; restores the caller's model even on panic.
/// Spawned threads use the normal published default, not this override.
pub fn with_model<T>(model: FlightDragModel, f: impl FnOnce() -> T) -> T {
    struct Restore(FlightDragModel);
    impl Drop for Restore {
        fn drop(&mut self) {
            MODEL.set(self.0);
        }
    }
    let _restore = Restore(MODEL.replace(model));
    f()
}
pub const LEGACY_DRAG_PER_M: f64 = 0.0012;
pub const DENSITY_KG_M3: f64 = 1.194;
pub const MASS_KG: f64 = 5.125 * 0.028349523125;
pub const RADIUS_M: f64 = 9.125 * 0.0254 / (2.0 * std::f64::consts::PI);
/// Nathan (2017), Eq. 10. Total spin, including gyro spin, determines drag.
/// The model is measurement-fitted; these nominal constants are not league calibration.
pub fn drag_per_m(model: FlightDragModel, total_spin_rpm: f64) -> f64 {
    match model {
        FlightDragModel::Legacy => LEGACY_DRAG_PER_M,
        FlightDragModel::Published2017 => {
            let cd = 0.297 + 0.0292 * total_spin_rpm / 1000.0;
            0.5 * DENSITY_KG_M3 * std::f64::consts::PI * RADIUS_M.powi(2) / MASS_KG * cd
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_drag_and_legacy_control_have_distinct_units_and_spin_response() {
        assert!(
            (drag_per_m(FlightDragModel::Published2017, 2600.0) - 0.006550488590302722).abs()
                < 1e-12
        );
        assert_eq!(drag_per_m(FlightDragModel::Legacy, 2600.0), 0.0012);
        assert!(
            drag_per_m(FlightDragModel::Published2017, 4000.0)
                > drag_per_m(FlightDragModel::Published2017, 0.0)
        );
    }
    #[test]
    fn diagnostic_override_is_nested_panic_safe_and_thread_local() {
        assert_eq!(current_model(), FlightDragModel::Published2017);
        with_model(FlightDragModel::Legacy, || {
            assert_eq!(current_model(), FlightDragModel::Legacy);
            assert_eq!(
                std::thread::spawn(current_model).join().unwrap(),
                FlightDragModel::Published2017
            );
            let _ = std::panic::catch_unwind(|| {
                with_model(FlightDragModel::Published2017, || panic!("restore"))
            });
            assert_eq!(current_model(), FlightDragModel::Legacy);
        });
        assert_eq!(current_model(), FlightDragModel::Published2017);
    }
}
