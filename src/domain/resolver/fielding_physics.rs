use crate::domain::random_provider::RandomProvider;
use crate::domain::resolver::fielding_config::MAX_REACH_HEIGHT;
use crate::domain::shared::ball::{BattedBall, FieldedBall};
use crate::domain::shared::game_state::ActiveFielder;
use crate::domain::util::PolarPosition;

/// Types of fielding errors
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FieldError {
    Fumble, // Fumbling around at the hands (processing time delay)
    Passed, // Ball gets by / tunneled (slips behind, transitions to covering play)
}

pub struct FieldPlayPlayResult<'a> {
    pub primary_interception: FielderInterception<'a>, // Main play (or the one who let it pass)
    pub covering_interception: Option<FielderInterception<'a>>, // Covering fielder when the ball gets by
}
impl<'a> FieldPlayPlayResult<'a> {
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CatchType {
    DirectFly,   // No-bounce catch
    BounceCatch, // Catch on one bounce / in the middle of a grounder
    FinalPickup, // Pickup at the final resting point
    Fumble,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FielderRiskTolerance {
    Aggressive,   // Go for the no-bounce catch even when it's right at the limit (high risk, high reward)
    Balanced,     // Normal fielding judgment
    Conservative, // Prioritize avoiding errors/misses; safely field it on one bounce (low risk)
}

pub fn evaluate_fielder_interception<'a>(
    rng: &mut dyn RandomProvider,
    ball: &'a BattedBall,
    fielder: &'a ActiveFielder,
) -> Option<FielderInterception<'a>> {
    // Fielder's Cartesian coordinates (x, y)
    let f_rad = fielder.polar_position.angle.to_radians();
    let f_x0 = fielder.polar_position.distance * f_rad.sin();
    let f_y0 = fielder.polar_position.distance * f_rad.cos();

    let dt = 0.05;
    let mut t = 0.10;

    // Scan chronologically for future points where the ball is at a catchable height (Z <= 2.2m)
    while t <= ball.total_time {
        // Get the ball's polar coordinates (b_r, b_theta), height, and Cartesian coordinates
        let (b_r, b_theta, b_x, b_y, b_z, is_direct) = estimate_ball_state_at_time(t, ball);

        // TODO: fielding_stat should be included to FielderInfo
        let fielding_stat: f64 = 0.99;

        // Only points where the ball is at a catchable height (Z <= MAX_REACH_HEIGHT) become the movement target
        if b_z <= MAX_REACH_HEIGHT {
            // Calculate the fielder's movement distance to the ball's straight-line trajectory and required time (only movement uses Cartesian distance temporarily)
            let move_dist = ((b_x - f_x0).powi(2) + (b_y - f_y0).powi(2)).sqrt();
            let fielder_needed_time =
                fielder.info.prep_time + (move_dist / fielder.info.running_speed);

            // TODO: risk_tolerance should be included to ActiveFielder
            let risk_tolerance = FielderRiskTolerance::Balanced;

            // Can the fielder reach the target point by the ball's arrival time t?
            if fielder_needed_time <= t {
                let waiting_time = t - fielder_needed_time;

                // ---------------------------------------------------------
                // 1. No-bounce catch avoidance judgment based on FielderRiskTolerance
                // ---------------------------------------------------------
                if is_direct {
                    let is_acceptable_risk = match risk_tolerance {
                        FielderRiskTolerance::Aggressive => {
                            // Go for it no matter how tight, as long as waiting time is >= 0
                            waiting_time >= 0.0
                        }
                        FielderRiskTolerance::Balanced => {
                            // Give up the no-bounce catch unless there's 0.15s of margin, and play it on the first bounce
                            waiting_time >= 0.15
                        }
                        FielderRiskTolerance::Conservative => {
                            // Avoid the no-bounce catch unless there's a solid 0.35s of margin
                            waiting_time >= 0.35
                        }
                    };

                    // If the risk is deemed too high, skip the catch at this (no-bounce) point
                    // and continue the loop to find a safe point after the first bounce
                    if !is_acceptable_risk {
                        t += dt;
                        continue;
                    }
                }

                // Fielding error judgment (varies with how little waiting time there is)
                let difficulty = if waiting_time < 0.2 { 3.0 } else { 1.0 };
                let base_error_rate = (1.0 - fielding_stat).clamp(0.01, 0.50);
                let error_probability = base_error_rate * difficulty * 0.10;
                let rng_value = rng.random();

                if rng_value < error_probability {
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

        t += dt;
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
        (4.0 * progress * (1.0 - progress) * 15.0).max(0.0)
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

    let dt = 0.05;
    let mut t = start_time;

    // Search for a point the fielder can reach while the ball is moving (or right after it stops)
    let max_search_time = ball.total_time + 10.0;

    while t <= max_search_time {
        let (b_r, b_theta, b_x, b_y, _b_z, _is_direct) = estimate_ball_state_at_time(t, ball);

        let move_dist = ((b_x - f_x0).powi(2) + (b_y - f_y0).powi(2)).sqrt();
        let fielder_needed_time = fielder.info.prep_time + (move_dist / fielder.info.running_speed);
        // TODO: fielding_stat should be included to FielderInfo
        let fielding_stat: f64 = 0.99;

        // Can the fielder reach that point (b_x, b_y) by time t?
        if fielder_needed_time <= t {
            // Vary the action delay depending on whether the cushion ball is still moving or the pickup is after it stops
            let is_moving_cushion = ball.fence_impact_time.is_some() && t < ball.total_time;
            let base_pickup_delay = if is_moving_cushion { 0.65 } else { 0.40 };
            let rng_value = rng.random();

            // -------------------------------------------------------------
            // Error judgment in final processing (fumble / bobble)
            // -------------------------------------------------------------
            // Error rate increases while handling the cushion (is_moving_cushion)
            let difficulty = if is_moving_cushion { 2.5 } else { 1.0 };
            let error_probability = (1.0 - fielding_stat).clamp(0.01, 0.40) * difficulty * 0.08;

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

        t += dt;
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
