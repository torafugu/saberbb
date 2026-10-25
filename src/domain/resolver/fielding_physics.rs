use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::ball::{BattedBall, FieldedBall};
use crate::domain::shared::game_state::ActiveFielder;
use crate::domain::util::PolarPosition;

use serde::{Deserialize, Serialize};
use strum_macros::AsRefStr;

const DT: f64 = 0.05;
const AIR_PROBE_STEP_SEC: f64 = 0.01;

/// Full airborne scan, independent of candidate filters, early returns and errors.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct AirCatchProbeDiagnostics {
    pub step_sec: f64,
    pub air_samples: u32,
    pub height_ok_samples: u32,
    pub reachable_samples: u32,
    pub risk_ok_samples: u32,
    pub max_margin_sec: Option<f64>,
    pub max_margin_time_sec: Option<f64>,
    pub max_margin_height_m: Option<f64>,
    pub first_catch_time_sec: Option<f64>,
    pub first_catch_height_m: Option<f64>,
    pub first_catch_margin_sec: Option<f64>,
}

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
    /// None only for legacy values; new plays probe every active fielder.
    #[serde(default)]
    pub air_probe: Option<AirCatchProbeDiagnostics>,
    /// Full-interval 50 ms control probe; absent in older records.
    #[serde(default)]
    pub air_probe_coarse: Option<AirCatchProbeDiagnostics>,
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

/// Probe all active fielders at 10 ms without mutating state or drawing randomness.
/// The trajectory samples are shared by the fielders to avoid repeated reconstruction.
/// A successful probe is geometric feasibility, not a guaranteed catch or play result.
pub fn probe_airborne_catches(
    ball: &BattedBall,
    fielders: &[ActiveFielder],
) -> Vec<AirCatchProbeDiagnostics> {
    probe_airborne_catches_with_stride(ball, fielders, 1)
}

/// Control probe with identical conditions and interval, on every fifth 10 ms tick.
/// This intentionally does not reproduce the normal search's accumulated float times,
/// random errors, filters, or early return. Only the sampling stride differs.
pub fn probe_airborne_catches_coarse(
    ball: &BattedBall,
    fielders: &[ActiveFielder],
) -> Vec<AirCatchProbeDiagnostics> {
    probe_airborne_catches_with_stride(ball, fielders, 5)
}

