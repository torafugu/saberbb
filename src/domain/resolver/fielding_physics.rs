use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::{BattedBall, FieldedBall};
use crate::domain::shared::game_state::ActiveFielder;
use crate::domain::util::PolarPosition;

use serde::{Deserialize, Serialize};
use strum_macros::AsRefStr;

const DT: f64 = 0.05;

/// Branch of process_fielding that selected the effective interception.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, AsRefStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum FieldingSelectionPath {
    Interception,
    Covering,
    FinalPickup,
}

/// Snapshot of the effective interception; capturing it consumes no random values.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FieldingCatchDiagnostics {
    pub selection_path: FieldingSelectionPath,
    pub fielder_position: crate::domain::shared::player::Position,
    pub catch_type: CatchType,
    pub error_type: Option<FieldError>,
    pub catch_time_sec: f64,
    pub catch_z_m: f64,
    pub waiting_time_sec: f64,
    pub first_bounce_time: Option<f64>,
}

/// Types of fielding errors
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, AsRefStr)]
pub enum FieldError {
    Fumble, // Fumbling around at the hands (processing time delay)
    Passed, // Ball gets by / tunneled (slips behind, transitions to covering play)
}

/// Diagnostics of the actual search prefix, ending at the first interception or exhaustion.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct AirSearchDiagnostics {
    pub air_samples: u32,
    pub height_ok_samples: u32,
    pub reachable_samples: u32,
    pub risk_ok_samples: u32,
    pub max_air_margin_sec: Option<f64>,
    pub max_air_margin_time_sec: Option<f64>,
    pub max_air_margin_height_m: Option<f64>,
    pub last_evaluated_time_sec: Option<f64>,
    pub end_reason: CandidateEndReason,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize, AsRefStr)]
#[serde(rename_all = "snake_case")]
#[strum(serialize_all = "snake_case")]
pub enum CandidateEndReason {
    FilteredLane,
    FilteredDistance,
    DirectFly,
    BounceCatch,
    FieldingError,
    #[default]
    NoInterception,
}

/// One row for every active fielder, including fielders rejected before the search.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct FielderCandidateDiagnostics {
    pub fielder_id: i64,
    pub fielder_position: crate::domain::shared::player::Position,
    pub lane_eligible: bool,
    pub distance_eligible: bool,
    pub required_margin_sec: f64,
    pub search: Option<AirSearchDiagnostics>,
    pub interception_catch_type: Option<CatchType>,
    pub interception_error_type: Option<FieldError>,
    pub interception_time_sec: Option<f64>,
    pub selected: bool,
    pub selected_path: Option<FieldingSelectionPath>,
}
impl FielderCandidateDiagnostics {
    pub fn end_reason(&self) -> CandidateEndReason {
        if !self.lane_eligible {
            CandidateEndReason::FilteredLane
        } else if !self.distance_eligible {
            CandidateEndReason::FilteredDistance
        } else {
            self.search
                .map_or(CandidateEndReason::NoInterception, |s| s.end_reason)
        }
    }
}

pub struct FieldPlayPlayResult<'a> {
    pub selection_path: FieldingSelectionPath,
    pub candidate_diagnostics: Vec<FielderCandidateDiagnostics>,
    pub primary_interception: FielderInterception<'a>, // Main play (or the one who let it pass)
    pub covering_interception: Option<FielderInterception<'a>>, // Covering fielder when the ball gets by
}
impl<'a> FieldPlayPlayResult<'a> {
    pub fn new(
        selection_path: FieldingSelectionPath,
        primary_interception: FielderInterception<'a>,
        covering_interception: Option<FielderInterception<'a>>,
        mut candidate_diagnostics: Vec<FielderCandidateDiagnostics>,
    ) -> Self {
        let selected = covering_interception.as_ref().unwrap_or(&primary_interception);
        for candidate in &mut candidate_diagnostics {
            candidate.selected = candidate.fielder_id == selected.fielder.id
                && candidate.fielder_position == selected.fielder.position;
            candidate.selected_path = candidate.selected.then_some(selection_path);
        }
        Self {
            selection_path,
            primary_interception,
            covering_interception,
            candidate_diagnostics,
        }
    }

