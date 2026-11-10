//! Fresh FinalPickup clocks and positions; reconstruction never consumes play RNG.
use saberbb::domain::{
    resolver::fielding_physics::{
        CatchType, FieldError, FieldingCatchDiagnostics, PickupFallbackPolicy,
        estimate_ball_state_at_time,
    },
    shared::ball::BattedBall,
    util::PolarPosition,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
const EPS: f64 = 1e-7;
fn near(a: f64, b: f64) -> bool {
    a.is_finite() && b.is_finite() && (a - b).abs() < EPS
}
fn position_near(a: PolarPosition, b: PolarPosition) -> bool {
    near(a.distance, b.distance) && near(a.angle, b.angle) && near(a.x, b.x) && near(a.y, b.y)
}
/// Replay the current search/handling arithmetic from captured inputs, not completion-time position.
pub(super) fn reconstruct(
    diag: FieldingCatchDiagnostics,
    ball: &BattedBall,
    field_time: f64,
    selected_id: Option<i64>,
) -> Result<PolarPosition, &'static str> {
    let p = diag.pickup.ok_or("missing_pickup_snapshot")?;
    if selected_id != Some(p.fielder_id) {
        return Err("missing_or_conflicting_selected_identity");
    }
    if diag.catch_type != CatchType::FinalPickup {
        return Err("not_final_pickup");
    }
    if ![
        p.initial_position.distance,
        p.initial_position.angle,
        p.initial_position.x,
        p.initial_position.y,
        p.prep_time_sec,
        p.running_speed_ms,
        p.pickup_time_sec,
        p.handling_delay_sec,
        ball.total_time,
    ]
    .iter()
    .all(|v| v.is_finite())
        || p.running_speed_ms <= 0.0
        || p.prep_time_sec < 0.0
        || ball.total_time < 0.0
        || ball.total_time > 10000.0
    {
        return Err("invalid_search_input");
    }
    let start = ball
        .fence_impact_time
        .or(ball.first_bounce_time)
        .unwrap_or(0.0);
    if !start.is_finite()
        || start < 0.0
        || !near(start, p.search_start_sec)
        || !near(p.search_step_sec, 0.05)
    {
        return Err("search_contract_mismatch");
    }
    let x0 = p.initial_position.distance * p.initial_position.angle.to_radians().sin();
    let y0 = p.initial_position.distance * p.initial_position.angle.to_radians().cos();
    let mut t = start;
    let mut found = None;
    while t <= ball.total_time + 10.0 {
        let (r, a, x, y, _, _) = estimate_ball_state_at_time(t, ball);
        let needed =
            p.prep_time_sec + ((x - x0).powi(2) + (y - y0).powi(2)).sqrt() / p.running_speed_ms;
        if needed <= t {
            found = Some((t, PolarPosition::new(r, a), needed));
            break;
        }
        t += 0.05;
    }
    let (sample, position, delay, waiting, error) = if let Some((time, position, needed)) = found {
        if p.safety_fallback {
            return Err("unexpected_safety_fallback");
        }
        let draw = p
            .handling_random
            .filter(|v| v.is_finite() && (0.0..1.0).contains(v))
            .ok_or("missing_or_invalid_handling_draw")?;
        // Catching skill is captured separately so fumbles can be reproduced too.
        let catching = p.catching;
        if !catching.is_finite() {
            return Err("invalid_catching");
        }
        let moving = ball.fence_impact_time.is_some() && time < ball.total_time;
        let base = if moving { 0.65 } else { 0.40 };
        let probability =
            (1.0 - catching).clamp(0.01, 0.40) * if moving { 2.5 } else { 1.0 } * 0.08;
        let (delay, error) = if draw < probability {
            (base + 1.5 + (draw * 10.0) % 1.0, Some(FieldError::Fumble))
        } else {
            (base, None)
        };
        (time, position, delay, (time - needed).max(0.0), error)
    } else {
        if !p.safety_fallback || p.handling_random.is_some() {
            return Err("missing_or_invalid_safety_fallback");
        }
        let (r, a, x, y, _, _) = estimate_ball_state_at_time(ball.total_time, ball);
        let needed =
            p.prep_time_sec + ((x - x0).powi(2) + (y - y0).powi(2)).sqrt() / p.running_speed_ms;
        match p.fallback_policy {
            PickupFallbackPolicy::LegacyBallEnd => {
                (ball.total_time, PolarPosition::new(r, a), 1.0, 0.0, None)
            }
            PickupFallbackPolicy::ReachableArrival => {
                let time = ball.total_time.max(needed);
                (
                    time,
                    PolarPosition::new(r, a),
                    1.0,
                    (time - needed).max(0.0),
                    None,
                )
            }
        }
    };
    if !near(sample, p.pickup_time_sec) || !position_near(position, p.pickup_position) {
        return Err("pickup_search_mismatch");
    }
    if !near(delay, p.handling_delay_sec)
        || error != diag.error_type
        || !near(waiting, diag.waiting_time_sec)
    {
        return Err("handling_mismatch");
    }
    if !near(sample + delay, diag.catch_time_sec) || !near(diag.catch_time_sec, field_time) {
        return Err("completion_clock_mismatch");
    }
    Ok(position)
}
#[derive(Default, Serialize)]
struct Moments {
    n: u64,
    mean: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
}
impl Moments {
    fn add(&mut self, v: f64) {
        if v.is_finite() {
            self.n += 1;
            self.mean = Some(self.mean.unwrap_or(v) + (v - self.mean.unwrap_or(v)) / self.n as f64);
            self.min = Some(self.min.map_or(v, |x| x.min(v)));
            self.max = Some(self.max.map_or(v, |x| x.max(v)));
        }
    }
}
#[derive(Default, Serialize)]
pub(super) struct PickupTimingAudit {
    source_records: u64,
    statuses: BTreeMap<String, u64>,
    matched: u64,
    safety_fallbacks: u64,
    pickup_time_sec: Moments,
    handling_delay_sec: Moments,
    completion_clock_residual_sec: Moments,
    completion_time_position_shift_m: Moments,
    model_height_at_pickup_m: Moments,
    required_arrival_sec: Moments,
    completion_minus_required_arrival_sec: Moments,
    completion_before_required_arrival: u64,
    by_selection_path: BTreeMap<String, u64>,
    by_fallback_policy: BTreeMap<String, u64>,
    by_result: BTreeMap<String, u64>,
    examples: BTreeMap<String, Vec<Value>>,
}
impl PickupTimingAudit {
    pub(super) fn record(
        &mut self,
        diag: FieldingCatchDiagnostics,
        ball: &BattedBall,
        field_time: f64,
        selected_id: Option<i64>,
        game_id: u32,
        count_seq: u16,
        result: &str,
    ) -> Option<PolarPosition> {
        self.source_records += 1;
        *self
            .by_selection_path
            .entry(diag.selection_path.as_ref().into())
            .or_default() += 1;
        *self.by_result.entry(result.into()).or_default() += 1;
        let checked = reconstruct(diag, ball, field_time, selected_id);
        let status = checked
            .as_ref()
            .map_or_else(|reason| *reason, |_| "matched");
        *self.statuses.entry(status.into()).or_default() += 1;
        let examples = self.examples.entry(status.into()).or_default();
        if examples.len() < 3 {
            examples.push(json!({"game_id":game_id,"count_seq":count_seq,"result":result,"field_time_sec":field_time,"diagnostics":diag}));
        }
        let position = checked.ok()?;
        self.matched += 1;
        let p = diag.pickup.unwrap();
        *self
            .by_fallback_policy
            .entry(p.fallback_policy.as_ref().into())
            .or_default() += 1;
        let detail = if p.safety_fallback {
            Some("safety_fallback")
        } else if diag.error_type.is_some() {
            Some("handling_error")
        } else {
            None
        };
        if let Some(key) = detail {
            let examples = self.examples.entry(key.into()).or_default();
            if examples.len() < 3 {
                examples.push(json!({"game_id":game_id,"count_seq":count_seq,"result":result,"field_time_sec":field_time,"diagnostics":diag,"ball":ball}));
            }
        }
        self.safety_fallbacks += u64::from(p.safety_fallback);
        let x0 = p.initial_position.distance * p.initial_position.angle.to_radians().sin();
        let y0 = p.initial_position.distance * p.initial_position.angle.to_radians().cos();
        let required =
            p.prep_time_sec + (position.x - x0).hypot(position.y - y0) / p.running_speed_ms;
        self.required_arrival_sec.add(required);
        self.completion_minus_required_arrival_sec
            .add(field_time - required);
        self.completion_before_required_arrival += u64::from(field_time + EPS < required);
        self.pickup_time_sec.add(p.pickup_time_sec);
        self.handling_delay_sec.add(p.handling_delay_sec);
        self.completion_clock_residual_sec
            .add(p.pickup_time_sec + p.handling_delay_sec - field_time);
        let (_, _, x, y, _, _) = estimate_ball_state_at_time(field_time, ball);
        self.completion_time_position_shift_m
            .add((x - position.x).hypot(y - position.y));
        self.model_height_at_pickup_m
            .add(estimate_ball_state_at_time(p.pickup_time_sec, ball).4);
        Some(position)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saberbb::domain::{
        random_provider::{FixedRng, RandomProvider, RealRng},
        resolver::fielding_physics::{FieldingSelectionPath, evaluate_final_pickup},
        shared::{
            game_state::ActiveFielder,
            player::{FielderInfo, FielderType, Position},
        },
    };
    fn fielder(speed: f64) -> ActiveFielder {
        let mut f = ActiveFielder::new(
            Position::CF,
            7,
            FielderInfo {
                fielder_type: FielderType::Outfielder,
                throw_speed: 35.0,
                running_speed: speed,
                reaction: 0.4,
                prep_time: 0.6,
                catching: 0.8,
                reach_height: 2.5,
                reach_range: 0.0,
            },
        );
        f.polar_position = PolarPosition::new(if speed < 1e-6 { 200.0 } else { 60.0 }, 0.0);
        f
    }
    fn ball() -> BattedBall {
        BattedBall {
            final_position: PolarPosition::new(80.0, 0.0),
            total_time: 8.0,
            first_bounce_time: Some(3.0),
            first_bounce_position: Some(PolarPosition::new(40.0, 0.0)),
            max_height: 15.0,
            ..BattedBall::default()
        }
    }
    fn fixture(ball: &BattedBall, speed: f64, draw: f64) -> FieldingCatchDiagnostics {
        let f = fielder(speed);
        let caught = evaluate_final_pickup(&mut FixedRng::new(draw), ball, &f);
        FieldingCatchDiagnostics {
            selection_path: FieldingSelectionPath::FinalPickup,
            fielder_position: Position::CF,
            catch_type: caught.catch_type,
            error_type: caught.error_type,
            catch_time_sec: caught.catch_time_sec,
            catch_z_m: caught.catch_z_m,
            waiting_time_sec: caught.waiting_time_sec,
            first_bounce_time: ball.first_bounce_time,
            pickup: caught.pickup,
            right_field_first_base: None,
        }
    }
    #[test]
    fn moving_pickup_uses_pre_delay_position_and_rejects_tampered_clocks() {
        let ball = ball();
        let d = fixture(&ball, 7.0, 0.9);
        let p = d.pickup.unwrap();
        assert!(p.pickup_time_sec < ball.total_time);
        assert!(near(p.pickup_time_sec, 3.25));
        assert!(near(p.pickup_position.distance, 42.0));
        assert!(p.pickup_position.distance < ball.final_position.distance);
        assert_eq!(p.handling_delay_sec, 0.4);
        let point = reconstruct(d, &ball, d.catch_time_sec, Some(7)).unwrap();
        assert!(estimate_ball_state_at_time(d.catch_time_sec, &ball).0 > point.distance);
        let mut altered = d;
        altered.pickup.as_mut().unwrap().pickup_time_sec += 0.05;
        assert_eq!(
            reconstruct(altered, &ball, d.catch_time_sec, Some(7)),
            Err("pickup_search_mismatch")
        );
        assert_eq!(
            reconstruct(d, &ball, d.catch_time_sec + 0.1, Some(7)),
            Err("completion_clock_mismatch")
        );
        let mut altered = d;
        altered.pickup.as_mut().unwrap().handling_delay_sec += 0.1;
        assert_eq!(
            reconstruct(altered, &ball, d.catch_time_sec, Some(7)),
            Err("handling_mismatch")
        );
    }
    #[test]
    fn wall_delay_and_fumble_are_reproduced_without_dropping_errors() {
        let mut ball = ball();
        ball.fence_impact_time = Some(2.0);
        ball.fence_impact_position = Some(PolarPosition::new(65.0, 0.0));
        for (draw, delay, error) in [(0.9, 0.65, None), (0.0, 2.15, Some(FieldError::Fumble))] {
            let d = fixture(&ball, 7.0, draw);
            assert!(near(d.pickup.unwrap().handling_delay_sec, delay));
            assert_eq!(d.error_type, error);
            reconstruct(d, &ball, d.catch_time_sec, Some(7)).unwrap();
        }
    }
    #[test]
    fn missing_legacy_snapshot_is_unknown_and_identity_is_required() {
        let ball = ball();
        let d = fixture(&ball, 7.0, 0.9);
        let mut value = serde_json::to_value(d).unwrap();
        value.as_object_mut().unwrap().remove("pickup");
        let legacy: FieldingCatchDiagnostics = serde_json::from_value(value).unwrap();
        assert_eq!(
            reconstruct(legacy, &ball, d.catch_time_sec, Some(7)),
            Err("missing_pickup_snapshot")
        );
        assert_eq!(
            reconstruct(d, &ball, d.catch_time_sec, Some(8)),
            Err("missing_or_conflicting_selected_identity")
        );
        let mut audit = PickupTimingAudit::default();
        audit.record(legacy, &ball, d.catch_time_sec, Some(7), 1, 1, "Single");
        assert_eq!(audit.source_records, 1);
        assert_eq!(audit.matched, 0);
        assert_eq!(audit.handling_delay_sec.n, 0);
    }
    #[test]
    fn snapshot_consumes_only_the_existing_handling_draw_and_fallback_consumes_none() {
        let ball = ball();
        for (speed, fallback) in [(7.0, false), (1e-8, true)] {
            let f = fielder(speed);
            let mut rng = RealRng::from_seed(42);
            let mut expected = RealRng::from_seed(42);
            let caught = evaluate_final_pickup(&mut rng, &ball, &f);
            let p = caught.pickup.unwrap();
            assert_eq!(p.safety_fallback, fallback);
            if !fallback {
                assert_eq!(p.handling_random, Some(expected.random()));
            } else {
                assert_eq!(p.handling_random, None);
            }
            assert_eq!(rng.random(), expected.random());
            let d = fixture(&ball, speed, 0.9);
            reconstruct(d, &ball, d.catch_time_sec, Some(7)).unwrap();
            if fallback {
                let mut audit = PickupTimingAudit::default();
                audit.record(d, &ball, d.catch_time_sec, Some(7), 1, 1, "Single");
                assert_eq!(audit.safety_fallbacks, 1);
                assert_eq!(audit.completion_before_required_arrival, 0);
                assert!(audit.completion_minus_required_arrival_sec.min.unwrap() >= 0.0);
                assert_eq!(audit.examples["safety_fallback"].len(), 1);
            }
        }
    }
}

#[cfg(test)]
mod legacy_policy_tests {
    use super::*;
    use saberbb::domain::{
        random_provider::FixedRng,
        resolver::fielding_physics::{FieldingSelectionPath, evaluate_final_pickup},
        shared::{
            game_state::ActiveFielder,
            player::{FielderInfo, FielderType, Position},
        },
    };
    #[test]
    fn missing_policy_on_historical_snapshot_replays_legacy_clock_and_flags_early_arrival() {
        let ball = BattedBall {
            total_time: 3.6,
            final_position: PolarPosition::new(110.0, 0.0),
            first_bounce_time: Some(1.0),
            first_bounce_position: Some(PolarPosition::new(60.0, 0.0)),
            ..BattedBall::default()
        };
        let f = ActiveFielder::new(
            Position::C,
            7,
            FielderInfo {
                fielder_type: FielderType::Catcher,
                throw_speed: 35.0,
                running_speed: 7.0,
                reaction: 0.4,
                prep_time: 0.6,
                catching: 0.8,
                reach_height: 2.5,
                reach_range: 0.0,
            },
        );
        let caught = evaluate_final_pickup(&mut FixedRng::new(0.9), &ball, &f);
        let mut d = FieldingCatchDiagnostics {
            selection_path: FieldingSelectionPath::FinalPickup,
            fielder_position: Position::C,
            catch_type: caught.catch_type,
            error_type: caught.error_type,
            catch_time_sec: caught.catch_time_sec,
            catch_z_m: 0.0,
            waiting_time_sec: caught.waiting_time_sec,
            first_bounce_time: ball.first_bounce_time,
            pickup: caught.pickup,
            right_field_first_base: None,
        };
        assert!(d.pickup.unwrap().safety_fallback);
        let mut value = serde_json::to_value(d).unwrap();
        let p = value["pickup"].as_object_mut().unwrap();
        p.remove("fallback_policy");
        p.insert("pickup_time_sec".into(), json!(ball.total_time));
        value["catch_time_sec"] = json!(ball.total_time + 1.0);
        d = serde_json::from_value(value).unwrap();
        assert_eq!(
            d.pickup.unwrap().fallback_policy,
            PickupFallbackPolicy::LegacyBallEnd
        );
        reconstruct(d, &ball, d.catch_time_sec, Some(7)).unwrap();
        let mut audit = PickupTimingAudit::default();
        audit.record(d, &ball, d.catch_time_sec, Some(7), 1, 1, "Out");
        assert_eq!(audit.completion_before_required_arrival, 1);
        assert_eq!(audit.by_fallback_policy["legacy_ball_end"], 1);
        d.pickup.as_mut().unwrap().fallback_policy = PickupFallbackPolicy::ReachableArrival;
        assert_eq!(
            reconstruct(d, &ball, d.catch_time_sec, Some(7)),
            Err("pickup_search_mismatch")
        );
    }
}
