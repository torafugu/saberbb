//! Frozen-play checks of infield interception and first-base timing.
use super::{
    ball::{BattedBall, INFIELD_DISTANCE, OutboundResult, TrajectoryType},
    game::BattingResult,
    game_state::ActiveFielder,
    player::{Position, RL},
    stadium::{Base, Stadium},
};
use crate::domain::{
    resolver::{
        batting_resolver::{GrounderFlightSample, replay_grounder_flight},
        fielding_physics::{
            CatchType, FielderCandidateDiagnostics, FieldPlayPlayResult, FieldingCatchDiagnostics,
            estimate_ball_state_at_time, pickup_handling,
        },
        fielding_resolver::PlayContext,
        fielding_timing::DefenseTimeCalculator,
    },
    util::{PolarPosition, calculate_polar_distance},
};
use serde::Serialize;
use std::{cell::RefCell, collections::BTreeMap};

const SCENARIOS: [&str; 3] = [
    "original_filters_live_grid",
    "without_filters_live_grid",
    "without_filters_10ms",
];

#[derive(Clone, Copy)]
struct Opportunity<'a> {
    fielder: &'a ActiveFielder,
    candidate: &'a FielderCandidateDiagnostics,
    time_sec: f64,
    position: PolarPosition,
    direct: bool,
}

#[derive(Default, Serialize)]
struct Scenario {
    reachable: u64,
    direct_catch: u64,
    bounce: u64,
    first_out: u64,
    first_safe: u64,
    first_unknown: u64,
    interception_time_sum_sec: f64,
    timed_first_plays: u64,
    defense_first_time_sum_sec: f64,
    batter_first_time_sum_sec: f64,
    by_position: BTreeMap<String, u64>,
    by_live_candidate_reason: BTreeMap<String, u64>,
}

impl Scenario {
    fn add(
        &mut self,
        opportunity: Opportunity<'_>,
        fielders: &[ActiveFielder],
        runner_first: Option<f64>,
    ) {
        self.reachable += 1;
        self.interception_time_sum_sec += opportunity.time_sec;
        *self
            .by_position
            .entry(opportunity.fielder.position.as_ref().into())
            .or_default() += 1;
        *self
            .by_live_candidate_reason
            .entry(opportunity.candidate.end_reason().as_ref().into())
            .or_default() += 1;
        if opportunity.direct {
            self.direct_catch += 1;
            return;
        }
        self.bounce += 1;
        match runner_first.zip(first_completion(opportunity, fielders)) {
            Some((runner, defense)) => {
                self.timed_first_plays += 1;
                self.defense_first_time_sum_sec += defense;
                self.batter_first_time_sum_sec += runner;
                if defense < runner {
                    self.first_out += 1;
                } else {
                    self.first_safe += 1;
                }
            }
            None => self.first_unknown += 1,
        }
    }
}

fn first_completion(opportunity: Opportunity<'_>, fielders: &[ActiveFielder]) -> Option<f64> {
    let fielder = opportunity.fielder;
    let base = Base::First.polar_position();
    let calc = DefenseTimeCalculator::default();
    let carry = opportunity.time_sec + calc.run_to_base_time(fielder, &opportunity.position, &base);
    let receiver = if fielder.position == Position::FB {
        Position::P
    } else {
        Position::FB
    };
    let direct = fielders
        .iter()
        .find(|f| f.position == receiver)
        .and_then(|cover| {
            if !cover.info.prep_time.is_finite()
                || cover.info.prep_time < 0.0
                || !cover.info.running_speed.is_finite()
                || cover.info.running_speed <= 0.0
                || !fielder.info.throw_speed.is_finite()
                || fielder.info.throw_speed <= 0.0
            {
                return None;
            }
            // Cover reacts at the interception. Throw and cover run happen concurrently.
            let cover_duration = cover.info.prep_time
                + calculate_polar_distance(&cover.polar_position, &base) / cover.info.running_speed;
            let throw_duration =
                calc.direct_play_time(fielder, &opportunity.position, Base::First, false);
            let completion = opportunity.time_sec + cover_duration.max(throw_duration);
            completion.is_finite().then_some(completion)
        })?;
    let completion = carry.min(direct);
    completion.is_finite().then_some(completion)
}

#[derive(Clone, Copy)]
struct ArrivalGap {
    ball_sec: f64,
    prep_sec: f64,
    travel_sec: f64,
    delay_sec: f64,
    position: Position,
}

#[derive(Clone, Copy, Default)]
struct ScanResult<'a> {
    opportunities: [Option<Opportunity<'a>>; 3],
    best_any: Option<ArrivalGap>,
    best_after_bounce: Option<ArrivalGap>,
}

fn keep_smaller_gap(best: &mut Option<ArrivalGap>, point: ArrivalGap) {
    if best.is_none_or(|previous| point.delay_sec < previous.delay_sec) {
        *best = Some(point);
    }
}