    pub fn diagnostics(&self) -> FieldingCatchDiagnostics {
        let selected = self.result();
        FieldingCatchDiagnostics {
            selection_path: self.selection_path,
            fielder_position: selected.fielder.position,
            catch_type: selected.catch_type,
            error_type: selected.error_type,
            catch_time_sec: selected.catch_time_sec,
            catch_z_m: selected.catch_z_m,
            waiting_time_sec: selected.waiting_time_sec,
            first_bounce_time: selected.ball.first_bounce_time,
        }
    }

    pub fn result(&self) -> &FielderInterception<'a> {
        if let Some(interception) = &self.covering_interception {
            interception
        } else {
            &self.primary_interception
        }
    }
}

#[derive(Debug, Clone)]
pub struct FielderInterception<'a> {
    pub fielder: &'a ActiveFielder,
    pub catch_type: CatchType,
    pub error_type: Option<FieldError>, // None: normal play / Some(FieldError): error occurred
    pub catch_time_sec: f64,            // Completion time of catch/pickup (s)
    pub catch_distance_m: f64,          // Polar r: catch/pickup distance (m)
    pub catch_spray_angle_deg: f64,     // Polar θ: catch/pickup azimuth angle (deg)
    pub catch_z_m: f64,                 // Ball height Z at the moment of catch
    pub waiting_time_sec: f64, // Time the fielder waited at the landing point (s) (>=0 means comfortable)
    pub ball: &'a BattedBall,
}
impl<'a> FielderInterception<'a> {
    // Helper to check whether the play recorded an error
    pub fn is_error(&self) -> bool {
        self.error_type.is_some()
    }

    pub fn ball(&self) -> FieldedBall {
        let is_fly_catch = if self.catch_type == CatchType::DirectFly {
            true
        } else {
            false
        };

        FieldedBall {
            ball: self.ball.clone(),
            fielded_by: self.fielder.position,
            catch_position: PolarPosition::new(self.catch_distance_m, self.catch_spray_angle_deg),
            time_to_field: self.catch_time_sec,
            is_fly_catch: is_fly_catch,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, AsRefStr)]
pub enum CatchType {
    DirectFly,   // No-bounce catch
    BounceCatch, // Catch on one bounce / in the middle of a grounder
    FinalPickup, // Pickup at the final resting point
    Fumble,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FielderRiskTolerance {
    Aggressive, // Go for the no-bounce catch even when it's right at the limit (high risk, high reward)
    Balanced,   // Normal fielding judgment
    Conservative, // Prioritize avoiding errors/misses; safely field it on one bounce (low risk)
}

impl FielderRiskTolerance {
    pub fn required_margin_sec(self) -> f64 {
        match self {
            Self::Aggressive => 0.0,
            Self::Balanced => 0.15,
            Self::Conservative => 0.35,
        }
    }
}

pub fn evaluate_fielder_interception<'a>(
    rng: &mut dyn RandomProvider,
    ball: &'a BattedBall,
    fielder: &'a ActiveFielder,
) -> Option<FielderInterception<'a>> {
    evaluate_fielder_interception_with_diagnostics(rng, ball, fielder).0
}

pub fn evaluate_fielder_interception_with_diagnostics<'a>(
    rng: &mut dyn RandomProvider,
    ball: &'a BattedBall,
    fielder: &'a ActiveFielder,
) -> (Option<FielderInterception<'a>>, AirSearchDiagnostics) {
    let mut diagnostics = AirSearchDiagnostics::default();
    let interception = search_fielder_interception(rng, ball, fielder, &mut diagnostics);
    (interception, diagnostics)
}

