use super::{ActiveFielder, DEFAULT_LINE_DRIVE_LIFT_OFFSET_MM, GameError, GameState, SwingReplay};
use crate::domain::{
    resolver::{
        batting_resolver::{BattingFactor, SwingExecutionError, adapt_to_pitch},
        fielding_physics::FieldPlayPlayResult,
        pitching_resolver::{PitchDisplacement, StrikeZoneDimensions},
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
    Aim {
        offsets: IntentAimOffsets,
        measure_retention: bool,
    },
    ContactFirstSpeed(f64),
    LengthLoss(f64),
    TimingGeometry(PitchBatTimingTrial),
}

#[derive(Clone, Copy, Debug, Serialize)]
pub struct PitchBatTimingTrial {
    pub contact_first_z_mm: i32,
    pub contact_first_attack_deg: i32,
    pub use_path_mismatch: bool,
    pub contact_first_vertical_bat_control_delta: f64,
}

#[derive(Clone, Copy, PartialEq)]
struct ReplayInputs {
    aim_m: f64,
    speed_factor: f64,
    length_loss: Option<f64>,
    attack_delta_deg: f64,
    use_path_mismatch: bool,
    vertical_bat_control_delta: f64,
}

impl Trial {
    fn inputs(self, intent: BattedBallIntent) -> ReplayInputs {
        let mut inputs = ReplayInputs {
            aim_m: super::intent_aim_offset_m(intent),
            speed_factor: 1.0,
            length_loss: None,
            attack_delta_deg: 0.0,
            use_path_mismatch: false,
            vertical_bat_control_delta: 0.0,
        };
        match self {
            Self::Aim {
                offsets,
                measure_retention,
            } => {
                inputs.aim_m = offsets.offset_m(intent);
                inputs.length_loss = measure_retention.then_some(0.0);
            }
            Self::ContactFirstSpeed(factor) if intent == BattedBallIntent::ContactFirst => {
                inputs.speed_factor = factor;
            }
            Self::ContactFirstSpeed(_) => {}
            Self::LengthLoss(loss) => inputs.length_loss = Some(loss),
            Self::TimingGeometry(trial) => {
                if intent == BattedBallIntent::ContactFirst {
                    inputs.aim_m = f64::from(trial.contact_first_z_mm) / 1000.0;
                    inputs.attack_delta_deg = f64::from(trial.contact_first_attack_deg);
                    inputs.vertical_bat_control_delta =
                        trial.contact_first_vertical_bat_control_delta;
                }
                inputs.use_path_mismatch = trial.use_path_mismatch;
                // Request the launch retention measurement with zero additional length loss.
                inputs.length_loss = Some(0.0);
            }
        }
        inputs
    }
}

/// The two local sagittal paths meet ahead of the plate for an early swing and
/// behind it for a late one. A positive height difference puts the ball above the bat.
fn timing_height_error_m(
    late_sec: f64,
    pitch_forward_speed: f64,
    pitch_vertical_speed: f64,
    bat_speed: f64,
    attack_deg: f64,
) -> f64 {
    let (bat_vertical, bat_forward) = attack_deg.to_radians().sin_cos();
    let bat_forward = bat_speed * bat_forward;
    let bat_vertical = bat_speed * bat_vertical;
    late_sec * (pitch_forward_speed * bat_vertical + bat_forward * pitch_vertical_speed)
        / (pitch_forward_speed + bat_forward)
}

#[derive(Clone, Copy)]
pub struct TwoStrikePitch {
    pub count_seq: u16,
    pub intent: BattedBallIntent,
    pub distance_from_zone_edge: f64,
    pub swung: bool,
}

struct Audit {
    arms: Vec<Trial>,
    audit_doubles: bool,
    swings: Vec<IntentAimSwing>,
}

thread_local! {
    static TWO_STRIKE_PITCHES: RefCell<Option<Vec<TwoStrikePitch>>> = const { RefCell::new(None) };
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
    with_aim_trials(arms, false, run)
}

/// Replay aim offsets with EV retention and record the live two-strike pitch decisions.
pub fn with_contact_approach_audit<T>(
    arms: &[IntentAimOffsets],
    run: impl FnOnce() -> T,
) -> (T, Vec<IntentAimSwing>, Vec<TwoStrikePitch>) {
    struct Restore(Option<Vec<TwoStrikePitch>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            TWO_STRIKE_PITCHES.replace(self.0.take());
        }
    }
    let _restore = Restore(TWO_STRIKE_PITCHES.replace(Some(Vec::new())));
    let (result, swings) = with_aim_trials(arms, true, run);
    let pitches = TWO_STRIKE_PITCHES
        .take()
        .expect("two-strike pitch audit is active");
    (result, swings, pitches)
}

pub(super) fn record_two_strike_pitch(
    count_seq: u16,
    distance_from_zone_edge: f64,
    swung: bool,
    intent: impl FnOnce() -> BattedBallIntent,
) {
    TWO_STRIKE_PITCHES.with_borrow_mut(|slot| {
        if let Some(pitches) = slot {
            pitches.push(TwoStrikePitch {
                count_seq,
                intent: intent(),
                distance_from_zone_edge,
                swung,
            });
        }
    });
}

