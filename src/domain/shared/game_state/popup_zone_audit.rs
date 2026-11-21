use super::{GameError, GameState};
use crate::domain::{
    resolver::{
        batting_resolver::{
            SwingContactType, SwingExecutionError, calculate_batted_ball_with_metrics,
            evaluate_swing_contact,
        },
        fielding_physics::CatchType,
        pitching_resolver::PitchDisplacement,
    },
    shared::{
        ball::{FOUL_DEGREE, INFIELD_DISTANCE, OutboundResult, PitchedBall},
        game::BattingResult,
        player::BatterInfo,
    },
    strategy::batting_strategy::BattedBallIntent,
};
use std::cell::RefCell;

pub const POPUP_SCAN_X_MM: [i32; 9] = [-160, -120, -80, -40, 0, 40, 80, 120, 160];
pub const POPUP_SCAN_Z_MM: [i32; 9] = [30, 40, 50, 60, 70, 80, 90, 100, 110];
pub const POPUP_SCAN_VLA_DEG: f64 = 50.0;
pub const POPUP_SCAN_INFIELD_M: f64 = INFIELD_DISTANCE;

#[derive(Clone, Copy, Debug)]
pub struct PopupZoneLaunch {
    pub exit_speed_ms: f64,
    pub vla_deg: f64,
    pub hla_deg: f64,
    pub foul: bool,
    pub projected_infield_bounce: bool,
    pub clean_infielder_air_out: bool,
}

#[derive(Clone, Copy, Debug)]
pub struct PopupZonePlay {
    pub x_m: f64,
    pub z_m: f64,
    pub outcome: Option<BattingResult>,
    pub launch: Option<PopupZoneLaunch>,
}

pub struct PopupZoneSwing {
    pub count_seq: u16,
    pub intent: BattedBallIntent,
    pub baseline: PopupZonePlay,
    /// Z-major order, with X increasing within each row.
    pub grid: Vec<PopupZonePlay>,
}

thread_local! {
    static SWINGS: RefCell<Option<Vec<PopupZoneSwing>>> = const { RefCell::new(None) };
}

pub fn with_popup_zone_audit<T>(run: impl FnOnce() -> T) -> (T, Vec<PopupZoneSwing>) {
    struct Restore(Option<Vec<PopupZoneSwing>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            SWINGS.replace(self.0.take());
        }
    }
    let _restore = Restore(SWINGS.replace(Some(Vec::new())));
    let result = run();
    (result, SWINGS.take().expect("popup zone audit is active"))
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

impl GameState {
    pub(super) fn record_popup_zone(
        &self,
        batter: &BatterInfo,
        pitch: PitchedBall,
        displacement: &PitchDisplacement,
        swing_error: SwingExecutionError,
        intent: BattedBallIntent,
    ) -> Result<(), GameError> {
        SWINGS.with_borrow_mut(|slot| {
            let Some(swings) = slot.as_mut() else {
                return Ok(());
            };
            let Some(active) = &self.inning_state.active_batter else {
                return Ok(());
            };
            let pitcher_id = self.current_pitcher()?.id;
            let sample = |error: SwingExecutionError| -> Result<PopupZonePlay, GameError> {
                let mut rng = self.rng.fork().ok_or(GameError::PairedContactRng)?;
                let contact = evaluate_swing_contact(rng.as_mut(), batter, displacement, &error);
                let mut play = PopupZonePlay {
                    x_m: contact.offset_x_m,
                    z_m: contact.offset_z_m,
                    outcome: None,
                    launch: None,
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
                let replay = self.replay_swing(
                    pitcher_id,
                    active.id,
                    batter.batting_side,
                    &contact,
                    Some((ball, metrics)),
                    rng,
                )?;
                play.outcome = replay.batting_result;
                play.launch = Some(PopupZoneLaunch {
                    exit_speed_ms: ball.launch_speed,
                    vla_deg: ball.launch_angle,
                    hla_deg: metrics.final_hla_deg,
                    foul: replay.batting_result == Some(BattingResult::Foul)
                        || ball.outbound_result == OutboundResult::Foul
                        || (replay.batting_result.is_none() && ball.is_foul()),
                    projected_infield_bounce: ball.first_bounce_position.is_some_and(|point| {
                        point.distance < INFIELD_DISTANCE && point.angle.abs() < FOUL_DEGREE
                    }),
                    clean_infielder_air_out: replay.batting_result == Some(BattingResult::Out)
                        && replay
                            .fielding
                            .and_then(|fielding| fielding.catch_diagnostics)
                            .is_some_and(|catch| {
                                catch.fielder_position.is_infielder()
                                    && catch.catch_type == CatchType::DirectFly
                                    && catch.error_type.is_none()
                            }),
                });
                Ok(play)
            };
            let baseline = sample(swing_error)?;
            let mut grid = Vec::with_capacity(POPUP_SCAN_X_MM.len() * POPUP_SCAN_Z_MM.len());
            for z_mm in POPUP_SCAN_Z_MM {
                for x_mm in POPUP_SCAN_X_MM {
                    grid.push(sample(error_at_offset(
                        baseline,
                        swing_error,
                        f64::from(x_mm) / 1000.0,
                        f64::from(z_mm) / 1000.0,
                    ))?);
                }
            }
            swings.push(PopupZoneSwing {
                count_seq: self.count_seq,
                intent,
                baseline,
                grid,
            });
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targeting_grid_offsets_preserves_angles_and_timing() {
        let baseline = PopupZonePlay {
            x_m: 0.13,
            z_m: -0.03,
            outcome: None,
            launch: None,
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