fn find_opportunities<'a>(
    ball: &BattedBall,
    candidates: &[(&'a ActiveFielder, &'a FielderCandidateDiagnostics)],
    pickup_sec: f64,
) -> ScanResult<'a> {
    let mut scan = ScanResult::default();
    let first_bounce = ball
        .first_bounce_time
        .filter(|t| t.is_finite() && (0.0..=ball.total_time).contains(t));
    for index in 0.. {
        // Same 0.10 s start and air sampling as live; the relaxed control
        // also samples every 10 ms after the bounce.
        let t = 0.10 + f64::from(index) * 0.01;
        if t >= pickup_sec || t > ball.total_time {
            break;
        }
        let (distance, angle, x, y, height, direct) = estimate_ball_state_at_time(t, ball);
        if !distance.is_finite() || distance >= INFIELD_DISTANCE {
            break;
        }
        let live_grid = direct || index % 5 == 0;
        for &(fielder, candidate) in candidates {
            let start = fielder.polar_position;
            let dx = x - start.distance * start.angle.to_radians().sin();
            let dy = y - start.distance * start.angle.to_radians().cos();
            let travel = (dx.powi(2) + dy.powi(2)).sqrt() / fielder.info.running_speed;
            let needed = fielder.info.prep_time + travel;
            if !height.is_finite() || height > fielder.info.reach_height || !needed.is_finite() {
                continue;
            }
            let point = ArrivalGap {
                ball_sec: t,
                prep_sec: fielder.info.prep_time,
                travel_sec: travel,
                delay_sec: needed - t,
                position: fielder.position,
            };
            keep_smaller_gap(&mut scan.best_any, point);
            if first_bounce.is_some_and(|bounce| t >= bounce) {
                keep_smaller_gap(&mut scan.best_after_bounce, point);
            }
            let required_margin = if direct {
                candidate.required_margin_sec
            } else {
                0.0
            };
            if needed > t || t - needed < required_margin {
                continue;
            }
            let opportunity = Opportunity {
                fielder,
                candidate,
                time_sec: t,
                position: PolarPosition::new(distance, angle),
                direct,
            };
            scan.opportunities[2].get_or_insert(opportunity);
            if live_grid {
                scan.opportunities[1].get_or_insert(opportunity);
                if candidate.lane_eligible && candidate.distance_eligible {
                    scan.opportunities[0].get_or_insert(opportunity);
                }
            }
        }
    }
    scan
}

type InfieldCandidate<'a> = (&'a ActiveFielder, &'a FielderCandidateDiagnostics);

fn infield_candidates<'a>(
    ball: &BattedBall,
    fielders: &'a [ActiveFielder],
    candidates: &'a [FielderCandidateDiagnostics],
) -> Option<Vec<InfieldCandidate<'a>>> {
    if !ball.total_time.is_finite() || ball.total_time < 0.0 {
        return None;
    }
    let inputs = fielders
        .iter()
        .filter(|f| f.position.is_infielder())
        .map(|fielder| {
            if !fielder.info.running_speed.is_finite()
                || fielder.info.running_speed <= 0.0
                || !fielder.info.prep_time.is_finite()
                || fielder.info.prep_time < 0.0
                || !fielder.info.reach_height.is_finite()
            {
                return None;
            }
            let candidate = candidates.iter().find(|c| {
                c.fielder_position == fielder.position && c.fielder_id == fielder.id
            })?;
            Some((fielder, candidate))
        })
        .collect::<Option<Vec<_>>>()?;
    (!inputs.is_empty()).then_some(inputs)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InfieldReach {
    LiveGridAndFilters,
    LiveGridWithoutFilters,
    FineGridWithoutFilters,
    AirRiskMargin,
    ArrivalTooLate,
    NoSampleWithinReachHeight,
    MissingInputs,
}