fn search_fielder_interception<'a>(
    rng: &mut dyn RandomProvider,
    ball: &'a BattedBall,
    fielder: &'a ActiveFielder,
    diagnostics: &mut AirSearchDiagnostics,
) -> Option<FielderInterception<'a>> {
    // Fielder's Cartesian coordinates (x, y)
    let f_rad = fielder.polar_position.angle.to_radians();
    let f_x0 = fielder.polar_position.distance * f_rad.sin();
    let f_y0 = fielder.polar_position.distance * f_rad.cos();

    let mut t = 0.10;

    // Scan chronologically for future points where the ball is at a catchable height.
    while t <= ball.total_time {
        // Get the ball's polar coordinates (b_r, b_theta), height, and Cartesian coordinates
        let (b_r, b_theta, b_x, b_y, b_z, is_direct) = estimate_ball_state_at_time(t, ball);
        diagnostics.last_evaluated_time_sec = Some(t);
        if is_direct {
            diagnostics.air_samples += 1;
        }

        // Only points where the ball is at the fielder's catchable height become the movement target.
        if b_z <= fielder.info.reach_height {
            // Calculate the fielder's movement distance to the ball's straight-line trajectory and required time (only movement uses Cartesian distance temporarily)
            let move_dist = ((b_x - f_x0).powi(2) + (b_y - f_y0).powi(2)).sqrt();
            let fielder_needed_time =
                fielder.info.prep_time + (move_dist / fielder.info.running_speed);

            if is_direct {
                diagnostics.height_ok_samples += 1;
                let margin = t - fielder_needed_time;
                if diagnostics.max_air_margin_sec.is_none_or(|best| margin > best) {
                    diagnostics.max_air_margin_sec = Some(margin);
                    diagnostics.max_air_margin_time_sec = Some(t);
                    diagnostics.max_air_margin_height_m = Some(b_z);
                }
            }

            // Can the fielder reach the target point by the ball's arrival time t?
            if fielder_needed_time <= t {
                let waiting_time = t - fielder_needed_time;

                // 1. No-bounce catch avoidance judgment based on FielderRiskTolerance
                if is_direct {
                    diagnostics.reachable_samples += 1;
                    let is_acceptable_risk =
                        waiting_time >= fielder.risk_tolerance.required_margin_sec();

                    // If the risk is deemed too high, skip the catch at this (no-bounce) point
                    // and continue the loop to find a safe point after the first bounce
                    if !is_acceptable_risk {
                        t += DT;
                        continue;
                    }
                    diagnostics.risk_ok_samples += 1;
                }

                // Fielding error judgment (varies with how little waiting time there is)
                let difficulty = if waiting_time < 0.2 { 3.0 } else { 1.0 };
                let base_error_rate = (1.0 - fielder.info.catching).clamp(0.01, 0.50);
                let error_probability = base_error_rate * difficulty * 0.10;
                let rng_value = rng.random();

                if rng_value < error_probability {
                    diagnostics.end_reason = CandidateEndReason::FieldingError;
                    if waiting_time >= 0.25 {
                        // [Fumble]: stopped it in front of the body but fumbled at the hands (time delay)
                        let fumble_penalty_sec = 1.2 + (rng_value * 10.0) % 0.8;
                        return Some(FielderInterception {
                            fielder: fielder,
                            catch_type: CatchType::Fumble,
                            error_type: Some(FieldError::Fumble),
                            catch_time_sec: t + fumble_penalty_sec,
                            catch_distance_m: b_r,
                            catch_spray_angle_deg: b_theta,
                            catch_z_m: 0.0,
                            waiting_time_sec: 0.0,
                            ball: ball,
                        });
                    } else {
                        // [Passed]: caught up but touched/deflected it behind (error confirmed)
                        return Some(FielderInterception {
                            fielder: fielder,
                            catch_type: if is_direct {
                                CatchType::DirectFly
                            } else {
                                CatchType::BounceCatch
                            },
                            error_type: Some(FieldError::Passed),
                            catch_time_sec: t, // Time the ball got by
                            catch_distance_m: b_r,
                            catch_spray_angle_deg: b_theta,
                            catch_z_m: b_z,
                            waiting_time_sec: waiting_time,
                            ball: ball,
                        });
                    }
                }

                // Normal catch
                diagnostics.end_reason = if is_direct {
                    CandidateEndReason::DirectFly
                } else {
                    CandidateEndReason::BounceCatch
                };
                return Some(FielderInterception {
                    fielder: fielder,
                    catch_type: if is_direct {
                        CatchType::DirectFly
                    } else {
                        CatchType::BounceCatch
                    },
                    error_type: None,
                    catch_time_sec: t,
                    catch_distance_m: b_r,
                    catch_spray_angle_deg: b_theta,
                    catch_z_m: b_z,
                    waiting_time_sec: t - fielder_needed_time,
                    ball: ball,
                });
            }
        }

        t += DT;
    }

    None // Failed to catch in mid-flight
}

