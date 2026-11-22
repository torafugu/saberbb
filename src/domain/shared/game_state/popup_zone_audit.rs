use super::{GameError, GameState, paired_contact::Play};
use crate::domain::{
    resolver::{
        batting_resolver::{
            SwingContactType, SwingExecutionError, calculate_batted_ball_with_metrics,
            calculate_trajectory, contact_normal_hla_deg, evaluate_swing_contact,
            vertical_contact_ranges_at_x, vertical_offset_boundary_to_exceed_deflection,
            wrap_hla_deg,
        },
        fielding_physics::CatchType,
        pitching_resolver::PitchDisplacement,
    },
    shared::{
        ball::{BattedBall, FOUL_DEGREE, INFIELD_DISTANCE, OutboundResult, PitchedBall},
        game::BattingResult,
        player::BatterInfo,
    },
    strategy::batting_strategy::BattedBallIntent,
    util::PolarPosition,
};
use std::cell::RefCell;

pub const POPUP_SCAN_X_MM: [i32; 9] = [-160, -120, -80, -40, 0, 40, 80, 120, 160];
pub const POPUP_SCAN_Z_MM: [i32; 9] = [30, 40, 50, 60, 70, 80, 90, 100, 110];
pub const POPUP_SCAN_VLA_DEG: f64 = 50.0;
// At the inclusive thickness limit, the computed angle belongs to zero or residual numerical EV.
pub const POPUP_ZERO_SPEED_TOLERANCE_MS: f64 = 1e-9;
pub const POPUP_SCAN_INFIELD_M: f64 = INFIELD_DISTANCE;
pub const POPUP_FIXED_X_STEP_M: f64 = 0.002;
pub const POPUP_HLA_RETENTION_PCT: [u8; 5] = [0, 25, 50, 75, 100];
pub const LIVE_POPUP_Z_SHIFTS_MM: [u8; 5] = [0, 4, 8, 12, 20];
pub const LIVE_POPUP_HLA_RETENTION_PCT: [u8; 3] = [50, 75, 100];

#[derive(Clone, Copy)]
pub enum PopupHlaTrial {
    Disabled,
    Control,
    Sweep,
}