impl InfieldReach {
    pub fn label(self) -> &'static str {
        match self {
            Self::LiveGridAndFilters => "live_grid_and_filters",
            Self::LiveGridWithoutFilters => "live_grid_without_filters_only",
            Self::FineGridWithoutFilters => "fine_grid_without_filters_only",
            Self::AirRiskMargin => "air_risk_margin",
            Self::ArrivalTooLate => "arrival_too_late",
            Self::NoSampleWithinReachHeight => "no_sample_within_reach_height",
            Self::MissingInputs => "missing_inputs",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct IntentAimInfield {
    pub selected_fielder: Position,
    pub selected_inside_50m: bool,
    pub selected_error: bool,
    pub projected_bounce_outside_50m: Option<bool>,
    pub infield_reach: InfieldReach,
}

pub(crate) fn intent_aim_infield(
    ball: &BattedBall,
    fielders: &[ActiveFielder],
    play: &FieldPlayPlayResult<'_>,
) -> Option<IntentAimInfield> {
    if ball.outbound_result != OutboundResult::InField
        || ball.is_foul()
        || !ball.launch_speed.is_finite()
        || ball.launch_speed < 45.0
        || !(5.0..15.0).contains(&ball.launch_angle)
    {
        return None;
    }
    let selected = play.result();
    let outfielder_touch = if selected.fielder.position.is_outfielder() {
        selected.pickup.map(|pickup| pickup.pickup_time_sec).or_else(|| {
            selected.error_type.is_none().then_some(selected.catch_time_sec)
        })
    } else {
        None
    };
    let scan_until = outfielder_touch
        .filter(|time| time.is_finite() && *time >= 0.0)
        .unwrap_or(f64::INFINITY);
    let infield_reach = infield_candidates(ball, fielders, &play.candidate_diagnostics)
        .map_or(InfieldReach::MissingInputs, |inputs| {
            let scan = find_opportunities(ball, &inputs, scan_until);
            match (scan.opportunities, scan.best_any) {
                ([Some(_), _, _], _) => InfieldReach::LiveGridAndFilters,
                ([_, Some(_), _], _) => InfieldReach::LiveGridWithoutFilters,
                ([_, _, Some(_)], _) => InfieldReach::FineGridWithoutFilters,
                (_, Some(gap)) if gap.delay_sec <= 0.0 => InfieldReach::AirRiskMargin,
                (_, Some(_)) => InfieldReach::ArrivalTooLate,
                _ => InfieldReach::NoSampleWithinReachHeight,
            }
        });
    Some(IntentAimInfield {
        selected_fielder: selected.fielder.position,
        selected_inside_50m: selected.catch_distance_m < INFIELD_DISTANCE,
        selected_error: selected.error_type.is_some(),
        projected_bounce_outside_50m: ball.first_bounce_position
            .filter(|point| point.distance.is_finite())
            .map(|point| point.distance >= INFIELD_DISTANCE),
        infield_reach,
    })
}

#[derive(Default, Serialize)]
struct GapSummary {
    samples: u64,
    no_sample: u64,
    delay_bands_sec: BTreeMap<&'static str, u64>,
    by_position: BTreeMap<String, u64>,
    ball_time_sum_sec: f64,
    prep_time_sum_sec: f64,
    travel_time_sum_sec: f64,
    delay_sum_sec: f64,
}

impl GapSummary {
    fn add(&mut self, point: Option<ArrivalGap>) {
        let Some(point) = point else {
            self.no_sample += 1;
            return;
        };
        self.samples += 1;
        let band = match point.delay_sec {
            delay if delay <= 0.0 => "on_time",
            delay if delay <= 0.1 => "(0,0.1]",
            delay if delay <= 0.25 => "(0.1,0.25]",
            delay if delay <= 0.5 => "(0.25,0.5]",
            delay if delay <= 1.0 => "(0.5,1]",
            _ => ">1",
        };
        *self.delay_bands_sec.entry(band).or_default() += 1;
        *self
            .by_position
            .entry(point.position.as_ref().into())
            .or_default() += 1;
        self.ball_time_sum_sec += point.ball_sec;
        self.prep_time_sum_sec += point.prep_sec;
        self.travel_time_sum_sec += point.travel_sec;
        self.delay_sum_sec += point.delay_sec;
    }
}

#[derive(Default, Serialize)]
struct ArrivalGaps {
    best_any: GapSummary,
    best_after_bounce: GapSummary,
}

impl ArrivalGaps {
    fn add(&mut self, scan: ScanResult<'_>) {
        self.best_any.add(scan.best_any);
        self.best_after_bounce.add(scan.best_after_bounce);
    }
}

fn band(value: f64, edges: &[f64]) -> String {
    if !value.is_finite() {
        return "unknown".into();
    }
    match edges.iter().position(|edge| value < *edge) {
        Some(0) => format!("<{}", edges[0]),
        Some(i) => format!("[{},{})", edges[i - 1], edges[i]),
        None => format!(">={}", edges.last().unwrap()),
    }
}

#[derive(Default, Serialize)]
struct Cohort {
    singles: u64,
    missing_inputs: u64,
    by_selection_path: BTreeMap<String, u64>,
    scenarios: BTreeMap<&'static str, Scenario>,
    arrival_gaps: ArrivalGaps,
    arrival_gaps_by_ev_hla: BTreeMap<String, ArrivalGaps>,
}

impl Cohort {
    fn add(
        &mut self,
        diagnostics: FieldingCatchDiagnostics,
        scan: Option<ScanResult<'_>>,
        speed_direction: &str,
        fielders: &[ActiveFielder],
        runner_first: Option<f64>,
    ) {
        self.singles += 1;
        *self
            .by_selection_path
            .entry(diagnostics.selection_path.as_ref().into())
            .or_default() += 1;
        let Some(scan) = scan else {
            self.missing_inputs += 1;
            return;
        };
        self.arrival_gaps.add(scan);
        self.arrival_gaps_by_ev_hla
            .entry(speed_direction.into())
            .or_default()
            .add(scan);
        for (name, opportunity) in SCENARIOS.into_iter().zip(scan.opportunities) {
            let scenario = self.scenarios.entry(name).or_default();
            if let Some(opportunity) = opportunity {
                scenario.add(opportunity, fielders, runner_first);
            }
        }
    }
}

#[derive(Default, Serialize)]
struct GrounderOutcome {
    plays: u64,
    missing_inputs: u64,
    arrival_gaps: ArrivalGaps,
    scenarios: BTreeMap<&'static str, Scenario>,
}

#[derive(Default, Serialize)]
struct FirstBaseRecovery {
    plays: u64,
    missing_inputs: u64,
    moving_interception_out: u64,
    stationary_recovery_out: u64,
    stop_outside_radius: u64,
    stop_inside_radius_first_late_or_tied: u64,
    stationary_completion_unavailable: u64,
    additional_stationary_out: u64,
    additional_stationary_examples: Vec<StationaryExample>,
}

impl FirstBaseRecovery {
    fn record_stationary(
        &mut self,
        play_key: (u32, u16),
        ball: &BattedBall,
        completion: Option<(Opportunity<'_>, f64)>,
        runner: f64,
        moving_out: bool,
    ) {
        match completion {
            Some((pickup, first)) if first < runner => {
                self.stationary_recovery_out += 1;
                if !moving_out {
                    self.additional_stationary_out += 1;
                    if self.additional_stationary_examples.len() < 4 {
                        self.additional_stationary_examples.push(StationaryExample {
                            game_id: play_key.0,
                            count_seq: play_key.1,
                            stop_distance_m: pickup.position.distance,
                            ball_stops_sec: ball.total_time,
                            fielder: pickup.fielder.position,
                            fielding_complete_sec: pickup.time_sec,
                            defense_first_sec: first,
                            batter_first_sec: runner,
                        });
                    }
                }
            }
            Some(_) => self.stop_inside_radius_first_late_or_tied += 1,
            None if ball.final_position.distance.is_finite()
                && ball.final_position.distance >= INFIELD_DISTANCE =>
            {
                self.stop_outside_radius += 1;
            }
            None => self.stationary_completion_unavailable += 1,
        }
    }
}

#[derive(Serialize)]
struct StationaryExample {
    game_id: u32,
    count_seq: u16,
    stop_distance_m: f64,
    ball_stops_sec: f64,
    fielder: Position,
    fielding_complete_sec: f64,
    defense_first_sec: f64,
    batter_first_sec: f64,
}

fn stationary_first_completion<'a>(
    ball: &BattedBall,
    candidates: &[(&'a ActiveFielder, &'a FielderCandidateDiagnostics)],
    fielders: &[ActiveFielder],
) -> Option<(Opportunity<'a>, f64)> {
    let position = ball.final_position;
    if !position.distance.is_finite() || position.distance >= INFIELD_DISTANCE {
        return None;
    }
    candidates
        .iter()
        .filter_map(|&(fielder, candidate)| {
            let arrival = fielder.info.prep_time
                + calculate_polar_distance(&fielder.polar_position, &position)
                    / fielder.info.running_speed;
            // Assume clean handling of a stationary ball and reuse live pickup timing.
            let handling = pickup_handling(fielder.info.catching, false, 1.0).0;
            let opportunity = Opportunity {
                fielder,
                candidate,
                time_sec: arrival.max(ball.total_time) + handling,
                position,
                direct: false,
            };
            first_completion(opportunity, fielders).map(|first| (opportunity, first))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
}

fn matches_live_flight(ball: &BattedBall, live: &BattedBall) -> bool {
    ball.outbound_result == live.outbound_result
        && (ball.total_time - live.total_time).abs() <= 1e-9
        && calculate_polar_distance(&ball.final_position, &live.final_position) <= 1e-8
        && ball.first_bounce_time == live.first_bounce_time
}

#[derive(Default, Serialize)]
struct FirstBounceComparison {
    samples: u64,
    missing_bounce: u64,
    distance_sum_m: f64,
    time_sum_sec: f64,
    exit_speed_sum_ms: f64,
    vertical_angle_sum_deg: f64,
    spin_rate_sum_rpm: f64,
    spin_toward_backspin_sum_rpm: f64,
    distance_bands_m: BTreeMap<String, u64>,
    time_bands_sec: BTreeMap<String, u64>,
}

impl FirstBounceComparison {
    fn observe(&mut self, ball: &BattedBall) {
        let sample = ball.first_bounce_position.zip(ball.first_bounce_time);
        let Some((position, time)) = sample.filter(|(p, t)| {
            p.distance.is_finite() && p.distance >= 0.0
                && t.is_finite() && (0.0..=ball.total_time).contains(t)
        }) else {
            self.missing_bounce += 1;
            return;
        };
        self.samples += 1;
        self.distance_sum_m += position.distance;
        self.time_sum_sec += time;
        self.exit_speed_sum_ms += ball.launch_speed;
        self.vertical_angle_sum_deg += ball.launch_angle;
        self.spin_rate_sum_rpm += ball.spin_rate;
        // Same spin-angle convention as the live Magnus force: 0° backspin, 180° topspin.
        self.spin_toward_backspin_sum_rpm += ball.spin_rate * ball.spin_angle.to_radians().cos();
        *self.distance_bands_m
            .entry(band(position.distance, &[20.0, 30.0, 40.0, 50.0, 60.0, 70.0]))
            .or_default() += 1;
        *self.time_bands_sec
            .entry(band(time, &[0.5, 0.75, 1.0, 1.25, 1.5, 1.75]))
            .or_default() += 1;
    }
}

#[derive(Default, Serialize)]
struct GrounderAngleBand {
    plays: u64,
    by_result: BTreeMap<String, u64>,
    single_rate: f64,
}

impl GrounderAngleBand {
    fn observe(&mut self, result: BattingResult) {
        self.plays += 1;
        *self.by_result.entry(result.as_ref().into()).or_default() += 1;
        self.single_rate =
            self.by_result.get("Single").copied().unwrap_or_default() as f64 / self.plays as f64;
    }
}

const BACKSPIN_REDUCTION_RPM: f64 = 200.0;

#[derive(Default, Serialize)]
struct LaunchSensitivity {
    plays: u64,
    missing_inputs: u64,
    baseline_errors: u64,
    baseline_mismatch: u64,
    angle_minus_1_deg: BounceDeltas,
    backspin_minus_200_rpm: BounceDeltas,
}

#[derive(Default, Serialize)]
struct BounceDeltas {
    replay_errors: u64,
    outbound_result_changed: u64,
    missing_bounce: u64,
    paired_samples: u64,
    distance_delta_sum_m: f64,
    time_delta_sum_sec: f64,
    baseline_bounce_at_or_beyond_50m: u64,
    trial_bounce_at_or_beyond_50m: u64,
    moved_inside_50m: u64,
    moved_outside_50m: u64,
}

impl BounceDeltas {
    fn observe(&mut self, baseline: &BattedBall, trial: &BattedBall) {
        if trial.outbound_result != baseline.outbound_result {
            self.outbound_result_changed += 1;
            return;
        }
        let bounces = baseline.first_bounce_position.zip(baseline.first_bounce_time)
            .zip(trial.first_bounce_position.zip(trial.first_bounce_time));
        let Some(((before, before_time), (after, after_time))) = bounces else {
            self.missing_bounce += 1;
            return;
        };
        self.paired_samples += 1;
        self.distance_delta_sum_m += after.distance - before.distance;
        self.time_delta_sum_sec += after_time - before_time;
        let before_outside = before.distance >= INFIELD_DISTANCE;
        let after_outside = after.distance >= INFIELD_DISTANCE;
        self.baseline_bounce_at_or_beyond_50m += u64::from(before_outside);
        self.trial_bounce_at_or_beyond_50m += u64::from(after_outside);
        self.moved_inside_50m += u64::from(before_outside && !after_outside);
        self.moved_outside_50m += u64::from(!before_outside && after_outside);
    }
}

impl LaunchSensitivity {
    fn observe(&mut self, live: &BattedBall, hla: Option<f64>, stadium: &Stadium) {
        self.plays += 1;
        let Some(hla) = hla.filter(|hla| hla.is_finite() && live.spin_angle.is_finite()) else {
            self.missing_inputs += 1;
            return;
        };
        let replay = |launch: &BattedBall| replay_grounder_flight(launch, hla, stadium, 1.0, |_| {});
        let Ok(baseline) = replay(live) else {
            self.baseline_errors += 1;
            return;
        };
        if !matches_live_flight(&baseline, live)
            || baseline.first_bounce_position.map(|p| p.distance)
                != live.first_bounce_position.map(|p| p.distance)
        {
            self.baseline_mismatch += 1;
            return;
        }

        let lower_angle = BattedBall {
            launch_angle: live.launch_angle - 1.0,
            ..*live
        };
        let spin_angle = live.spin_angle.to_radians();
        let backspin = live.spin_rate * spin_angle.cos() - BACKSPIN_REDUCTION_RPM;
        let sidespin = live.spin_rate * spin_angle.sin();
        // Recompose the spin vector with the original signed sidespin component.
        let lower_backspin = BattedBall {
            spin_rate: backspin.hypot(sidespin),
            spin_angle: sidespin.atan2(backspin).to_degrees().rem_euclid(360.0),
            ..*live
        };
        for (launch, deltas) in [
            (lower_angle, &mut self.angle_minus_1_deg),
            (lower_backspin, &mut self.backspin_minus_200_rpm),
        ] {
            match replay(&launch) {
                Ok(trial) => deltas.observe(&baseline, &trial),
                Err(_) => deltas.replay_errors += 1,
            }
        }
    }
}

#[derive(Default, Serialize)]
struct PreBounceReach {
    plays: u64,
    first_bounce: FirstBounceComparison,
    missing_inputs: u64,
    replay_errors: u64,
    baseline_mismatch: u64,
    crossed_50m_before_bounce: u64,
    crossing_time_sum_sec: f64,
    crossing_height_sum_m: f64,
    crossing_height_bands: BTreeMap<String, u64>,
    crossing_best_arrival_margin_sum_sec: f64,
    crossing_above_all_reach_heights: u64,
    crossing_height_ok_but_late: u64,
    crossing_reachable: u64,
    height_ok_before_crossing: u64,
    reachable_before_crossing: u64,
    live_candidate_margin_ok_before_crossing: u64,
}

fn air_arrival_margin(sample: GrounderFlightSample, fielder: &ActiveFielder) -> f64 {
    sample.time_sec - fielder.info.prep_time
        - calculate_polar_distance(&fielder.polar_position, &sample.position)
            / fielder.info.running_speed
}

impl PreBounceReach {
    fn observe(
        &mut self,
        live: &BattedBall,
        hla: f64,
        stadium: &Stadium,
        candidates: &[(&ActiveFielder, &FielderCandidateDiagnostics)],
    ) {
        let mut crossing = None;
        let mut height_ok = false;
        let mut reachable = false;
        let mut live_candidate_margin_ok = false;
        let replay = replay_grounder_flight(live, hla, stadium, 1.0, |sample| {
            if sample.after_bounce || crossing.is_some() {
                return;
            }
            if sample.position.distance >= INFIELD_DISTANCE {
                crossing = Some(sample);
                return;
            }
            if sample.time_sec < 0.10 {
                return;
            }
            for &(fielder, candidate) in candidates {
                if sample.height_m > fielder.info.reach_height {
                    continue;
                }
                height_ok = true;
                let margin = air_arrival_margin(sample, fielder);
                reachable |= margin >= 0.0;
                live_candidate_margin_ok |= candidate.lane_eligible
                    && candidate.distance_eligible
                    && margin >= candidate.required_margin_sec;
            }
        });
        let Ok(ball) = replay else {
            self.replay_errors += 1;
            return;
        };
        if !matches_live_flight(&ball, live) {
            self.baseline_mismatch += 1;
            return;
        }
        let Some(crossing) = crossing else { return };
        self.crossed_50m_before_bounce += 1;
        self.crossing_time_sum_sec += crossing.time_sec;
        self.crossing_height_sum_m += crossing.height_m;
        *self.crossing_height_bands
            .entry(band(crossing.height_m, &[1.0, 2.0, 3.0, 4.0]))
            .or_default() += 1;
        // Inputs guarantee at least one infielder with valid timing parameters.
        let mut fastest_margin = f64::NEG_INFINITY;
        let mut height_eligible_margin = f64::NEG_INFINITY;
        for &(fielder, _) in candidates {
            let margin = air_arrival_margin(crossing, fielder);
            fastest_margin = fastest_margin.max(margin);
            if crossing.height_m <= fielder.info.reach_height {
                height_eligible_margin = height_eligible_margin.max(margin);
            }
        }
        self.crossing_best_arrival_margin_sum_sec += fastest_margin;
        if height_eligible_margin >= 0.0 {
            self.crossing_reachable += 1;
        } else if height_eligible_margin.is_finite() {
            self.crossing_height_ok_but_late += 1;
        } else {
            self.crossing_above_all_reach_heights += 1;
        }
        self.height_ok_before_crossing += u64::from(height_ok);
        self.reachable_before_crossing += u64::from(reachable);
        self.live_candidate_margin_ok_before_crossing += u64::from(live_candidate_margin_ok);
    }
}

#[derive(Default, Serialize)]
struct GrounderFlightSweep {
    missing_inputs: u64,
    baseline_mismatch: u64,
    trials: BTreeMap<&'static str, GrounderFlightTotals>,
}

#[derive(Default, Serialize)]
struct GrounderFlightTotals {
    replay_errors: u64,
    out_of_play: u64,
    fair_plays: u64,
    first_bounce_samples: u64,
    first_bounce_time_sum_sec: f64,
    first_bounce_speed_sum_ms: f64,
    crossed_50m: u64,
    crossed_50m_after_bounce: u64,
    crossing_time_sum_sec: f64,
    crossing_speed_sum_ms: f64,
    bounce_to_crossing_time_sum_sec: f64,
    stopped_inside_50m: u64,
    post_bounce_reachable_before_50m: u64,
    post_bounce_first_out: u64,
    stationary_first_out: u64,
    moving_or_stationary_first_out: u64,
}

impl GrounderFlightSweep {
    fn observe(
        &mut self,
        live: &BattedBall,
        hla: f64,
        stadium: &Stadium,
        candidates: &[(&ActiveFielder, &FielderCandidateDiagnostics)],
        fielders: &[ActiveFielder],
        runner: f64,
    ) {
        for (label, scale) in [("1.0", 1.0), ("1.2", 1.2), ("1.4", 1.4)] {
            let totals = self.trials.entry(label).or_default();
            let mut bounce: Option<GrounderFlightSample> = None;
            let mut crossing = None;
            let mut reachable = false;
            let mut moving_out = false;
            let replay = replay_grounder_flight(live, hla, stadium, scale, |sample| {
                if sample.after_bounce && bounce.is_none() {
                    bounce = Some(sample);
                }
                if crossing.is_none() && sample.position.distance >= INFIELD_DISTANCE {
                    crossing = Some(sample);
                }
                if !sample.after_bounce || crossing.is_some() || sample.time_sec < 0.10 {
                    return;
                }
                for &(fielder, candidate) in candidates {
                    let arrival = fielder.info.prep_time
                        + calculate_polar_distance(&fielder.polar_position, &sample.position)
                            / fielder.info.running_speed;
                    if arrival > sample.time_sec || sample.height_m > fielder.info.reach_height {
                        continue;
                    }
                    reachable = true;
                    let interception = Opportunity {
                        fielder,
                        candidate,
                        time_sec: sample.time_sec,
                        position: sample.position,
                        direct: false,
                    };
                    moving_out |= first_completion(interception, fielders)
                        .is_some_and(|first| first < runner);
                }
            });
            let Ok(ball) = replay else {
                totals.replay_errors += 1;
                if scale == 1.0 {
                    break;
                }
                continue;
            };
            if scale == 1.0 && !matches_live_flight(&ball, live) {
                self.baseline_mismatch += 1;
                break;
            }
            if ball.outbound_result != OutboundResult::InField {
                totals.out_of_play += 1;
                continue;
            }
            totals.fair_plays += 1;
            if let Some(bounce) = bounce {
                totals.first_bounce_samples += 1;
                totals.first_bounce_time_sum_sec += bounce.time_sec;
                totals.first_bounce_speed_sum_ms += bounce.horizontal_speed_ms;
            }
            if let Some(crossing) = crossing {
                totals.crossed_50m += 1;
                totals.crossing_time_sum_sec += crossing.time_sec;
                totals.crossing_speed_sum_ms += crossing.horizontal_speed_ms;
                if let Some(bounce) = bounce.filter(|b| b.time_sec <= crossing.time_sec) {
                    totals.crossed_50m_after_bounce += 1;
                    totals.bounce_to_crossing_time_sum_sec += crossing.time_sec - bounce.time_sec;
                }
            }
            totals.stopped_inside_50m += u64::from(ball.final_position.distance < INFIELD_DISTANCE);
            let stationary_out = stationary_first_completion(&ball, candidates, fielders)
                .is_some_and(|(_, first)| first < runner);
            totals.post_bounce_reachable_before_50m += u64::from(reachable);
            totals.post_bounce_first_out += u64::from(moving_out);
            totals.stationary_first_out += u64::from(stationary_out);
            totals.moving_or_stationary_first_out += u64::from(moving_out || stationary_out);
        }
    }
}

#[derive(Default, Serialize)]
pub struct InfieldOpportunityAudit {
    hard_low_grounder_half_degree_bands: BTreeMap<String, GrounderAngleBand>,
    hard_low_grounder_launch_sensitivity: BTreeMap<String, BTreeMap<String, LaunchSensitivity>>,
    hard_low_pre_bounce_reach_by_ev_vla_hla_and_result:
        BTreeMap<String, BTreeMap<String, PreBounceReach>>,
    hard_low_single_ground_friction_sweep: GrounderFlightSweep,
    hard_low_grounder_first_base_recovery: BTreeMap<&'static str, FirstBaseRecovery>,
    hard_low_grounder_outs_by_live_target: BTreeMap<String, u64>,
    unreproduced_hard_low_out_count: u64,
    unreproduced_hard_low_out_examples: Vec<OutReplayExample>,
    hard_low_grounders_by_ev_vla_hla_and_result:
        BTreeMap<String, BTreeMap<String, GrounderOutcome>>,
    outfield_pickup_grounder_singles: Cohort,
    hard_low_grounder_subset: Cohort,
}

thread_local! {
    static ACTIVE: RefCell<Option<InfieldOpportunityAudit>> = const { RefCell::new(None) };
}

impl InfieldOpportunityAudit {
    pub fn observe_game<T>(&mut self, run: impl FnOnce() -> T) -> T {
        struct Restore<'a> {
            previous: Option<InfieldOpportunityAudit>,
            target: &'a mut InfieldOpportunityAudit,
        }
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                *self.target = ACTIVE.replace(self.previous.take()).unwrap();
            }
        }
        let previous = ACTIVE.replace(Some(std::mem::take(self)));
        let _restore = Restore {
            previous,
            target: self,
        };
        run()
    }
}

#[derive(Serialize)]
struct OutReplayExample {
    game_id: u32,
    count_seq: u16,
    live_fielder: Position,
    catch_type: CatchType,
    error_type: Option<crate::domain::resolver::fielding_physics::FieldError>,
    live_position: PolarPosition,
    live_fielding_complete_sec: f64,
    observed_control_sec: Option<f64>,
    model_at_observed_control: Option<ReplayPoint>,
    diagnostic_interception: Option<(Position, f64, PolarPosition)>,
    diagnostic_first_completion_sec: Option<f64>,
    batter_first_sec: Option<f64>,
    live_throw_target: Base,
    live_defense_sec: f64,
}

#[derive(Serialize)]
struct ReplayPoint {
    position: PolarPosition,
    height_m: f64,
    position_error_m: f64,
    inside_search_radius: bool,
    height_reachable: bool,
    fielder_arrival_minus_ball_sec: f64,
}

pub(crate) fn record(
    ctx: &PlayContext<'_>,
    stadium: &Stadium,
    play_key: (u32, u16),
    defense: &crate::domain::resolver::fielding_resolver::DefensePlayResult,
    batting_side: RL,
    diagnostics: FieldingCatchDiagnostics,
    candidates: &[FielderCandidateDiagnostics],
    result: BattingResult,
    launch_direction_deg: Option<f64>,
) {
    ACTIVE.with_borrow_mut(|active| {
        let Some(audit) = active else { return };
        let ball = &ctx.fielded_ball.ball;
        if ball.trajectory() != TrajectoryType::Grounder {
            return;
        }
        let pickup_single = result == BattingResult::Single
            && diagnostics.fielder_position.is_outfielder()
            && diagnostics.catch_type == CatchType::FinalPickup;
        let hard_low = ball.launch_speed.is_finite()
            && ball.launch_speed >= 45.0
            && (0.0..10.0).contains(&ball.launch_angle);
        let hard_low_comparison =
            hard_low && matches!(result, BattingResult::Single | BattingResult::Out);
        if !pickup_single && !hard_low {
            return;
        }
        let ev = band(
            ball.launch_speed,
            &[20.0, 30.0, 35.0, 40.0, 95.0 * 0.44704, 45.0, 50.0, 55.0],
        );
        let hla = band(
            launch_direction_deg.unwrap_or(f64::NAN),
            &[-45.0, -30.0, -15.0, -5.0, 5.0, 15.0, 30.0, 45.0],
        );
        if hard_low {
            const HALF_DEGREE_EDGES: [f64; 21] = [
                0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5, 4.0, 4.5, 5.0,
                5.5, 6.0, 6.5, 7.0, 7.5, 8.0, 8.5, 9.0, 9.5, 10.0,
            ];
            let vla = band(ball.launch_angle, &HALF_DEGREE_EDGES);
            audit.hard_low_grounder_half_degree_bands
                .entry(format!("EV:{ev}|VLA:{vla}|HLA:{hla}"))
                .or_default()
                .observe(result);
            if hard_low_comparison && ball.launch_angle >= 5.0 {
                let vla = band(ball.launch_angle, &[5.0, 7.5, 10.0]);
                audit.hard_low_grounder_launch_sensitivity
                    .entry(format!("EV:{ev}|VLA:{vla}|HLA:{hla}"))
                    .or_default()
                    .entry(result.as_ref().into())
                    .or_default()
                    .observe(ball, launch_direction_deg, stadium);
            }
        }
        if !pickup_single && !hard_low_comparison {
            return;
        }
        let runner_first = ctx
            .runners
            .batter_runner_time_to(Base::First, true, batting_side)
            .ok()
            .filter(|t| t.is_finite() && *t > 0.0);
        let inputs = infield_candidates(ball, ctx.fielders, candidates);
        if hard_low_comparison && result == BattingResult::Single {
            let sweep = &mut audit.hard_low_single_ground_friction_sweep;
            match (
                inputs.as_deref(),
                launch_direction_deg.filter(|hla| hla.is_finite()),
                runner_first,
            ) {
                (Some(inputs), Some(hla), Some(runner)) => {
                    sweep.observe(ball, hla, stadium, inputs, ctx.fielders, runner);
                }
                _ => sweep.missing_inputs += 1,
            }
        }
        let speed_direction = format!("EV:{ev}|HLA:{hla}");
        if hard_low_comparison {
            let vla = band(ball.launch_angle, &[0.0, 2.5, 5.0, 7.5, 10.0]);
            let band_key = format!("EV:{ev}|VLA:{vla}|HLA:{hla}");
            let air = audit
                .hard_low_pre_bounce_reach_by_ev_vla_hla_and_result
                .entry(band_key.clone())
                .or_default()
                .entry(result.as_ref().into())
                .or_default();
            air.plays += 1;
            // Compare every recorded first bounce in this cohort, including balls
            // that never reached 50 m before bouncing.
            air.first_bounce.observe(ball);
            match (inputs.as_deref(), launch_direction_deg.filter(|hla| hla.is_finite())) {
                (Some(inputs), Some(hla)) => air.observe(ball, hla, stadium, inputs),
                _ => air.missing_inputs += 1,
            }
            let group = audit
                .hard_low_grounders_by_ev_vla_hla_and_result
                .entry(band_key)
                .or_default()
                .entry(result.as_ref().into())
                .or_default();
            let recovery_group = if result == BattingResult::Single {
                Some("single")
            } else {
                *audit
                    .hard_low_grounder_outs_by_live_target
                    .entry(defense.throw_target_base.as_ref().into())
                    .or_default() += 1;
                (defense.throw_target_base == Base::First).then_some("live_first_out")
            };
            let recovery = recovery_group.map(|key| {
                let row = audit
                    .hard_low_grounder_first_base_recovery
                    .entry(key)
                    .or_default();
                row.plays += 1;
                row
            });
            group.plays += 1;
            if let Some(inputs) = &inputs {
                // Both outcomes use the projected path through the infield,
                // without stopping at their different observed fielding times.
                let scan = find_opportunities(ball, inputs, f64::INFINITY);
                if let Some(recovery) = recovery {
                    if let Some(runner) = runner_first {
                        let moving_out = scan.opportunities[2].is_some_and(|p| {
                            p.direct
                                || first_completion(p, ctx.fielders)
                                    .is_some_and(|first| first < runner)
                        });
                        recovery.moving_interception_out += u64::from(moving_out);
                        recovery.record_stationary(
                            play_key,
                            ball,
                            stationary_first_completion(ball, inputs, ctx.fielders),
                            runner,
                            moving_out,
                        );
                    } else {
                        recovery.missing_inputs += 1;
                    }
                }
                group.arrival_gaps.add(scan);
                if result == BattingResult::Out {
                    let fine = scan.opportunities[2];
                    let diagnostic_first = fine
                        .filter(|p| !p.direct)
                        .and_then(|p| first_completion(p, ctx.fielders));
                    let reproduced = fine.is_some_and(|p| p.direct)
                        || diagnostic_first.zip(runner_first)
                            .is_some_and(|(defense, runner)| defense < runner);
                    if !reproduced {
                        audit.unreproduced_hard_low_out_count += 1;
                        if audit.unreproduced_hard_low_out_examples.len() < 4 {
                            let live_position = ctx.fielded_ball.catch_position;
                            let fielder = ctx.try_catch_fielder;
                            let control = diagnostics.pickup.map(|p| p.pickup_time_sec)
                                .or_else(|| diagnostics.error_type.is_none()
                                    .then_some(diagnostics.catch_time_sec));
                            let point = control.filter(|time| {
                                time.is_finite() && *time >= 0.0
                            }).map(|time| {
                                let (distance, angle, _, _, height, _) =
                                    estimate_ball_state_at_time(time, ball);
                                let position = PolarPosition::new(distance, angle);
                                ReplayPoint {
                                    position,
                                    height_m: height,
                                    position_error_m: calculate_polar_distance(&live_position, &position),
                                    inside_search_radius: distance < INFIELD_DISTANCE,
                                    height_reachable: height <= fielder.info.reach_height,
                                    fielder_arrival_minus_ball_sec: fielder.info.prep_time
                                        + calculate_polar_distance(&fielder.polar_position, &position)
                                            / fielder.info.running_speed
                                        - time,
                                }
                            });
                            audit.unreproduced_hard_low_out_examples.push(OutReplayExample {
                                game_id: play_key.0,
                                count_seq: play_key.1,
                                live_fielder: fielder.position,
                                catch_type: diagnostics.catch_type,
                                error_type: diagnostics.error_type,
                                live_position,
                                live_fielding_complete_sec: diagnostics.catch_time_sec,
                                observed_control_sec: control,
                                model_at_observed_control: point,
                                diagnostic_interception: fine.map(|p| (p.fielder.position, p.time_sec, p.position)),
                                diagnostic_first_completion_sec: diagnostic_first,
                                batter_first_sec: runner_first,
                                live_throw_target: defense.throw_target_base,
                                live_defense_sec: defense.defense_time,
                            });
                        }
                    }
                }
                for (name, opportunity) in SCENARIOS.into_iter().zip(scan.opportunities) {
                    let scenario = group.scenarios.entry(name).or_default();
                    if let Some(opportunity) = opportunity {
                        scenario.add(opportunity, ctx.fielders, runner_first);
                    }
                }
            } else {
                group.missing_inputs += 1;
                if let Some(recovery) = recovery {
                    recovery.missing_inputs += 1;
                }
            }
        }
        if !pickup_single {
            return;
        }
        let opportunities = inputs.as_ref().and_then(|inputs| {
            let pickup = diagnostics.pickup?;
            let t = pickup.pickup_time_sec;
            (t.is_finite() && t >= 0.0).then(|| find_opportunities(ball, inputs, t))
        });
        audit.outfield_pickup_grounder_singles.add(
            diagnostics,
            opportunities,
            &speed_direction,
            ctx.fielders,
            runner_first,
        );
        if (45.0..50.0).contains(&ball.launch_speed) && (0.0..10.0).contains(&ball.launch_angle) {
            audit.hard_low_grounder_subset.add(
                diagnostics,
                opportunities,
                &speed_direction,
                ctx.fielders,
                runner_first,
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::test_support::active_fielder;

    #[test]
    fn separates_candidate_filters_from_post_bounce_sampling() {
        let mut ball = BattedBall::default();
        ball.total_time = 1.0;
        ball.first_bounce_time = Some(0.0);
        ball.first_bounce_position = Some(PolarPosition::new(0.0, 0.0));
        ball.final_position = PolarPosition::new(100.0, 0.0);
        let mut fielder = active_fielder(Position::SS, 43.0, 0.0);
        fielder.info.running_speed = 2.0;
        fielder.info.prep_time = 0.4;
        let candidate = FielderCandidateDiagnostics {
            fielder_id: fielder.id,
            fielder_position: fielder.position,
            lane_eligible: false,
            distance_eligible: true,
            required_margin_sec: 0.0,
            search: None,
            air_probe: None,
            air_probe_coarse: None,
            interception_catch_type: None,
            interception_error_type: None,
            interception_time_sec: None,
            selected: false,
            selected_path: None,
        };
        let [live, relaxed, fine] =
            find_opportunities(&ball, &[(&fielder, &candidate)], 0.9).opportunities;
        assert!(live.is_none() && relaxed.is_none());
        assert!((fine.unwrap().time_sec - 0.43).abs() < 1e-9);

        fielder.polar_position = PolarPosition::new(45.0, 0.0);
        let [live, relaxed, fine] =
            find_opportunities(&ball, &[(&fielder, &candidate)], 0.9).opportunities;
        assert!(live.is_none());
        assert!((relaxed.unwrap().time_sec - 0.45).abs() < 1e-9);
        assert!((fine.unwrap().time_sec - 0.45).abs() < 1e-9);

        fielder.polar_position = PolarPosition::new(52.0, 0.0);
        let scan = find_opportunities(&ball, &[(&fielder, &candidate)], 0.9);
        let best = scan.best_after_bounce.unwrap();
        assert_eq!(best.position, Position::SS);
        assert!((best.ball_sec - 0.49).abs() < 1e-9);
        assert!((best.delay_sec - 1.41).abs() < 1e-9);
        assert!(
            find_opportunities(&ball, &[(&fielder, &candidate)], 0.9)
                .opportunities
                .iter()
                .all(Option::is_none)
        );
    }
}