pub fn estimate_ball_state_at_time(
    time_sec: f64,
    ball: &BattedBall,
) -> (f64, f64, f64, f64, f64, bool) {
    let t = time_sec.min(ball.total_time); // Clamp time

    let is_direct = match ball.first_bounce_time {
        Some(bounce_t) => t < bounce_t,
        None => ball.fence_impact_time.map_or(true, |imp| t < imp),
    };

    // Fence impact (cushion ball) occurred and current time t is after the impact
    let (current_dist, current_angle) = match (ball.fence_impact_position, ball.fence_impact_time) {
        (Some(impact_position), Some(impact_time)) => {
            if t >= impact_time {
                // [After fence impact]: interpolate from impact position back toward the final resting position
                let post_impact_duration = ball.total_time - impact_time;
                let factor = if post_impact_duration > 0.0 {
                    ((t - impact_time) / post_impact_duration).min(1.0)
                } else {
                    1.0
                };

                let r = impact_position.distance
                    + (ball.final_position.distance - impact_position.distance) * factor;
                let theta = impact_position.angle
                    + (ball.final_position.angle - impact_position.angle) * factor;
                (r, theta)
            } else {
                // [Before fence impact]: interpolate from home toward the impact position
                let factor = (t / impact_time).min(1.0);
                let r = impact_position.distance * factor;
                let theta = impact_position.angle * factor;
                (r, theta)
            }
        }
        _ => {
            // Normal trajectory interpolation without a cushion
            let factor = (t / ball.total_time).min(1.0);
            let r = ball.final_position.distance * factor;
            let theta = ball.final_position.angle * factor;
            (r, theta)
        }
    };

    // Convert to Cartesian coordinates
    let rad = current_angle.to_radians();
    let x = current_dist * rad.sin();
    let y = current_dist * rad.cos();

    // Determine height Z
    let z = if is_direct {
        // During airborne flight (parabola)
        let progress = if let Some(impact_time) = ball.fence_impact_time {
            (t / impact_time).min(1.0)
        } else {
            (t / ball.total_time).min(1.0)
        };
        // NOTE: The apex of the parabola is at 0.5
        // The base parabola at progress rate p ∈ [0, 1] is p(1 - p), so 0.5 × 0.5 = 0.25
        const HIGHEST_POINT: f64 = 0.25;
        (progress * (1.0 - progress) / HIGHEST_POINT * ball.max_height).max(0.0)
    } else {
        // During ground bounce / rolling
        0.3
    };

    (current_dist, current_angle, x, y, z, is_direct)
}