fn with_aim_trials<T>(
    arms: &[IntentAimOffsets],
    measure_retention: bool,
    run: impl FnOnce() -> T,
) -> (T, Vec<IntentAimSwing>) {
    with_trials(
        arms.iter()
            .copied()
            .map(|offsets| Trial::Aim {
                offsets,
                measure_retention,
            })
            .collect(),
        false,
        run,
    )
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
        false,
        run,
    )
}

/// Replay both intents with the original contact geometry and alternate length attenuation.
pub fn with_contact_length_audit<T>(
    losses: &[f64],
    run: impl FnOnce() -> T,
) -> (T, Vec<IntentAimSwing>) {
    with_trials(
        losses.iter().copied().map(Trial::LengthLoss).collect(),
        false,
        run,
    )
}

/// Replay ContactFirst aim and attack angle with and without pitch–bat timing geometry.
pub fn with_pitch_bat_timing_audit<T>(
    trials: &[PitchBatTimingTrial],
    audit_doubles: bool,
    run: impl FnOnce() -> T,
) -> (T, Vec<IntentAimSwing>) {
    with_trials(
        trials.iter().copied().map(Trial::TimingGeometry).collect(),
        audit_doubles,
        run,
    )
}

fn with_trials<T>(
    arms: Vec<Trial>,
    audit_doubles: bool,
    run: impl FnOnce() -> T,
) -> (T, Vec<IntentAimSwing>) {
    struct Restore(Option<Audit>);
    impl Drop for Restore {
        fn drop(&mut self) {
            AUDIT.replace(self.0.take());
        }
    }
    let _restore = Restore(AUDIT.replace(Some(Audit {
        arms,
        audit_doubles,
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
        raw_displacement: &PitchDisplacement,
        batting_factor: &BattingFactor,
        swing_error: &SwingExecutionError,
    ) -> Result<(), GameError> {
        AUDIT.with_borrow_mut(|slot| {
            let Some(audit) = slot.as_mut() else {
                return Ok(());
            };
            let Some(active) = &self.inning_state.active_batter else {
                return Ok(());
            };
            let adapted = adapt_to_pitch(raw_displacement, batter.bat_control, batting_factor);
            let mut unique = Vec::new();
            let mut plays = Vec::with_capacity(audit.arms.len());
            for arm in &audit.arms {
                let inputs = arm.inputs(intent);
                if let Some(&(_, play)) = unique.iter().find(|(seen, _)| *seen == inputs) {
                    plays.push(play);
                    continue;
                }
                // Keep the live horizontal and timing corrections, even in the reduced-control arm.
                let mut displacement = adapted;
                if inputs.vertical_bat_control_delta != 0.0 {
                    displacement.vertical_offset_m = adapt_to_pitch(
                        raw_displacement,
                        batter.bat_control + inputs.vertical_bat_control_delta,
                        batting_factor,
                    )
                    .vertical_offset_m;
                }
                let mut error = *swing_error;
                if inputs.aim_m != 0.0 {
                    error.additional_z_m += inputs.aim_m;
                }
                if inputs.attack_delta_deg != 0.0 {
                    error.actual_attack_angle_deg += inputs.attack_delta_deg;
                }
                let trial_batter = BatterInfo {
                    swing_speed: batter.swing_speed * inputs.speed_factor,
                    ..*batter
                };
                if inputs.use_path_mismatch {
                    let velocity = pitch
                        .velocity_at_plate(&StrikeZoneDimensions::default())
                        .filter(|v| v.is_finite() && v.y < 0.0)
                        .ok_or(GameError::InvalidPitchTimingGeometry)?;
                    error.additional_z_m += timing_height_error_m(
                        displacement.timing_offset_sec,
                        -velocity.y,
                        velocity.z,
                        trial_batter.swing_speed,
                        error.actual_attack_angle_deg,
                    );
                }
                let play = capture_infield(|| {
                    self.replay_swing_variant(
                        active.id,
                        &trial_batter,
                        pitch,
                        &displacement,
                        error,
                        (inputs.length_loss, audit.audit_doubles),
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

#[cfg(test)]
mod tests {
    use super::timing_height_error_m;

    #[test]
    fn matched_paths_or_on_time_swings_add_no_height_error() {
        assert!(
            timing_height_error_m(0.010, 40.0, -10.0, 32.0, 0.25_f64.atan().to_degrees()).abs()
                < 1e-12
        );
        assert_eq!(timing_height_error_m(0.0, 40.0, -10.0, 32.0, 25.0), 0.0);
    }

    #[test]
    fn opposite_timing_errors_move_contact_to_opposite_sides() {
        let late = timing_height_error_m(0.010, 40.0, -4.0, 32.0, 20.0);
        let early = timing_height_error_m(-0.010, 40.0, -4.0, 32.0, 20.0);
        assert!(late > 0.04 && late < 0.05);
        assert_eq!(early, -late);
        assert!(timing_height_error_m(0.010, 40.0, -4.0, 32.0, 0.0) < 0.0);
    }
}