impl PopupHlaTrial {
    pub fn retention_percentages(self) -> &'static [u8] {
        match self {
            Self::Disabled => &[],
            Self::Control => &POPUP_HLA_RETENTION_PCT[..1],
            Self::Sweep => &POPUP_HLA_RETENTION_PCT,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct PopupZoneLaunch {
    pub exit_speed_ms: f64,
    pub vla_deg: f64,
    pub hla_deg: f64,
    pub foul: bool,
    pub projected_infield_bounce: bool,
    pub projected_outfield_bounce: bool,
    pub clean_infielder_air_out: bool,
    pub foul_infielder_air_out: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PopupZonePlay {
    pub x_m: f64,
    pub z_m: f64,
    pub outcome: Option<BattingResult>,
    pub launch: Option<PopupZoneLaunch>,
    pub hla_trial_launches: [Option<PopupZoneLaunch>; POPUP_HLA_RETENTION_PCT.len() - 1],
}

pub struct PopupZoneSwing {
    pub count_seq: u16,
    pub intent: BattedBallIntent,
    pub baseline: PopupZonePlay,
    pub actual_bat_angle_deg: f64,
    pub actual_attack_angle_deg: f64,
    /// Z-major order for a grid, or ascending shifts for the live Z sweep.
    pub grid: Vec<PopupZonePlay>,
    pub fixed_horizontal: Option<FixedHorizontalScan>,
}

pub enum FixedHorizontalScan {
    NoContactAtOrBelowLiveBat,
    VerticalDeflectionCap,
    ThicknessEndsBeforeHighVla,
    LengthEndsBeforeHighVla,
    HighVlaSamples(Vec<PopupZonePlay>),
}

#[derive(Clone, Copy)]
enum AuditMode {
    Grid,
    FixedHorizontal(PopupHlaTrial),
    LiveHlaSweep,
    LiveZSweep,
}

struct Audit {
    mode: AuditMode,
    swings: Vec<PopupZoneSwing>,
}

thread_local! {
    static AUDIT: RefCell<Option<Audit>> = const { RefCell::new(None) };
}

pub fn with_popup_zone_audit<T>(run: impl FnOnce() -> T) -> (T, Vec<PopupZoneSwing>) {
    with_audit(AuditMode::Grid, run)
}

pub fn with_fixed_horizontal_popup_audit<T>(
    hla_control: bool,
    run: impl FnOnce() -> T,
) -> (T, Vec<PopupZoneSwing>) {
    let trial = if hla_control {
        PopupHlaTrial::Control
    } else {
        PopupHlaTrial::Disabled
    };
    with_audit(AuditMode::FixedHorizontal(trial), run)
}

pub fn with_popup_hla_sweep_audit<T>(run: impl FnOnce() -> T) -> (T, Vec<PopupZoneSwing>) {
    with_audit(AuditMode::FixedHorizontal(PopupHlaTrial::Sweep), run)
}

pub fn with_live_popup_hla_sweep_audit<T>(run: impl FnOnce() -> T) -> (T, Vec<PopupZoneSwing>) {
    with_audit(AuditMode::LiveHlaSweep, run)
}

pub fn with_live_popup_z_sweep_audit<T>(run: impl FnOnce() -> T) -> (T, Vec<PopupZoneSwing>) {
    with_audit(AuditMode::LiveZSweep, run)
}

fn with_audit<T>(mode: AuditMode, run: impl FnOnce() -> T) -> (T, Vec<PopupZoneSwing>) {
    struct Restore(Option<Audit>);
    impl Drop for Restore {
        fn drop(&mut self) {
            AUDIT.replace(self.0.take());
        }
    }
    let _restore = Restore(AUDIT.replace(Some(Audit {
        mode,
        swings: Vec::new(),
    })));
    let result = run();
    let swings = AUDIT.take().expect("popup zone audit is active").swings;
    (result, swings)
}

fn fixed_horizontal_scan(
    baseline: PopupZonePlay,
    error: SwingExecutionError,
    mut sample: impl FnMut(SwingExecutionError) -> Result<PopupZonePlay, GameError>,
) -> Result<FixedHorizontalScan, GameError> {
    use FixedHorizontalScan::*;
    let Some([(length_min, length_max), (thickness_min, thickness_max)]) =
        vertical_contact_ranges_at_x(baseline.x_m, error.actual_bat_angle_deg)
    else {
        return Ok(NoContactAtOrBelowLiveBat);
    };
    let lower = baseline.z_m.max(length_min).max(thickness_min);
    let upper = length_max.min(thickness_max);
    if lower > upper {
        return Ok(NoContactAtOrBelowLiveBat);
    }
    let Some(boundary) = vertical_offset_boundary_to_exceed_deflection(
        POPUP_SCAN_VLA_DEG - error.actual_attack_angle_deg,
    ) else {
        return Ok(VerticalDeflectionCap);
    };
    if upper <= boundary {
        return Ok(if thickness_max <= length_max {
            ThicknessEndsBeforeHighVla
        } else {
            LengthEndsBeforeHighVla
        });
    }

    // Use both ends and subdivide the feasible high-VLA interval. The replay
    // still checks contact and the strict VLA threshold at each sampled Z.
    let start = lower.max(boundary);
    let steps = ((upper - start) / POPUP_FIXED_X_STEP_M).ceil() as usize;
    let step = (upper - start) / steps.max(1) as f64;
    let samples = (0..=steps)
        .map(|i| {
            let z_m = if i == steps {
                upper
            } else {
                start + step * i as f64
            };
            sample(error_at_offset(baseline, error, baseline.x_m, z_m))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(HighVlaSamples(samples))
}

/// Set a final world offset while retaining the original timing term for EV and HLA.
fn error_at_offset(
    baseline: PopupZonePlay,
    mut error: SwingExecutionError,
    x_m: f64,
    z_m: f64,
) -> SwingExecutionError {
    error.additional_x_m += x_m - baseline.x_m;
    error.additional_z_m += z_m - baseline.z_m;
    error
}

fn is_foul_launch(
    ball: &BattedBall,
    outcome: Option<BattingResult>,
    air_out_position: Option<PolarPosition>,
) -> bool {
    if let Some(position) = air_out_position {
        return position.angle.abs() >= FOUL_DEGREE;
    }
    outcome == Some(BattingResult::Foul)
        || ball.outbound_result == OutboundResult::Foul
        || ball.is_foul()
}

fn hla_with_rebound_retention(
    live_hla: f64,
    signed_thickness_m: f64,
    tilt_deg: f64,
    handedness: f64,
    retention_pct: u8,
) -> f64 {
    let rebound = contact_normal_hla_deg(signed_thickness_m, tilt_deg) * handedness;
    wrap_hla_deg(live_hla - rebound * (1.0 - f64::from(retention_pct) / 100.0))
}

fn classify_launch(replay: &Play, ball: BattedBall, hla_deg: f64) -> PopupZoneLaunch {
    let clean_air_out = replay
        .fielding
        .and_then(|fielding| fielding.catch_diagnostics)
        .filter(|catch| {
            replay.batting_result == Some(BattingResult::Out)
                && catch.catch_type == CatchType::DirectFly
                && catch.error_type.is_none()
        });
    let foul = is_foul_launch(
        &ball,
        replay.batting_result,
        clean_air_out.and_then(|catch| catch.catch_position),
    );
    let infielder_air_out =
        clean_air_out.is_some_and(|catch| catch.fielder_position.is_infielder());
    PopupZoneLaunch {
        exit_speed_ms: ball.launch_speed,
        vla_deg: ball.launch_angle,
        hla_deg,
        foul,
        projected_infield_bounce: ball.first_bounce_position.is_some_and(|point| {
            point.distance < INFIELD_DISTANCE && point.angle.abs() < FOUL_DEGREE
        }),
        projected_outfield_bounce: ball.first_bounce_position.is_some_and(|point| {
            point.distance >= INFIELD_DISTANCE && point.angle.abs() < FOUL_DEGREE
        }),
        clean_infielder_air_out: infielder_air_out && !foul,
        foul_infielder_air_out: infielder_air_out && foul,
    }
}

impl GameState {
    pub(super) fn record_popup_zone(
        &self,
        batter: &BatterInfo,
        pitch: PitchedBall,
        displacement: &PitchDisplacement,
        swing_error: SwingExecutionError,
        intent: BattedBallIntent,
    ) -> Result<(), GameError> {
        AUDIT.with_borrow_mut(|slot| {
            let Some(audit) = slot.as_mut() else {
                return Ok(());
            };
            let Some(active) = &self.inning_state.active_batter else {
                return Ok(());
            };
            let pitcher_id = self.current_pitcher()?.id;
            let live_only = matches!(audit.mode, AuditMode::LiveHlaSweep);
            let sample = |error: SwingExecutionError,
                          retentions: &[u8]|
             -> Result<PopupZonePlay, GameError> {
                let mut rng = self.rng.fork().ok_or(GameError::PairedContactRng)?;
                let contact = evaluate_swing_contact(rng.as_mut(), batter, displacement, &error);
                let mut play = PopupZonePlay {
                    x_m: contact.offset_x_m,
                    z_m: contact.offset_z_m,
                    outcome: None,
                    launch: None,
                    hla_trial_launches: [None; POPUP_HLA_RETENTION_PCT.len() - 1],
                };
                if contact.adjusted_contact_type == SwingContactType::SwungAndMiss {
                    play.outcome = Some(if self.inning_state.strike + 1 >= super::MAX_STRIKE {
                        BattingResult::Strikeout
                    } else {
                        BattingResult::StrikeSwung
                    });
                    return Ok(play);
                }
                let (ball, metrics) = calculate_batted_ball_with_metrics(
                    rng.as_mut(),
                    batter,
                    pitch,
                    &contact,
                    &self.stadium,
                )?;
                let control_rng = if !retentions.is_empty()
                    && (live_only || ball.launch_angle > POPUP_SCAN_VLA_DEG)
                    && ball.launch_speed > POPUP_ZERO_SPEED_TOLERANCE_MS
                {
                    Some(rng.fork().ok_or(GameError::PairedContactRng)?)
                } else {
                    None
                };
                let replay = self.replay_swing(
                    pitcher_id,
                    active.id,
                    batter.batting_side,
                    &contact,
                    Some((ball, metrics)),
                    rng,
                )?;
                play.outcome = replay.batting_result;
                play.launch = Some(classify_launch(&replay, ball, metrics.final_hla_deg));
                if let Some(control_rng) = control_rng {
                    for (output, &retention_pct) in play
                        .hla_trial_launches
                        .iter_mut()
                        .zip(retentions.iter().filter(|&&pct| pct != 100))
                    {
                        let control_hla = hla_with_rebound_retention(
                            metrics.final_hla_deg,
                            contact.signed_thickness_offset_m.unwrap_or_default(),
                            error.actual_bat_angle_deg,
                            batter.batting_side.handedness_sign(),
                            retention_pct,
                        );
                        let control_ball = calculate_trajectory(
                            ball.launch_speed,
                            ball.launch_angle,
                            control_hla,
                            ball.spin_rate,
                            ball.spin_angle,
                            &self.stadium,
                        )?;
                        let mut control_metrics = metrics;
                        control_metrics.final_hla_deg = control_hla;
                        let control = self.replay_swing(
                            pitcher_id,
                            active.id,
                            batter.batting_side,
                            &contact,
                            Some((control_ball, control_metrics)),
                            control_rng.fork().ok_or(GameError::PairedContactRng)?,
                        )?;
                        *output = Some(classify_launch(&control, control_ball, control_hla));
                    }
                }
                Ok(play)
            };
            let baseline_retentions = if live_only {
                &LIVE_POPUP_HLA_RETENTION_PCT[..]
            } else {
                &[]
            };
            let baseline = sample(swing_error, baseline_retentions)?;
            let mut grid = Vec::new();
            if matches!(audit.mode, AuditMode::LiveZSweep) {
                grid.push(baseline);
                for &shift_mm in &LIVE_POPUP_Z_SHIFTS_MM[1..] {
                    let z_m = baseline.z_m + f64::from(shift_mm) / 1000.0;
                    grid.push(sample(
                        error_at_offset(baseline, swing_error, baseline.x_m, z_m),
                        &[],
                    )?);
                }
            }
            let fixed_horizontal = match audit.mode {
                AuditMode::LiveHlaSweep => None,
                AuditMode::LiveZSweep | AuditMode::FixedHorizontal(_) => {
                    let retentions = match audit.mode {
                        AuditMode::FixedHorizontal(trial) => trial.retention_percentages(),
                        _ => &[],
                    };
                    Some(fixed_horizontal_scan(baseline, swing_error, |error| {
                        sample(error, retentions)
                    })?)
                }
                AuditMode::Grid => {
                    grid.reserve(POPUP_SCAN_X_MM.len() * POPUP_SCAN_Z_MM.len());
                    for z_mm in POPUP_SCAN_Z_MM {
                        for x_mm in POPUP_SCAN_X_MM {
                            grid.push(sample(
                                error_at_offset(
                                    baseline,
                                    swing_error,
                                    f64::from(x_mm) / 1000.0,
                                    f64::from(z_mm) / 1000.0,
                                ),
                                &[],
                            )?);
                        }
                    }
                    None
                }
            };
            audit.swings.push(PopupZoneSwing {
                count_seq: self.count_seq,
                intent,
                baseline,
                actual_bat_angle_deg: swing_error.actual_bat_angle_deg,
                actual_attack_angle_deg: swing_error.actual_attack_angle_deg,
                grid,
                fixed_horizontal,
            });
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removes_only_signed_rebound_from_wrapped_horizontal_angle() {
        let face = 48.0;
        let rebound = contact_normal_hla_deg(-0.060, 30.0);
        for handedness in [-1.0, 1.0] {
            let live = wrap_hla_deg((face + rebound) * handedness);
            assert!(
                (hla_with_rebound_retention(live, -0.060, 30.0, handedness, 0) - face * handedness)
                    .abs()
                    < 1e-12
            );
            for retention in [25, 50, 75, 100] {
                let expected =
                    wrap_hla_deg((face + rebound * f64::from(retention) / 100.0) * handedness);
                let trial = hla_with_rebound_retention(live, -0.060, 30.0, handedness, retention);
                assert!(wrap_hla_deg(trial - expected).abs() < 1e-12);
            }
        }
    }

    #[test]
    fn air_out_uses_interception_territory_even_if_the_ball_would_land_elsewhere() {
        let mut ball = BattedBall {
            final_position: PolarPosition::new(30.0, 15.0),
            ..BattedBall::default()
        };
        assert!(is_foul_launch(
            &ball,
            Some(BattingResult::Out),
            Some(PolarPosition::new(8.0, -145.0))
        ));
        assert!(is_foul_launch(
            &ball,
            Some(BattingResult::Out),
            Some(PolarPosition::new(8.0, 45.0))
        ));
        ball.final_position = PolarPosition::new(30.0, 60.0);
        assert!(!is_foul_launch(
            &ball,
            Some(BattingResult::Out),
            Some(PolarPosition::new(8.0, 44.9))
        ));
        assert!(is_foul_launch(&ball, None, None));
        ball.final_position = PolarPosition::new(30.0, 15.0);
        assert!(is_foul_launch(&ball, Some(BattingResult::Foul), None));
        assert!(!is_foul_launch(&ball, Some(BattingResult::Out), None));
    }

    #[test]
    fn samples_only_high_angle_contact_interval_at_live_x() {
        use FixedHorizontalScan::*;
        let baseline = PopupZonePlay {
            x_m: 0.0,
            z_m: 0.02,
            outcome: None,
            launch: None,
            hla_trial_launches: [None; POPUP_HLA_RETENTION_PCT.len() - 1],
        };
        let error = SwingExecutionError {
            actual_bat_angle_deg: 30.0,
            actual_attack_angle_deg: 15.0,
            ..SwingExecutionError::default()
        };
        let record = |changed: SwingExecutionError| {
            Ok(PopupZonePlay {
                z_m: baseline.z_m + changed.additional_z_m,
                x_m: baseline.x_m + changed.additional_x_m,
                ..baseline
            })
        };
        let HighVlaSamples(samples) = fixed_horizontal_scan(baseline, error, record).unwrap()
        else {
            panic!("expected high-angle samples")
        };
        assert!((samples[0].z_m - 0.05958).abs() < 0.00001);
        assert!((samples.last().unwrap().z_m - 0.08083).abs() < 0.00001);
        assert!(
            samples
                .windows(2)
                .all(|pair| pair[1].z_m - pair[0].z_m <= POPUP_FIXED_X_STEP_M + 1e-12)
        );
        assert!(samples.iter().all(|play| play.x_m == baseline.x_m));
        let classify = |x_m| {
            fixed_horizontal_scan(PopupZonePlay { x_m, ..baseline }, error, |_| {
                panic!("unexpected replay")
            })
            .unwrap()
        };
        assert!(matches!(classify(-0.05), ThicknessEndsBeforeHighVla));
        assert!(matches!(classify(0.165), LengthEndsBeforeHighVla));
        assert!(matches!(classify(0.4), NoContactAtOrBelowLiveBat));
    }

    #[test]
    fn targeting_grid_offsets_preserves_angles_and_timing() {
        let baseline = PopupZonePlay {
            x_m: 0.13,
            z_m: -0.03,
            outcome: None,
            launch: None,
            hla_trial_launches: [None; POPUP_HLA_RETENTION_PCT.len() - 1],
        };
        let error = SwingExecutionError {
            actual_bat_angle_deg: 35.0,
            actual_attack_angle_deg: 15.0,
            ..SwingExecutionError::default()
        };
        let changed = error_at_offset(baseline, error, -0.04, 0.07);
        assert!((baseline.x_m + changed.additional_x_m + 0.04).abs() < 1e-12);
        assert!((baseline.z_m + changed.additional_z_m - 0.07).abs() < 1e-12);
        assert_eq!(changed.actual_bat_angle_deg, error.actual_bat_angle_deg);
        assert_eq!(
            changed.actual_attack_angle_deg,
            error.actual_attack_angle_deg
        );
    }
}