// Final processing when a hit gets through or the ball gets past the fielder
pub fn evaluate_final_pickup<'a>(
    rng: &mut dyn RandomProvider,
    ball: &'a BattedBall,
    fielder: &'a ActiveFielder,
) -> FielderInterception<'a> {
    let f_rad = fielder.polar_position.angle.to_radians();
    let f_x0 = fielder.polar_position.distance * f_rad.sin();
    let f_y0 = fielder.polar_position.distance * f_rad.cos();

    // Cushion ball occurrence time (or first bounce time if none, or 0.0s if neither)
    let start_time = ball
        .fence_impact_time
        .map(|impact_time| impact_time)
        .or(ball.first_bounce_time)
        .unwrap_or(0.0);

    let mut t = start_time;

    // Search for a point the fielder can reach while the ball is moving (or right after it stops)
    let max_search_time = ball.total_time + 10.0;

    while t <= max_search_time {
        let (b_r, b_theta, b_x, b_y, _b_z, _is_direct) = estimate_ball_state_at_time(t, ball);

        let move_dist = ((b_x - f_x0).powi(2) + (b_y - f_y0).powi(2)).sqrt();
        let fielder_needed_time = fielder.info.prep_time + (move_dist / fielder.info.running_speed);

        // Can the fielder reach that point (b_x, b_y) by time t?
        if fielder_needed_time <= t {
            // Vary the action delay depending on whether the cushion ball is still moving or the pickup is after it stops
            let is_moving_cushion = ball.fence_impact_time.is_some() && t < ball.total_time;
            let base_pickup_delay = if is_moving_cushion { 0.65 } else { 0.40 };
            let rng_value = rng.random();

            // Error judgment in final processing (fumble / bobble)
            // Error rate increases while handling the cushion (is_moving_cushion)
            let difficulty = if is_moving_cushion { 2.5 } else { 1.0 };
            let error_probability =
                (1.0 - fielder.info.catching).clamp(0.01, 0.40) * difficulty * 0.08;

            let (final_delay, error_type) = if rng_value < error_probability {
                // Bobbling a cushion ball or fumbling a rolling ball (significant 1.5s ~ 2.5s delay)
                let fumble_time = base_pickup_delay + 1.5 + (rng_value * 10.0) % 1.0;
                (fumble_time, Some(FieldError::Fumble))
            } else {
                (base_pickup_delay, None)
            };

            return FielderInterception {
                fielder: fielder,
                catch_type: CatchType::FinalPickup,
                error_type,
                catch_time_sec: t + final_delay,
                catch_distance_m: b_r, // Exact polar r at the interception point
                catch_spray_angle_deg: b_theta, // Exact polar θ at the interception point
                catch_z_m: 0.0,
                waiting_time_sec: (t - fielder_needed_time).max(0.0),
                ball: ball,
            };
        }

        t += DT;
    }

    // Fallback (return the final resting position as a safety measure)
    let (final_r, final_theta, _x, _y, _z, _dir) =
        estimate_ball_state_at_time(ball.total_time, ball);
    FielderInterception {
        fielder: fielder,
        catch_type: CatchType::FinalPickup,
        error_type: None,
        catch_time_sec: ball.total_time + 1.0,
        catch_distance_m: final_r,
        catch_spray_angle_deg: final_theta,
        catch_z_m: 0.0,
        waiting_time_sec: 0.0,
        ball: ball,
    }
}

#[cfg(test)]
mod candidate_diagnostics_tests {
    use super::*;
    use crate::domain::random_provider::{FixedRng, RealRng};
    use crate::domain::shared::player::Position;
    use crate::domain::test_support::active_fielder;

    fn short_fly() -> BattedBall {
        BattedBall {
            final_position: PolarPosition::new(1.0, 0.0),
            total_time: 0.5,
            first_bounce_time: Some(0.3),
            first_bounce_position: Some(PolarPosition::new(0.6, 0.0)),
            max_height: 10.0,
            ..BattedBall::default()
        }
    }

    fn nearby_fielder() -> ActiveFielder {
        let mut fielder = active_fielder(Position::CF, 0.0, 0.0);
        fielder.info.running_speed = 1e9;
        fielder.info.prep_time = 0.15;
        fielder.info.reach_height = 100.0;
        fielder
    }

    #[test]
    fn height_failure_has_no_max_margin_but_records_air_samples() {
        let ball = short_fly();
        let mut fielder = nearby_fielder();
        fielder.info.reach_height = 0.0;
        let (result, diagnostics) = evaluate_fielder_interception_with_diagnostics(
            &mut FixedRng::new(0.5), &ball, &fielder,
        );
        assert!(result.is_none());
        assert!(diagnostics.air_samples > 0);
        assert_eq!(diagnostics.height_ok_samples, 0);
        assert_eq!(diagnostics.max_air_margin_sec, None);
        assert_eq!(diagnostics.max_air_margin_time_sec, None);
        assert_eq!(diagnostics.max_air_margin_height_m, None);
        assert_eq!(diagnostics.end_reason, CandidateEndReason::NoInterception);
    }