fn probe_airborne_catches_with_stride(
    ball: &BattedBall,
    fielders: &[ActiveFielder],
    stride: u32,
) -> Vec<AirCatchProbeDiagnostics> {
    let mut results = vec![AirCatchProbeDiagnostics {
        step_sec: AIR_PROBE_STEP_SEC * f64::from(stride),
        ..AirCatchProbeDiagnostics::default()
    }; fielders.len()];
    if fielders.is_empty() || !ball.total_time.is_finite() || ball.total_time <= 0.0 {
        return results;
    }

    let mut samples = Vec::new();
    let mut sample_index = 0_u32;
    loop {
        // Use integer ticks rather than accumulating rounding error in t += step.
        let t = 0.10 + f64::from(sample_index) * AIR_PROBE_STEP_SEC;
        if t >= ball.total_time {
            break;
        }
        let (_, _, x, y, height, is_direct) = estimate_ball_state_at_time(t, ball);
        if !is_direct {
            break; // Neither ground nor fence rebounds qualify as direct flies.
        }
        samples.push((t, x, y, height));
        sample_index += stride;
    }

    for (fielder, result) in fielders.iter().zip(&mut results) {
        let rad = fielder.polar_position.angle.to_radians();
        let f_x = fielder.polar_position.distance * rad.sin();
        let f_y = fielder.polar_position.distance * rad.cos();
        let speed = fielder.info.running_speed;
        let prep_time = fielder.info.prep_time;
        let required_margin = fielder.risk_tolerance.required_margin_sec();
        for &(t, x, y, height) in &samples {
            result.air_samples += 1;
            if !height.is_finite() || !fielder.info.reach_height.is_finite()
                || height > fielder.info.reach_height
            {
                continue;
            }
            result.height_ok_samples += 1;
            if !speed.is_finite() || speed <= 0.0 || !prep_time.is_finite() {
                continue;
            }
            let movement = ((x - f_x).powi(2) + (y - f_y).powi(2)).sqrt();
            let margin = t - (prep_time + movement / speed);
            if !margin.is_finite() {
                continue;
            }
            if result.max_margin_sec.is_none_or(|best| margin > best) {
                result.max_margin_sec = Some(margin);
                result.max_margin_time_sec = Some(t);
                result.max_margin_height_m = Some(height);
            }
            if margin < 0.0 {
                continue;
            }
            result.reachable_samples += 1;
            if margin < required_margin {
                continue;
            }
            result.risk_ok_samples += 1;
            if result.first_catch_time_sec.is_none() {
                result.first_catch_time_sec = Some(t);
                result.first_catch_height_m = Some(height);
                result.first_catch_margin_sec = Some(margin);
            }
        }
    }
    results
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

/// Reconstruct a coarse trajectory through the recorded ground/fence events.
/// Horizontal segments are straight lines in Cartesian space. The initial flight
/// remains a height envelope, not a replay of the batting physics integration.
pub fn estimate_ball_state_at_time(
    time_sec: f64,
    ball: &BattedBall,
) -> (f64, f64, f64, f64, f64, bool) {
    let total_time = ball.total_time.max(0.0);
    let t = time_sec.max(0.0).min(total_time);
    if total_time == 0.0 {
        let position = ball.final_position;
        return (position.distance, position.angle, position.x, position.y, 0.0, false);
    }

    // Ignore out-of-range timestamps. Time-only legacy records still establish
    // a flight boundary; their missing horizontal anchor uses the final ray.
    let valid_time = |time: f64| time.is_finite() && (0.0..=total_time).contains(&time);
    let bounce_time = ball.first_bounce_time.filter(|time| valid_time(*time));
    let fence_time = ball.fence_impact_time.filter(|time| valid_time(*time));
    let fallback_position = |time: f64| {
        PolarPosition::new(ball.final_position.distance * (time / total_time), ball.final_position.angle)
    };
    let bounce = bounce_time.map(|time| {
        (time, ball.first_bounce_position.unwrap_or_else(|| fallback_position(time)))
    });
    let fence = fence_time.map(|time| {
        (time, ball.fence_impact_position.unwrap_or_else(|| fallback_position(time)))
    });

    // Include both events, regardless of which happened first. For simultaneous
    // events the ground anchor wins; no division by a zero-duration segment occurs.
    let mut events = [fence, bounce];
    if events[0].is_none()
        || matches!((events[0], events[1]), (Some(a), Some(b)) if b.0 < a.0)
    {
        events.swap(0, 1);
    }
    let mut start_time = 0.0;
    let mut start_position = PolarPosition::new(0.0, ball.final_position.angle);
    let mut end_time = total_time;
    let mut end_position = ball.final_position;
    for (event_time, event_position) in events.into_iter().flatten() {
        if t < event_time {
            end_time = event_time;
            end_position = event_position;
            break;
        }
        start_time = event_time;
        start_position = event_position;
    }
    let duration = end_time - start_time;
    let progress = if duration > 0.0 {
        ((t - start_time) / duration).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let x = start_position.x + (end_position.x - start_position.x) * progress;
    let y = start_position.y + (end_position.y - start_position.y) * progress;
    let distance = x.hypot(y);
    let angle = if distance > 0.0 {
        x.atan2(y).to_degrees()
    } else {
        end_position.angle
    };

    // A ball is no longer a direct fly after either a ground bounce or a fence
    // collision, including when the other event is recorded at a later time.
    let direct_until = bounce_time.into_iter().chain(fence_time).fold(total_time, f64::min);
    let is_direct = t < direct_until;

    // Rolling time must not stretch the initial airborne arc. When the fence
    // precedes the ground, retain airborne height until that first ground contact,
    // while is_direct is already false. With no ground event, use the fence/end
    // as the available height boundary. Subsequent ground hops are not reconstructed.
    let flight_time = bounce_time.or(fence_time).unwrap_or(total_time);
    let height = if flight_time > 0.0 && t < flight_time {
        let flight_progress = t / flight_time;
        (4.0 * flight_progress * (1.0 - flight_progress) * ball.max_height).max(0.0)
    } else {
        0.0
    };

    (distance, angle, x, y, height, is_direct)
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
        assert_eq!(result.unwrap().catch_type, CatchType::BounceCatch);
        assert!(diagnostics.air_samples > 0);
        assert_eq!(diagnostics.height_ok_samples, 0);
        assert_eq!(diagnostics.max_air_margin_sec, None);
        assert_eq!(diagnostics.max_air_margin_time_sec, None);
        assert_eq!(diagnostics.max_air_margin_height_m, None);
        assert_eq!(diagnostics.end_reason, CandidateEndReason::BounceCatch);
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

#[cfg(test)]
mod trajectory_tests {
    use super::*;
    use crate::domain::random_provider::FixedRng;
    use crate::domain::shared::player::Position;
    use crate::domain::test_support::active_fielder;

    fn observed_fly() -> BattedBall {
        BattedBall {
            launch_speed: 27.8212296937015,
            launch_angle: 42.7842008991087,
            total_time: 6.9,
            first_bounce_time: Some(3.9),
            first_bounce_position: Some(PolarPosition::new(75.8059723102372, 5.60276591880655)),
            final_position: PolarPosition::new(110.241980734776, 4.73367573653012),
            // Representative height; the DB row does not retain max_height.
            max_height: 18.7,
            ..BattedBall::default()
        }
    }

    fn assert_near(actual: f64, expected: f64) {
        assert!((actual - expected).abs() < 1e-8, "{actual} != {expected}");
    }

    fn assert_position(time: f64, ball: &BattedBall, expected: PolarPosition) {
        let (r, angle, x, y, _, _) = estimate_ball_state_at_time(time, ball);
        assert_near(r, expected.distance);
        assert_near(angle, expected.angle);
        assert_near(x, expected.x);
        assert_near(y, expected.y);
    }

    #[test]
    fn observed_fly_reaches_its_recorded_landing_point_and_descends_before_landing() {
        let ball = observed_fly();
        let landing = ball.first_bounce_position.unwrap();
        assert_position(3.9, &ball, landing);
        assert_position(6.9, &ball, ball.final_position);
        let (_, _, _, _, before_height, before_direct) =
            estimate_ball_state_at_time(3.8, &ball);
        assert!(before_direct);
        assert!(before_height > 0.0 && before_height < 2.5);
        let (_, _, _, _, landing_height, landing_direct) =
            estimate_ball_state_at_time(3.9, &ball);
        assert_eq!(landing_height, 0.0);
        assert!(!landing_direct);
        let (_, _, _, _, after_height, after_direct) =
            estimate_ball_state_at_time(4.0, &ball);
        assert_eq!(after_height, 0.0);
        assert!(!after_direct);

        // Before contact the ball follows the landing ray, not a turn from angle 0.
        let (r, angle, _, _, _, _) = estimate_ball_state_at_time(1.95, &ball);
        assert_near(r, landing.distance / 2.0);
        assert_near(angle, landing.angle);
    }

    #[test]
    fn position_and_height_are_continuous_at_first_bounce() {
        let ball = observed_fly();
        let at = estimate_ball_state_at_time(3.9, &ball);
        for time in [3.9 - 1e-8, 3.9 + 1e-8] {
            let state = estimate_ball_state_at_time(time, &ball);
            assert!((state.2 - at.2).abs() < 1e-5);
            assert!((state.3 - at.3).abs() < 1e-5);
            assert!((state.4 - at.4).abs() < 1e-5);
        }
        // The rolling interval joins the landing point to the stop position.
        let halfway = estimate_ball_state_at_time(5.4, &ball);
        let landing = ball.first_bounce_position.unwrap();
        assert_near(halfway.2, (landing.x + ball.final_position.x) / 2.0);
        assert_near(halfway.3, (landing.y + ball.final_position.y) / 2.0);
    }

    #[test]
    fn center_fielder_can_catch_the_observed_shape_before_it_bounces() {
        let ball = observed_fly();
        let fielder = active_fielder(Position::CF, 90.0, 0.0);
        let (interception, diagnostics) = evaluate_fielder_interception_with_diagnostics(
            &mut FixedRng::new(0.5), &ball, &fielder,
        );
        let interception = interception.expect("descending fly should be reachable");
        assert_eq!(interception.catch_type, CatchType::DirectFly);
        assert_eq!(interception.error_type, None);
        assert!(interception.catch_time_sec < 3.9);
        assert!(interception.catch_z_m <= fielder.info.reach_height);
        assert!(diagnostics.max_air_margin_sec.unwrap() >= 0.15);
        assert!(diagnostics.max_air_margin_time_sec.unwrap() > 3.0);
        assert_eq!(diagnostics.risk_ok_samples, 1);
    }

    #[test]
    fn fence_then_ground_uses_both_anchors_and_never_returns_direct_after_fence() {
        let ball = BattedBall {
            total_time: 6.0,
            max_height: 10.0,
            fence_impact_time: Some(2.0),
            fence_impact_position: Some(PolarPosition::new(100.0, 10.0)),
            first_bounce_time: Some(3.0),
            first_bounce_position: Some(PolarPosition::new(80.0, 8.0)),
            final_position: PolarPosition::new(70.0, 6.0),
            ..BattedBall::default()
        };
        assert_position(2.0, &ball, ball.fence_impact_position.unwrap());
        assert_position(3.0, &ball, ball.first_bounce_position.unwrap());
        assert_position(6.0, &ball, ball.final_position);
        assert!(estimate_ball_state_at_time(1.99, &ball).5);
        let at_fence = estimate_ball_state_at_time(2.0, &ball);
        assert!(!at_fence.5);
        assert!(at_fence.4 > 0.0); // It may still be airborne after striking the wall.
        assert!(!estimate_ball_state_at_time(2.5, &ball).5);
        for time in [2.0 - 1e-8, 2.0 + 1e-8] {
            let nearby = estimate_ball_state_at_time(time, &ball);
            assert!((nearby.2 - at_fence.2).abs() < 1e-5);
            assert!((nearby.3 - at_fence.3).abs() < 1e-5);
            assert!((nearby.4 - at_fence.4).abs() < 1e-5);
        }
    }

    #[test]
    fn ground_then_fence_is_interpolated_in_chronological_order() {
        let ball = BattedBall {
            total_time: 6.0,
            max_height: 10.0,
            first_bounce_time: Some(2.0),
            first_bounce_position: Some(PolarPosition::new(60.0, 0.0)),
            fence_impact_time: Some(4.0),
            fence_impact_position: Some(PolarPosition::new(100.0, 0.0)),
            final_position: PolarPosition::new(90.0, 0.0),
            ..BattedBall::default()
        };
        for (time, distance) in [(1.0, 30.0), (2.0, 60.0), (3.0, 80.0), (4.0, 100.0), (5.0, 95.0)] {
            assert_position(time, &ball, PolarPosition::new(distance, 0.0));
        }
        assert!(estimate_ball_state_at_time(1.0, &ball).5);
        for time in [2.0, 3.0, 4.0, 5.0] {
            let state = estimate_ball_state_at_time(time, &ball);
            assert!(!state.5);
            assert_eq!(state.4, 0.0);
        }
    }

    #[test]
    fn zero_time_grounders_stopped_balls_and_out_of_range_queries_are_finite() {
        let grounder = BattedBall {
            total_time: 2.0,
            first_bounce_time: Some(0.0),
            first_bounce_position: Some(PolarPosition::new(0.0, 15.0)),
            final_position: PolarPosition::new(40.0, 15.0),
            ..BattedBall::default()
        };
        assert_position(1.0, &grounder, PolarPosition::new(20.0, 15.0));
        for time in [-1.0, 0.0, 1.0, 2.0, 10.0] {
            let state = estimate_ball_state_at_time(time, &grounder);
            assert!([state.0, state.1, state.2, state.3, state.4].iter().all(|n| n.is_finite()));
            assert_eq!(state.4, 0.0);
            assert!(!state.5);
        }
        assert_position(10.0, &grounder, grounder.final_position);
        let stopped = BattedBall {
            final_position: PolarPosition::new(10.0, -20.0),
            ..BattedBall::default()
        };
        assert_position(1.0, &stopped, stopped.final_position);
        assert!(!estimate_ball_state_at_time(1.0, &stopped).5);
    }

    #[test]
    fn sparse_and_simultaneous_event_metadata_has_safe_boundaries() {
        let mut ball = BattedBall {
            total_time: 4.0,
            max_height: 10.0,
            final_position: PolarPosition::new(80.0, 20.0),
            first_bounce_time: Some(2.0),
            ..BattedBall::default()
        };
        // Missing position: preserve the time boundary using a point on the final ray.
        assert_position(2.0, &ball, PolarPosition::new(40.0, 20.0));
        assert!(!estimate_ball_state_at_time(2.0, &ball).5);
        ball.fence_impact_time = Some(2.0);
        ball.fence_impact_position = Some(PolarPosition::new(40.0, 20.0));
        assert_position(2.0, &ball, ball.fence_impact_position.unwrap());
        ball.first_bounce_time = None;
        assert!(!estimate_ball_state_at_time(2.0, &ball).5);
        assert_eq!(estimate_ball_state_at_time(2.0, &ball).4, 0.0);
        ball.fence_impact_time = None;
        assert!(estimate_ball_state_at_time(3.0, &ball).5);
        assert_eq!(estimate_ball_state_at_time(4.0, &ball).4, 0.0);
    }

    #[test]
    fn cartesian_segments_do_not_rotate_through_zero_when_angles_wrap() {
        let ball = BattedBall {
            total_time: 4.0,
            first_bounce_time: Some(2.0),
            first_bounce_position: Some(PolarPosition::new(100.0, 179.0)),
            final_position: PolarPosition::new(100.0, -179.0),
            ..BattedBall::default()
        };
        let state = estimate_ball_state_at_time(3.0, &ball);
        assert!(state.0 > 99.9);
        assert!(state.1.abs() > 179.0);
        assert_near(state.2, 0.0);
        assert!(state.3 < 0.0);
    }
}

#[cfg(test)]
mod air_probe_tests {
    use super::*;
    use crate::domain::random_provider::FixedRng;
    use crate::domain::shared::player::Position;
    use crate::domain::test_support::{active_fielder, default_fielders};

    fn left_field_example() -> (BattedBall, ActiveFielder) {
        let ball = BattedBall {
            launch_speed: 30.0,
            launch_angle: 40.0,
            total_time: 6.0,
            // Match the integrator's 305 additions of 0.01 rather than rounded SQL output.
            first_bounce_time: Some(3.049999999999979),
            first_bounce_position: Some(PolarPosition::new(70.833, -13.867)),
            final_position: PolarPosition::new(100.0, -13.775),
            // Representative height: the historical row does not store this value.
            max_height: 20.0,
            ..BattedBall::default()
        };
        let mut fielder = active_fielder(Position::LF, 80.0, -26.0);
        fielder.id = 6755;
        fielder.info.running_speed = 8.4353393605285;
        fielder.info.prep_time = 0.637923851954722;
        fielder.info.reach_height = 2.49425091521223;
        (ball, fielder)
    }

    #[test]
    fn fine_probe_finds_three_late_air_points_missed_by_the_normal_step() {
        let (ball, fielder) = left_field_example();
        let (_, normal) = evaluate_fielder_interception_with_diagnostics(
            &mut FixedRng::new(0.5), &ball, &fielder,
        );
        assert_eq!(normal.risk_ok_samples, 0);
        let coarse = probe_airborne_catches_coarse(&ball, &[fielder.clone()])[0];
        assert_eq!(coarse.step_sec, 0.05);
        assert_eq!(coarse.air_samples, 59);
        assert_eq!(coarse.risk_ok_samples, 0);
        let probe = probe_airborne_catches(&ball, &[fielder])[0];
        assert_eq!(probe.step_sec, 0.01);
        assert_eq!(probe.air_samples, 295);
        assert_eq!(probe.risk_ok_samples, 3); // Scan continues after the first success.
        assert!((probe.first_catch_time_sec.unwrap() - 3.02).abs() < 1e-9);
        assert!((probe.max_margin_time_sec.unwrap() - 3.04).abs() < 1e-9);
        assert!(probe.first_catch_margin_sec.unwrap() > 0.17);
        assert!(probe.first_catch_height_m.unwrap() < 2.4943);
    }

    #[test]
    fn coarse_control_uses_the_same_height_and_margin_at_common_ticks() {
        let ball = BattedBall {
            total_time: 0.41,
            first_bounce_time: Some(0.40),
            first_bounce_position: Some(PolarPosition::new(1.0, 0.0)),
            final_position: PolarPosition::new(2.0, 0.0),
            max_height: 0.1,
            ..BattedBall::default()
        };
        let mut fielder = active_fielder(Position::CF, 1.0, 0.0);
        fielder.info.prep_time = 0.0;
        fielder.info.running_speed = 100.0;
        fielder.info.reach_height = 3.0;
        fielder.risk_tolerance = FielderRiskTolerance::Aggressive;
        let fine = probe_airborne_catches(&ball, &[fielder.clone()])[0];
        let coarse = probe_airborne_catches_coarse(&ball, &[fielder])[0];
        assert!(coarse.risk_ok_samples > 0);
        assert!(fine.risk_ok_samples >= coarse.risk_ok_samples);
        assert_eq!(fine.first_catch_time_sec, coarse.first_catch_time_sec);
        assert_eq!(fine.first_catch_height_m, coarse.first_catch_height_m);
        assert_eq!(fine.first_catch_margin_sec, coarse.first_catch_margin_sec);
        assert!(coarse.max_margin_time_sec.unwrap() < 0.40);
        assert!(fine.max_margin_time_sec.unwrap() < 0.40);
    }

    #[test]
    fn fine_probe_applies_height_conditions_not_just_arrival_time() {
        let (mut ball, fielder) = left_field_example();
        ball.max_height = 500.0;
        let probe = probe_airborne_catches(&ball, &[fielder])[0];
        assert!(probe.air_samples > 0);
        assert_eq!(probe.height_ok_samples, 0);
        assert_eq!(probe.risk_ok_samples, 0);
        assert_eq!(probe.max_margin_sec, None);
        assert_eq!(probe.first_catch_time_sec, None);
    }

    #[test]
    fn fine_probe_keeps_negative_margins_and_rejects_invalid_movement_speed() {
        let (ball, mut fielder) = left_field_example();
        fielder.info.prep_time = 100.0;
        let probe = probe_airborne_catches(&ball, &[fielder.clone()])[0];
        assert!(probe.height_ok_samples > 0);
        assert!(probe.max_margin_sec.unwrap() < 0.0);
        assert_eq!(probe.reachable_samples, 0);
        assert_eq!(probe.first_catch_time_sec, None);

        fielder.info.running_speed = 0.0;
        let invalid = probe_airborne_catches(&ball, &[fielder])[0];
        assert!(invalid.height_ok_samples > 0);
        assert_eq!(invalid.max_margin_sec, None);
        assert_eq!(invalid.risk_ok_samples, 0);
    }

    #[test]
    fn fine_probe_checks_risk_tolerance_and_stops_at_fence_impact() {
        let ball = BattedBall {
            total_time: 0.5,
            first_bounce_time: Some(0.3),
            first_bounce_position: Some(PolarPosition::new(0.6, 0.0)),
            final_position: PolarPosition::new(1.0, 0.0),
            max_height: 10.0,
            ..BattedBall::default()
        };
        let mut fielder = active_fielder(Position::CF, 0.0, 0.0);
        fielder.info.running_speed = 1e9;
        fielder.info.prep_time = 0.15;
        fielder.info.reach_height = 100.0;
        let balanced = probe_airborne_catches(&ball, &[fielder.clone()])[0];
        assert!(balanced.reachable_samples > 0);
        assert_eq!(balanced.risk_ok_samples, 0);
        fielder.risk_tolerance = FielderRiskTolerance::Aggressive;
        let aggressive = probe_airborne_catches(&ball, &[fielder.clone()])[0];
        assert!(aggressive.risk_ok_samples > 0);

        let mut wall_ball = ball;
        wall_ball.fence_impact_time = Some(0.2);
        wall_ball.fence_impact_position = Some(PolarPosition::new(0.4, 0.0));
        let wall = probe_airborne_catches(&wall_ball, &[fielder])[0];
        assert!(wall.air_samples < aggressive.air_samples);
        assert!(wall.max_margin_time_sec.unwrap() < 0.2);
        assert!(wall.first_catch_time_sec.unwrap() < 0.2);
    }

    #[test]
    fn ground_balls_have_a_recorded_zero_probe_for_every_fielder() {
        let grounder = BattedBall {
            total_time: 3.0,
            first_bounce_time: Some(0.0),
            first_bounce_position: Some(PolarPosition::new(0.0, 0.0)),
            final_position: PolarPosition::new(50.0, 0.0),
            ..BattedBall::default()
        };
        let fielders = default_fielders();
        let before = fielders.clone();
        let probes = probe_airborne_catches(&grounder, &fielders);
        assert_eq!(fielders, before);
        assert_eq!(probes.len(), 9);
        assert!(probes.iter().all(|p| p.step_sec == 0.01 && p.air_samples == 0
            && p.risk_ok_samples == 0 && p.max_margin_sec.is_none()
            && p.first_catch_time_sec.is_none()));
        assert!(probe_airborne_catches(&grounder, &[]).is_empty());
    }
}
