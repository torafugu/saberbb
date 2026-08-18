pub const DEEP_OUTFIELD_DISTANCE: f64 = 90.0;
pub const SHALLOW_INFIELD_DISTANCE: f64 = 25.0;
pub const CUTOFF_NEEDED_DISTANCE_FOR_RUNNER_ON_THIRD: f64 = 80.0;
pub const CUTOFF_NEEDED_DISTANCE_FOR_RUNNER_ON_FIRST: f64 = 75.0;
pub const CUTOFF_NEEDED_TIME_TO_CATCH: f64 = 3.5;
// NOTE: Example: outfield at 90m, base at 0m (home) → place cutoff around 35–40m
pub const CUTOFF_DISTANCE_COEFFICIENT: f64 = 0.45;

// Base distance: assume top speed can be maintained up to 30m
pub const BALL_FLIGHT_SPEED_CONTINUE_DISTANCE: f64 = 30.0;

pub const TOUCH_PENALTY_TIME: f64 = 0.3;

// Maximum jump catch height for a fielder (2.5m)
pub const MAX_REACH_HEIGHT: f64 = 2.5; // TODO: Should be changed to Player's ability

pub const WEIGHT_SS_BASE_COVER: f64 = 0.3;
pub const WEIGHT_IS_LOADED_TARGET_THIRD: f64 = 0.3;