    #[test]
    fn unreachable_air_points_keep_negative_max_margin() {
        let ball = short_fly();
        let mut fielder = nearby_fielder();
        fielder.info.prep_time = 100.0;
        let (result, diagnostics) = evaluate_fielder_interception_with_diagnostics(
            &mut FixedRng::new(0.5), &ball, &fielder,
        );
        assert!(result.is_none());
        assert!(diagnostics.height_ok_samples > 0);
        assert_eq!(diagnostics.air_samples, diagnostics.height_ok_samples);
        assert_eq!(diagnostics.reachable_samples, 0);
        assert_eq!(diagnostics.risk_ok_samples, 0);
        assert!(diagnostics.max_air_margin_sec.unwrap() < 0.0);
        let t = diagnostics.max_air_margin_time_sec.unwrap();
        let (_, _, x, y, z, direct) = estimate_ball_state_at_time(t, &ball);
        assert!(direct);
        let expected = t - (100.0 + x.hypot(y) / fielder.info.running_speed);
        assert!((diagnostics.max_air_margin_sec.unwrap() - expected).abs() < 1e-9);
        assert_eq!(diagnostics.max_air_margin_height_m, Some(z));
    }

    #[test]
    fn risk_rejection_is_distinct_from_arriving_too_late() {
        let ball = short_fly();
        let mut fielder = nearby_fielder();
        let (result, diagnostics) = evaluate_fielder_interception_with_diagnostics(
            &mut FixedRng::new(0.5), &ball, &fielder,
        );
        assert_eq!(result.unwrap().catch_type, CatchType::BounceCatch);
        assert!(diagnostics.reachable_samples > 0);
        assert_eq!(diagnostics.risk_ok_samples, 0);
        assert!(diagnostics.max_air_margin_sec.unwrap() > 0.0);
        assert!(diagnostics.max_air_margin_sec.unwrap() < 0.15);
        assert_eq!(diagnostics.end_reason, CandidateEndReason::BounceCatch);

        fielder.risk_tolerance = FielderRiskTolerance::Aggressive;
        let (result, aggressive) = evaluate_fielder_interception_with_diagnostics(
            &mut FixedRng::new(0.5), &ball, &fielder,
        );
        assert_eq!(result.unwrap().catch_type, CatchType::DirectFly);
        assert_eq!(aggressive.risk_ok_samples, 1);
        assert!(aggressive.air_samples < diagnostics.air_samples);
        assert_eq!(aggressive.end_reason, CandidateEndReason::DirectFly);
    }

    #[test]
    fn accepted_air_point_can_end_in_a_fielding_error() {
        let ball = short_fly();
        let mut fielder = nearby_fielder();
        fielder.risk_tolerance = FielderRiskTolerance::Aggressive;
        let (result, diagnostics) = evaluate_fielder_interception_with_diagnostics(
            &mut FixedRng::new(0.0), &ball, &fielder,
        );
        assert_eq!(result.unwrap().error_type, Some(FieldError::Passed));
        assert_eq!(diagnostics.risk_ok_samples, 1);
        assert_eq!(diagnostics.end_reason, CandidateEndReason::FieldingError);
    }

    #[test]
    fn instrumentation_does_not_add_random_draws() {
        let ball = short_fly();
        for can_reach in [false, true] {
            let mut fielder = nearby_fielder();
            if !can_reach {
                fielder.info.prep_time = 100.0;
            }
            let mut actual_rng = RealRng::from_seed(42);
            let mut expected_rng = RealRng::from_seed(42);
            let (result, _) = evaluate_fielder_interception_with_diagnostics(
                &mut actual_rng, &ball, &fielder,
            );
            assert_eq!(result.is_some(), can_reach);
            // The existing interception search draws once when it returns an interception.
            if can_reach {
                expected_rng.random();
            }
            assert_eq!(actual_rng.random(), expected_rng.random());
        }
    }
}
