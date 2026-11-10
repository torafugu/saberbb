//! Read-only aggregation of completed GameResult records. Never draws RNG or reruns a play.
use saberbb::domain::{
    resolver::{
        fielding_physics::{
            CatchType, FieldingCatchDiagnostics, PickupTimingDiagnostics,
            RightFieldFirstBaseSnapshot, estimate_ball_state_at_time,
        },
        fielding_timing::DefenseTimeCalculator,
        running_resolver::{RunnersOnBase, RunningEvent},
    },
    shared::{
        ball::OutboundResult,
        game::{BattingResult, GameResult, GameSchedule},
        game_state::{ActiveFielder, ActiveRunner, Ruling},
        game_stats::{PlayerGameBatting, PlayerGameFielding, PlayerGameRunning},
        player::{Player, Position},
        stadium::Base,
    },
    util::{PolarPosition, calculate_polar_distance},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const HARD_HIT_MPH: f64 = 95.0;
const HARD_HIT_MS: f64 = HARD_HIT_MPH * 0.44704;
const CONVERSION_EV_EDGES_MS: &[f64] = &[20.0, 30.0, 35.0, 40.0, HARD_HIT_MS, 45.0, 50.0, 55.0];
const CONVERSION_HLA_EDGES_DEG: &[f64] = &[-45.0, -30.0, -15.0, -5.0, 5.0, 15.0, 30.0, 45.0];
const LENGTH_EDGES_M: &[f64] = &[0.02, 0.05, 0.10, 0.16];
const THICKNESS_EDGES_M: &[f64] = &[0.005, 0.01, 0.02, 0.025, 0.04, 0.055, 0.07];

#[derive(Default, Serialize)]
struct Moments {
    n: u64,
    mean: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
}
impl Moments {
    fn add(&mut self, v: Option<f64>) {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            self.n += 1;
            self.mean = Some(self.mean.unwrap_or(v) + (v - self.mean.unwrap_or(v)) / self.n as f64);
            self.min = Some(self.min.map_or(v, |x| x.min(v)));
            self.max = Some(self.max.map_or(v, |x| x.max(v)));
        }
    }
}
#[derive(Default, Serialize)]
struct Group {
    n: u64,
    hits: u64,
    singles: u64,
    doubles: u64,
    triples: u64,
    results: BTreeMap<String, u64>,
    ev_ms: Moments,
    vla_deg: Moments,
    field_time_sec: Moments,
    defense_time_sec: Moments,
    field_to_defense_phase_sec: Moments,
    target_runner_margin_sec: Moments,
}
impl Group {
    fn add(
        &mut self,
        result: BattingResult,
        ev: f64,
        vla: f64,
        field: Option<f64>,
        defense: Option<f64>,
        margin: Option<f64>,
    ) {
        self.n += 1;
        self.hits += u64::from(is_hit(result));
        self.singles += u64::from(result == BattingResult::Single);
        self.doubles += u64::from(result == BattingResult::Double);
        self.triples += u64::from(result == BattingResult::Triple);
        *self.results.entry(result.as_ref().into()).or_default() += 1;
        self.ev_ms.add(Some(ev));
        self.vla_deg.add(Some(vla));
        self.field_time_sec.add(field);
        self.defense_time_sec.add(defense);
        self.field_to_defense_phase_sec
            .add(field.zip(defense).map(|(f, d)| d - f));
        self.target_runner_margin_sec.add(margin);
    }
    fn json(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap();
        for (key, count) in [
            ("hit_rate", self.hits),
            ("single_rate", self.singles),
            ("out_on_play_rate", self.n - self.hits),
        ] {
            v[key] = json!((self.n > 0).then(|| count as f64 / self.n as f64));
        }
        v
    }
}
fn is_hit(r: BattingResult) -> bool {
    matches!(
        r,
        BattingResult::Single | BattingResult::Double | BattingResult::Triple
    )
}
fn is_bip(r: BattingResult) -> bool {
    matches!(
        r,
        BattingResult::Single
            | BattingResult::Double
            | BattingResult::Triple
            | BattingResult::Out
            | BattingResult::FieldersChoice
            | BattingResult::DoublePlay
    )
}
fn band(value: f64, edges: &[f64]) -> String {
    if !value.is_finite() {
        return "unknown".into();
    }
    for (i, edge) in edges.iter().enumerate() {
        if value < *edge {
            return if i == 0 {
                format!("<{edge}")
            } else {
                format!("[{},{})", edges[i - 1], edge)
            };
        }
    }
    format!(">={}", edges.last().unwrap())
}
fn actual_margin(target: Option<i64>, defense: f64, runner: f64) -> Option<f64> {
    (target.is_some()
        && defense.is_finite()
        && runner.is_finite()
        && defense >= 0.0
        && runner > 0.0)
        .then_some(defense - runner)
}
#[derive(Default, Serialize)]
struct ContactPopulation {
    n: u64,
    bip: u64,
    fouls: u64,
    hr: u64,
    ev_ms: Moments,
    length_offset_m: Moments,
    thickness_offset_m: Moments,
    abs_adapted_timing_ms: Moments,
}
#[derive(Default)]
struct ExitSpeedGroup {
    ev_ms: Moments,
    hard_hits: u64,
}
impl ExitSpeedGroup {
    fn add(&mut self, speed: f64) {
        if speed.is_finite() && speed >= 0.0 {
            self.ev_ms.add(Some(speed));
            self.hard_hits += u64::from(speed >= HARD_HIT_MS);
        }
    }

    fn json(&self) -> Value {
        json!({
            "ev_ms": self.ev_ms,
            "hard_hits": self.hard_hits,
            "hard_hit_rate": (self.ev_ms.n > 0)
                .then(|| self.hard_hits as f64 / self.ev_ms.n as f64),
        })
    }
}

#[derive(Default)]
struct ContactOffsetCell {
    contacts: u64,
    fouls: u64,
    bip: u64,
    home_runs: u64,
    other_results: u64,
    by_contact_quality: BTreeMap<String, u64>,
    all_contact_ev: ExitSpeedGroup,
    bip_plus_hr_ev: ExitSpeedGroup,
}
impl ContactOffsetCell {
    fn add(&mut self, quality: &str, result: BattingResult, speed: f64) {
        self.contacts += 1;
        *self.by_contact_quality.entry(quality.into()).or_default() += 1;
        self.all_contact_ev.add(speed);
        match result {
            BattingResult::Foul => self.fouls += 1,
            BattingResult::HomeRun => self.home_runs += 1,
            result if is_bip(result) => self.bip += 1,
            _ => self.other_results += 1,
        }
        if is_bip(result) || result == BattingResult::HomeRun {
            self.bip_plus_hr_ev.add(speed);
        }
    }

    fn json(&self) -> Value {
        json!({
            "contacts": self.contacts,
            "fouls": self.fouls,
            "foul_rate": (self.contacts > 0).then(|| self.fouls as f64 / self.contacts as f64),
            "bip": self.bip,
            "home_runs": self.home_runs,
            "other_results": self.other_results,
            "by_contact_quality": self.by_contact_quality,
            "all_contact_ev": self.all_contact_ev.json(),
            "bip_plus_hr_ev": self.bip_plus_hr_ev.json(),
        })
    }
}

#[derive(Default)]
struct ContactOffsetAudit {
    total: ContactOffsetCell,
    cells: BTreeMap<String, ContactOffsetCell>,
}
impl ContactOffsetAudit {
    fn add(
        &mut self,
        length: f64,
        thickness: Option<f64>,
        quality: &str,
        result: BattingResult,
        speed: f64,
    ) {
        let length = band(length.abs(), LENGTH_EDGES_M);
        let thickness = band(
            thickness.map(f64::abs).unwrap_or(f64::NAN),
            THICKNESS_EDGES_M,
        );
        self.total.add(quality, result, speed);
        self.cells
            .entry(format!("length_m:{length}|thickness_m:{thickness}"))
            .or_default()
            .add(quality, result, speed);
    }

    fn json(&self) -> Value {
        let cells: BTreeMap<_, _> = self
            .cells
            .iter()
            .map(|(k, cell)| (k, cell.json()))
            .collect();
        json!({
            "source": "contact_offset_ev_v1",
            "length_edges_m": LENGTH_EDGES_M,
            "thickness_edges_m": THICKNESS_EDGES_M,
            "hard_hit_threshold_mph": HARD_HIT_MPH,
            "hard_hit_threshold_ms": HARD_HIT_MS,
            "total": self.total.json(),
            "cells": cells,
            "definitions": [
                "Absolute length and thickness offsets in meters, grouped into half-open bands. Unknown offsets retain a separate bin.",
                "Population: recorded adjusted SolidContact, WeakContact and MarginalContact, including fouls and home runs. Swing misses are outside this denominator.",
                "bip_plus_hr_ev covers terminal BIP plus home runs. Rates use each EV group's finite nonnegative sample count (ev_ms.n); an empty denominator produces null.",
                "Foul rate uses all contacts in the cell. Counts sum to the total; unobserved cells are omitted."
            ],
        })
    }
}

#[derive(Default, Serialize)]
struct FirstBasePhaseGroup {
    n: u64,
    runner_first_sec: Moments,
    pickup_sec: Moments,
    handling_sec: Moments,
    field_complete_sec: Moments,
    delivery_after_field_sec: Moments,
    defense_complete_sec: Moments,
}

fn first_base_phase(
    runner: Option<f64>,
    pickup: Option<f64>,
    ready: Option<f64>,
    defense: Option<f64>,
) -> &'static str {
    let (Some(runner), Some(ready)) = (runner, ready) else {
        return "missing_clock";
    };
    if pickup.is_some_and(|t| t >= runner) {
        "pickup_at_or_after_runner"
    } else if ready >= runner {
        if pickup.is_some() {
            "handling_crossed_runner"
        } else {
            "field_complete_late_pickup_unknown"
        }
    } else if let Some(defense) = defense {
        if defense >= runner {
            "delivery_crossed_runner"
        } else {
            "defense_completed_before_runner"
        }
    } else {
        "field_complete_before_runner_delivery_unobserved"
    }
}

struct FirstBaseOpportunity {
    target: &'static str,
    runner_sec: f64,
    ready_sec: f64,
    pickup: Option<PickupTimingDiagnostics>,
}

#[derive(Default, Serialize)]
struct CoverScenario {
    modeled_out: u64,
    out_with_at_least_025_sec: u64,
    cover_later_than_ball: u64,
    completion_minus_runner_sec: Moments,
}

impl CoverScenario {
    fn add(&mut self, ball_arrival: f64, cover_arrival: f64, runner_arrival: f64) {
        let margin = ball_arrival.max(cover_arrival) - runner_arrival;
        self.modeled_out += u64::from(margin < 0.0);
        self.out_with_at_least_025_sec += u64::from(margin <= -0.25);
        self.cover_later_than_ball += u64::from(cover_arrival > ball_arrival);
        self.completion_minus_runner_sec.add(Some(margin));
    }
}

#[derive(Default, Serialize)]
struct RightFieldCoverTrial {
    eligible: u64,
    missing_snapshot: u64,
    invalid_snapshot: u64,
    estimated_origin_ideal_cover: CoverScenario,
    recorded_origin_ideal_cover: CoverScenario,
    cover_starts_at_contact: CoverScenario,
    cover_starts_at_catch: CoverScenario,
    estimated_to_recorded_position_m: Moments,
}

impl RightFieldCoverTrial {
    fn record(
        &mut self,
        opportunity: &FirstBaseOpportunity,
        snapshot: Option<RightFieldFirstBaseSnapshot>,
        estimated: &PolarPosition,
        thrower: &ActiveFielder,
        estimated_delivery: f64,
    ) {
        self.eligible += 1;
        let Some(snapshot) = snapshot else {
            self.missing_snapshot += 1;
            return;
        };
        let valid_position = |pos: PolarPosition| {
            pos.distance.is_finite() && pos.distance >= 0.0 && pos.angle.is_finite()
        };
        if !valid_position(snapshot.catch_position)
            || !valid_position(snapshot.cover_start)
            || !snapshot.cover_prep_sec.is_finite()
            || snapshot.cover_prep_sec < 0.0
            || !snapshot.cover_speed_ms.is_finite()
            || snapshot.cover_speed_ms <= 0.0
        {
            self.invalid_snapshot += 1;
            return;
        }
        let distance =
            calculate_polar_distance(&snapshot.cover_start, &Base::First.polar_position());
        let cover = snapshot.cover_prep_sec + distance / snapshot.cover_speed_ms;
        let ball = opportunity.ready_sec
            + DefenseTimeCalculator::default().direct_play_time(
                thrower,
                &snapshot.catch_position,
                Base::First,
                false,
            );
        let late_cover = opportunity.ready_sec + cover;
        if !ball.is_finite() || !late_cover.is_finite() {
            self.invalid_snapshot += 1;
            return;
        }
        let runner = opportunity.runner_sec;
        self.estimated_to_recorded_position_m
            .add(Some(calculate_polar_distance(
                estimated,
                &snapshot.catch_position,
            )));
        self.estimated_origin_ideal_cover.add(
            opportunity.ready_sec + estimated_delivery,
            0.0,
            runner,
        );
        self.recorded_origin_ideal_cover.add(ball, 0.0, runner);
        self.cover_starts_at_contact.add(ball, cover, runner);
        self.cover_starts_at_catch.add(ball, late_cover, runner);
    }
}

#[derive(Default, Serialize)]
struct FirstBaseTrialGroup {
    eligible: u64,
    missing: BTreeMap<&'static str, u64>,
    origin_sources: BTreeMap<&'static str, u64>,
    methods: BTreeMap<&'static str, u64>,
    modeled_out: u64,
    modeled_safe: u64,
    distance_to_first_m: Moments,
    completion_minus_runner_sec: Moments,
    margin_bands_sec: BTreeMap<String, u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    right_field_cover: Option<RightFieldCoverTrial>,
}

impl FirstBaseTrialGroup {
    fn record(
        &mut self,
        opportunity: FirstBaseOpportunity,
        b: &PlayerGameBatting,
        fields: &[&PlayerGameFielding],
        game: &GameResult,
        roster: &BTreeMap<i64, &Player>,
    ) {
        self.eligible += 1;
        let trial = (|| {
            let [f] = fields else {
                return Err("missing_or_ambiguous_primary_fielding");
            };
            let position = f.catch_fielder_position;
            let diag = f.catch_diagnostics.ok_or("missing_fielding_diagnostics")?;
            if diag.fielder_position != position {
                return Err("inconsistent_fielder_position");
            }
            let mut selected = game.player_fielding_candidates.iter().filter(|c| {
                c.count_seq == b.count_seq
                    && c.diagnostics.selected
                    && c.diagnostics.fielder_position == position
            });
            let id = selected
                .next()
                .ok_or("missing_selected_fielder")?
                .diagnostics
                .fielder_id;
            if selected.next().is_some() {
                return Err("ambiguous_selected_fielder");
            }
            let (origin, source) = match diag.catch_type {
                CatchType::FinalPickup => {
                    let pickup = opportunity
                        .pickup
                        .ok_or("missing_or_invalid_pickup_snapshot")?;
                    if pickup.fielder_id != id {
                        return Err("inconsistent_pickup_fielder");
                    }
                    (pickup.pickup_position, "recorded_pickup")
                }
                CatchType::BounceCatch
                    if diag.error_type.is_none()
                        && (diag.catch_time_sec - opportunity.ready_sec).abs() <= 1e-7 =>
                {
                    let (distance, angle, _, _, _, _) =
                        estimate_ball_state_at_time(diag.catch_time_sec, &b.ball);
                    (
                        PolarPosition::new(distance, angle),
                        "estimated_bounce_catch",
                    )
                }
                _ => return Err("unsupported_catch_origin"),
            };
            if !origin.distance.is_finite() || origin.distance < 0.0 || !origin.angle.is_finite() {
                return Err("invalid_origin");
            }
            let info = roster
                .get(&id)
                .and_then(|p| p.fielder().ok())
                .ok_or("missing_fielder_skills")?;
            if !info.prep_time.is_finite()
                || info.prep_time < 0.0
                || !info.throw_speed.is_finite()
                || info.throw_speed <= 0.0
                || (position.is_infielder()
                    && (!info.running_speed.is_finite() || info.running_speed <= 0.0))
            {
                return Err("invalid_fielder_skills");
            }
            let thrower = ActiveFielder::new(position, id, info);
            let calc = DefenseTimeCalculator::default();
            let base = Base::First.polar_position();
            let direct = calc.direct_play_time(&thrower, &origin, Base::First, false);
            let (delivery, method) = if position.is_infielder() {
                let carry = calc.run_to_base_time(&thrower, &origin, &base);
                if carry < direct {
                    (carry, "carry")
                } else {
                    (direct, "direct_throw")
                }
            } else if position.is_outfielder() {
                (direct, "direct_throw")
            } else {
                return Err("unsupported_fielder_position");
            };
            let margin = opportunity.ready_sec + delivery - opportunity.runner_sec;
            let distance = calculate_polar_distance(&origin, &base);
            if !margin.is_finite() || !distance.is_finite() {
                return Err("invalid_trial_time_or_distance");
            }
            if opportunity.target == "target_unrecorded"
                && position == Position::RF
                && source == "estimated_bounce_catch"
            {
                self.right_field_cover
                    .get_or_insert_with(RightFieldCoverTrial::default)
                    .record(
                        &opportunity,
                        diag.right_field_first_base,
                        &origin,
                        &thrower,
                        delivery,
                    );
            }
            Ok((source, method, distance, margin))
        })();
        let (source, method, distance, margin) = match trial {
            Ok(values) => values,
            Err(reason) => {
                *self.missing.entry(reason).or_default() += 1;
                return;
            }
        };
        *self.origin_sources.entry(source).or_default() += 1;
        *self.methods.entry(method).or_default() += 1;
        self.modeled_out += u64::from(margin < 0.0);
        self.modeled_safe += u64::from(margin >= 0.0);
        self.distance_to_first_m.add(Some(distance));
        self.completion_minus_runner_sec.add(Some(margin));
        *self
            .margin_bands_sec
            .entry(band(margin, &[-0.5, -0.25, 0.0, 0.25, 0.5]))
            .or_default() += 1;
    }
}

#[derive(Default, Serialize)]
struct HardGrounderFirstBase {
    n: u64,
    groups: BTreeMap<String, FirstBasePhaseGroup>,
    unobserved_first_trial: BTreeMap<String, FirstBaseTrialGroup>,
}

impl HardGrounderFirstBase {
    fn add(
        &mut self,
        result: BattingResult,
        batter_id: i64,
        primary: Option<&PlayerGameRunning>,
        modeled_first: Option<f64>,
        ready: Option<f64>,
        diag: Option<FieldingCatchDiagnostics>,
    ) -> Option<FirstBaseOpportunity> {
        let (target, first, defense) = match primary {
            Some(r)
                if r.throw_target_base == Base::First && r.target_runner_id == Some(batter_id) =>
            {
                (
                    "batter_first_observed",
                    Some(r.runner_time),
                    Some(r.defense_time),
                )
            }
            Some(r) if r.target_runner_id.is_some() => ("other_target", modeled_first, None),
            Some(_) => ("target_unrecorded", modeled_first, None),
            None => ("no_primary_grounder_record", modeled_first, None),
        };
        let first = first.filter(|t| t.is_finite() && *t > 0.0);
        let pickup = diag.and_then(|d| d.pickup).filter(|p| {
            p.pickup_time_sec.is_finite()
                && p.pickup_time_sec >= 0.0
                && p.handling_delay_sec.is_finite()
                && p.handling_delay_sec >= 0.0
                && ready
                    .is_some_and(|t| (p.pickup_time_sec + p.handling_delay_sec - t).abs() <= 1e-7)
        });
        let invalid_defense = defense
            .is_some_and(|t| !t.is_finite() || t < 0.0 || ready.is_some_and(|ready| t < ready))
            || primary
                .filter(|_| defense.is_some())
                .zip(first.zip(defense))
                .is_some_and(|(r, (first, defense))| {
                    (defense >= first) != (r.ruling == Ruling::Safe)
                });
        let phase = if invalid_defense {
            "inconsistent_defense_clock"
        } else {
            first_base_phase(first, pickup.map(|p| p.pickup_time_sec), ready, defense)
        };
        let catch = diag.map_or("unknown", |d| match d.catch_type {
            CatchType::DirectFly => "DirectFly",
            CatchType::BounceCatch => "BounceCatch",
            CatchType::FinalPickup => "FinalPickup",
            CatchType::Fumble => "Fumble",
        });
        let key = format!("{target}|{catch}|{}|{phase}", result.as_ref());
        let group = self.groups.entry(key).or_default();
        self.n += 1;
        group.n += 1;
        group.runner_first_sec.add(first);
        group.pickup_sec.add(pickup.map(|p| p.pickup_time_sec));
        group.handling_sec.add(pickup.map(|p| p.handling_delay_sec));
        group.field_complete_sec.add(ready);
        if !invalid_defense {
            group.defense_complete_sec.add(defense);
            group
                .delivery_after_field_sec
                .add(defense.zip(ready).map(|(d, f)| d - f));
        }
        if result == BattingResult::Single
            && phase == "field_complete_before_runner_delivery_unobserved"
        {
            let (runner_sec, ready_sec) = first.zip(ready)?;
            Some(FirstBaseOpportunity {
                target,
                runner_sec,
                ready_sec,
                pickup,
            })
        } else {
            None
        }
    }
}

#[derive(Default)]
pub struct BipAudit {
    source_pitch_records: u64,
    contact_population: BTreeMap<String, ContactPopulation>,
    contact_offset_ev: ContactOffsetAudit,
    hard_grounder_first_base: HardGrounderFirstBase,
    total: Group,
    tables: BTreeMap<String, BTreeMap<String, Group>>,
    coverage: BTreeMap<String, u64>,
    flags: BTreeMap<String, u64>,
    samples: BTreeMap<String, Vec<Value>>,
    batter_first_sec: Moments,
    batter_second_no_lag_sec: Moments,
    batter_third_no_lag_sec: Moments,
    final_minus_model_catch_distance_m: Moments,
    target_straight_line_distance_gap_m: Moments,
    second_plan_vs_attempt_lag_sec: Moments,
    third_plan_vs_attempt_lag_sec: Moments,
}
impl BipAudit {
    fn coverage(&mut self, key: &str) {
        *self.coverage.entry(key.into()).or_default() += 1;
    }
    fn flag(&mut self, key: &str, sample: &Value) {
        *self.flags.entry(key.into()).or_default() += 1;
        let samples = self.samples.entry(key.into()).or_default();
        if samples.len() < 4 {
            samples.push(sample.clone());
        }
    }
    pub fn observe(&mut self, schedule: &GameSchedule, game: &GameResult) {
        let roster: BTreeMap<_, _> = schedule
            .away_team
            .players
            .iter()
            .chain(&schedule.home_team.players)
            .map(|p| (p.info.id, p))
            .collect();
        self.source_pitch_records += game.player_battings.len() as u64;
        for b in &game.player_battings {
            use saberbb::domain::resolver::batting_resolver::SwingContactType;
            if matches!(
                b.adjusted_contact_type,
                SwingContactType::SolidContact
                    | SwingContactType::WeakContact
                    | SwingContactType::MarginalContact
            ) {
                let quality = format!("{:?}", b.adjusted_contact_type);
                self.contact_offset_ev.add(
                    b.length_offset_m,
                    b.thickness_offset_m,
                    &quality,
                    b.result,
                    b.ball.launch_speed,
                );
                let c = self.contact_population.entry(quality).or_default();
                c.n += 1;
                c.bip += u64::from(is_bip(b.result));
                c.fouls += u64::from(b.result == BattingResult::Foul);
                c.hr += u64::from(b.result == BattingResult::HomeRun);
                c.ev_ms.add(Some(b.ball.launch_speed));
                c.length_offset_m.add(Some(b.length_offset_m));
                c.thickness_offset_m.add(b.thickness_offset_m);
                c.abs_adapted_timing_ms
                    .add(b.adapted_timing_offset_sec.map(|t| t.abs() * 1000.0));
            }
            if !is_bip(b.result) {
                continue;
            }
            self.coverage("bip_terminal_results");
            let fields: Vec<_> = game
                .player_fieldings
                .iter()
                .filter(|f| f.count_seq == b.count_seq && f.seq == 1)
                .collect();
            let f = fields.first().copied();
            let running = game
                .player_runnings
                .iter()
                .filter(|r| r.count_seq == b.count_seq && r.event == RunningEvent::GrounderPlay)
                .min_by_key(|r| r.seq);
            let diag = f.and_then(|f| f.catch_diagnostics);
            let field_time = f
                .map(|f| f.time_to_field)
                .filter(|v| v.is_finite() && *v >= 0.0);
            let defense_time = running
                .map(|r| r.defense_time)
                .filter(|v| v.is_finite() && *v >= 0.0);
            let margin = running
                .and_then(|r| actual_margin(r.target_runner_id, r.defense_time, r.runner_time));
            let runner_times = roster.get(&b.batter_id).and_then(|p| {
                p.offense_skills.batter.as_ref().map(|batter| {
                    let runners = RunnersOnBase {
                        batter_runner: Some(ActiveRunner {
                            id: p.info.id,
                            skills: p.offense_skills.running,
                        }),
                        ..Default::default()
                    };
                    (
                        runners
                            .batter_runner_time_to(Base::First, true, batter.batting_side)
                            .ok(),
                        runners
                            .batter_runner_time_to(Base::Second, false, batter.batting_side)
                            .ok(),
                        runners
                            .batter_runner_time_to(Base::Third, false, batter.batting_side)
                            .ok(),
                        runners
                            .batter_runner_time_to(Base::Second, true, batter.batting_side)
                            .ok(),
                        runners
                            .batter_runner_time_to(Base::Third, true, batter.batting_side)
                            .ok(),
                    )
                })
            });
            let (first, second, third, second_plan, third_plan) =
                runner_times.unwrap_or((None, None, None, None, None));
            self.batter_first_sec.add(first);
            self.batter_second_no_lag_sec.add(second);
            self.batter_third_no_lag_sec.add(third);
            let trajectory = if b.ball.launch_angle.is_finite()
                && b.ball.spin_rate.is_finite()
                && b.ball.spin_angle.is_finite()
            {
                format!("{:?}", b.ball.trajectory())
            } else {
                "unknown".into()
            };
            if trajectory == "Grounder"
                && (45.0..50.0).contains(&b.ball.launch_speed)
                && (0.0..10.0).contains(&b.ball.launch_angle)
            {
                if let Some(opportunity) = self.hard_grounder_first_base.add(
                    b.result,
                    b.batter_id,
                    running,
                    first,
                    field_time,
                    diag,
                ) {
                    let (role, position) = match fields.as_slice() {
                        [f] if f.catch_fielder_position.is_infielder() => {
                            ("infield", f.catch_fielder_position.as_ref())
                        }
                        [f] if f.catch_fielder_position.is_outfielder() => {
                            ("outfield", f.catch_fielder_position.as_ref())
                        }
                        _ => ("unknown", "unknown"),
                    };
                    let catch = diag.as_ref().map_or("unknown", |d| d.catch_type.as_ref());
                    let key = format!("{}|{role}|{position}|{catch}", opportunity.target);
                    self.hard_grounder_first_base
                        .unobserved_first_trial
                        .entry(key)
                        .or_default()
                        .record(opportunity, b, &fields, game, &roster);
                }
            }
            let ev = band(b.ball.launch_speed, &[20.0, 35.0, 45.0]);
            let vla = band(b.ball.launch_angle, &[-10.0, 0.0, 10.0, 25.0, 50.0]);
            // Only model 2 records guarantee that HLA is in stadium coordinates.
            let hla = if b.hla_model_version == Some(2) {
                band(b.final_hla_deg, CONVERSION_HLA_EDGES_DEG)
            } else {
                "unknown".into()
            };
            let conversion_ev = band(b.ball.launch_speed, CONVERSION_EV_EDGES_MS);
            let position = f.map_or_else(
                || "unknown".into(),
                |f| format!("{:?}", f.catch_fielder_position),
            );
            let path = diag.map_or_else(|| "unknown".into(), |d| format!("{:?}", d.selection_path));
            let catch = diag.map_or_else(|| "unknown".into(), |d| format!("{:?}", d.catch_type));
            let dimensions = [
                ("by_trajectory", trajectory.clone()),
                ("by_ev_ms", ev.clone()),
                ("by_vla_deg", vla.clone()),
                (
                    "by_trajectory_ev_vla",
                    format!("{trajectory}|EV:{ev}|VLA:{vla}"),
                ),
                (
                    "by_trajectory_ev_vla_hla",
                    format!("{trajectory}|EV:{conversion_ev}|VLA:{vla}|HLA:{hla}"),
                ),
                ("by_fielder_position", position),
                ("by_selection_path", path),
                ("by_catch_type", catch),
                (
                    "by_fielding_error",
                    diag.map_or_else(
                        || "unknown".into(),
                        |d| {
                            d.error_type
                                .map_or_else(|| "none".into(), |e| format!("{e:?}"))
                        },
                    ),
                ),
                (
                    "by_contact_quality",
                    format!("{:?}", b.adjusted_contact_type),
                ),
                (
                    "by_final_distance_m",
                    band(b.ball.final_position.distance, &[30.0, 60.0, 90.0]),
                ),
            ];
            self.total.add(
                b.result,
                b.ball.launch_speed,
                b.ball.launch_angle,
                field_time,
                defense_time,
                margin,
            );
            for (table, key) in dimensions {
                self.tables
                    .entry(table.into())
                    .or_default()
                    .entry(key)
                    .or_default()
                    .add(
                        b.result,
                        b.ball.launch_speed,
                        b.ball.launch_angle,
                        field_time,
                        defense_time,
                        margin,
                    );
            }
            let candidates: Vec<_> = game
                .player_fielding_candidates
                .iter()
                .filter(|c| c.count_seq == b.count_seq)
                .collect();
            let probe_possible = candidates.iter().any(|c| {
                c.diagnostics
                    .air_probe
                    .is_some_and(|p| p.risk_ok_samples > 0)
            });
            let modeled_catch = diag
                .filter(|d| {
                    d.error_type.is_none()
                        && matches!(d.catch_type, CatchType::DirectFly | CatchType::BounceCatch)
                })
                .map(|d| {
                    let (r, angle, x, y, _, _) =
                        estimate_ball_state_at_time(d.catch_time_sec, &b.ball);
                    json!({"distance_m":r,"angle_deg":angle,"x_m":x,"y_m":y})
                });
            if let Some(v) = modeled_catch
                .as_ref()
                .and_then(|v| v["distance_m"].as_f64())
            {
                self.final_minus_model_catch_distance_m
                    .add(Some(b.ball.final_position.distance - v));
            }
            let target_distance_gap = modeled_catch.as_ref().and_then(|v| {
                let catch = saberbb::domain::util::PolarPosition::new(
                    v["distance_m"].as_f64()?,
                    v["angle_deg"].as_f64()?,
                );
                let target = running?.throw_target_base.polar_position();
                Some(
                    saberbb::domain::util::calculate_polar_distance(
                        &b.ball.final_position,
                        &target,
                    ) - saberbb::domain::util::calculate_polar_distance(&catch, &target),
                )
            });
            self.target_straight_line_distance_gap_m
                .add(target_distance_gap);
            if let Some(r) =
                running.filter(|r| r.target_runner_id == Some(b.batter_id) && r.runner_time > 0.0)
            {
                if r.throw_target_base == Base::Second {
                    self.second_plan_vs_attempt_lag_sec
                        .add(second.map(|t| r.runner_time - t));
                }
                if r.throw_target_base == Base::Third {
                    self.third_plan_vs_attempt_lag_sec
                        .add(third.map(|t| r.runner_time - t));
                }
            }
            let sample = json!({"game_id":game.id,"count_seq":b.count_seq,"batter_id":b.batter_id,"result":b.result,
                "trajectory":trajectory,"ev_ms":b.ball.launch_speed,"vla_deg":b.ball.launch_angle,"hla_deg":b.final_hla_deg,
                "contact_quality":b.adjusted_contact_type,"length_offset_m":b.length_offset_m,"thickness_offset_m":b.thickness_offset_m,
                "ball_final_position":b.ball.final_position,"ball_first_bounce_time_sec":b.ball.first_bounce_time,
                "modeled_catch_position":modeled_catch,"target_straight_line_distance_gap_m":target_distance_gap,"fielding":f,"running":running,
                "batter_first_with_lag_sec":first,"batter_second_no_lag_sec":second,"batter_third_no_lag_sec":third,
                "batter_second_plan_with_lag_sec":second_plan,"batter_third_plan_with_lag_sec":third_plan,
                "candidate_count":candidates.len(),"probe_risk_ok_candidate_count":candidates.iter().filter(|c|c.diagnostics.air_probe.is_some_and(|p|p.risk_ok_samples>0)).count()});
            if b.ball.outbound_result == OutboundResult::GroundRuleDouble {
                self.coverage("automatic_ground_rule_doubles");
            } else if f.is_none() {
                self.coverage("missing_primary_fielding_record");
                self.flag("missing_primary_fielding_record", &sample);
            }
            if fields.len() > 1 {
                self.flag("duplicate_primary_fielding_records", &sample);
            }
            if diag.is_some() {
                self.coverage("catch_diagnostics_present");
            }
            if diag.is_some_and(|d| d.error_type.is_some()) {
                self.flag("recorded_fielding_error", &sample);
            }
            if running.is_some() {
                self.coverage("grounder_play_running_record_present");
            }
            if margin.is_some() {
                self.coverage("actual_target_timing_comparable");
            }
            if running.is_some_and(|r| r.target_runner_id == Some(b.batter_id)) && margin.is_some()
            {
                self.coverage("actual_batter_target_timing_comparable");
            }
            if let Some(r) = running {
                if let Some(m) = margin {
                    if (m >= 0.0) != (r.ruling == Ruling::Safe) {
                        self.flag("target_timing_ruling_mismatch", &sample);
                    }
                }
                if field_time.zip(defense_time).is_some_and(|(f, d)| d < f) {
                    self.flag("defense_time_before_field_time", &sample);
                }
            }
            if !candidates.is_empty() {
                self.coverage("candidate_records_present");
            }
            if is_hit(b.result)
                && probe_possible
                && !diag
                    .is_some_and(|d| d.catch_type == CatchType::DirectFly && d.error_type.is_none())
            {
                self.flag("hit_with_risk_ok_air_probe", &sample);
            }
            if is_hit(b.result) {
                self.flag("hit_examples", &sample);
            }
            if running.is_some_and(|r| {
                r.target_runner_id == Some(b.batter_id)
                    && r.throw_target_base == Base::First
                    && r.ruling == Ruling::Safe
            }) && target_distance_gap.is_some_and(|d| d > 10.0)
            {
                self.flag(
                    "batter_first_safe_with_final_origin_over_10m_farther",
                    &sample,
                );
            }
            if let Some(r) = running
                .filter(|r| r.target_runner_id == Some(b.batter_id) && r.ruling == Ruling::Out)
            {
                if r.throw_target_base == Base::Second
                    && second_plan.is_some_and(|t| r.defense_time > t)
                    && r.defense_time < r.runner_time
                {
                    self.flag(
                        "second_plan_threshold_met_but_accelerated_attempt_out",
                        &sample,
                    );
                }
                if r.throw_target_base == Base::Third
                    && third_plan.is_some_and(|t| r.defense_time > t)
                    && r.defense_time < r.runner_time
                {
                    self.flag(
                        "third_plan_threshold_met_but_accelerated_attempt_out",
                        &sample,
                    );
                }
            }
            if b.result == BattingResult::Single {
                if field_time
                    .zip(first)
                    .is_some_and(|(field, first)| field >= first)
                {
                    self.flag("single_fielding_after_first_arrival", &sample);
                }
                if running.is_some_and(|r| {
                    r.target_runner_id == Some(b.batter_id)
                        && r.throw_target_base == Base::Second
                        && r.ruling == Ruling::Out
                }) {
                    self.flag("single_batter_out_trying_second", &sample);
                }
                if f.is_some_and(|f| f.catch_fielder_position.is_outfielder())
                    && defense_time.zip(second_plan).is_some_and(|(d, t)| d > t)
                {
                    self.flag("outfield_single_second_plan_threshold_met", &sample);
                    if !running.is_some_and(|r| {
                        r.target_runner_id == Some(b.batter_id) && r.ruling == Ruling::Out
                    }) {
                        self.flag(
                            "outfield_single_second_threshold_without_batter_out",
                            &sample,
                        );
                    }
                }
                if running.is_some_and(|r| {
                    r.runner_2nd_id == Some(b.batter_id) || r.runner_3rd_id == Some(b.batter_id)
                }) {
                    self.flag("single_batter_in_second_or_third_snapshot", &sample);
                }
            }
            if !is_hit(b.result) {
                self.flag("out_examples", &sample);
            }
        }
    }
    pub fn json(&self) -> Value {
        json!({"source_pitch_records":self.source_pitch_records,"contact_population":self.contact_population,"summary":self.total.json(),"tables":self.tables.iter().map(|(t,groups)|(t,groups.iter().map(|(k,g)|(k,g.json())).collect::<BTreeMap<_,_>>())).collect::<BTreeMap<_,_>>(),
            "coverage":self.coverage,"flags":self.flags,"samples":self.samples,
            "contact_offset_ev":self.contact_offset_ev.json(),
            "hard_grounder_first_base":self.hard_grounder_first_base,
            "batter_first_with_lag_sec":self.batter_first_sec,"batter_second_no_lag_sec":self.batter_second_no_lag_sec,"batter_third_no_lag_sec":self.batter_third_no_lag_sec,
            "final_minus_model_catch_distance_m":self.final_minus_model_catch_distance_m,
            "target_straight_line_distance_gap_m":self.target_straight_line_distance_gap_m,
            "second_plan_vs_attempt_lag_sec":self.second_plan_vs_attempt_lag_sec,"third_plan_vs_attempt_lag_sec":self.third_plan_vs_attempt_lag_sec,
            "definitions":["BIP denominator: Single/Double/Triple/Out/FieldersChoice/DoublePlay; HR and nonterminal fouls excluded. Automatic ground-rule doubles remain in the denominator and are separately counted.",
                "Contact population includes all recorded contact types before foul/HR exclusion, while BIP tables include only terminal BIP outcomes. Group n is plays, not outs; double plays are one BIP. Timings have independent n; missing values never become zero.",
                "target_runner_margin_sec = recorded defense_time - recorded runner_time only for a known target with positive runner_time; >=0 means Safe. The target may be a lead runner, not the batter.",
                "Batter first-base timing includes the live acceleration lag; second/third no-lag timings expose legacy RunningPlan thresholds; current planning includes acceleration. Plan-threshold flags use current acceleration-aware times, also included in samples. These are not alternative throw simulations.",
                "single_rate = Single / BIP. out_on_play_rate = (Out + FieldersChoice + DoublePlay) / BIP; these are batting result codes, not individual outs recorded on the play.",
                "The joint trajectory/EV/VLA/HLA table uses m/s and degrees. HLA is the recorded initial direction in stadium coordinates, negative left and positive right. Missing or unsupported HLA is unknown.",
                "Trajectory uses the live spin-adjusted classifier. Model catch positions use the fielding estimator, not the original physical integration; errors/fumbles and FinalPickup (which has an unrecorded pickup delay) are excluded from position reconstruction.",
                "Live throw timing currently references ball.final_position; the modeled catch/final gap is diagnostic evidence, not a corrected throw time. Target straight-line gaps do not model cutoff/receiving/run-to-base choices.",
                "Risk-ok air probes are counterfactual search diagnostics, not guaranteed catches or proof of a fielding bug. Samples are the first four per flag, not a representative random sample."]})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn joins_play_records_without_treating_lead_runner_time_as_batter_time() {
        use saberbb::domain::resolver::fielding_resolver::PlayType;
        use saberbb::domain::shared::{
            ball::BattedBall,
            game::GameType,
            game_stats::{PlayerGameBattingMetrics, PlayerGameRunning},
            stadium::Stadium,
            team::Team,
        };
        let date = chrono::NaiveDate::from_ymd_opt(2026, 4, 1).unwrap();
        let schedule = GameSchedule {
            id: 1,
            season: 2026,
            round_seq: 1,
            seq: 1,
            planned_date: date,
            away_team: Team::min(1, "A"),
            home_team: Team::min(2, "B"),
            stadium: Stadium::default(),
            game_type: GameType::Regular,
            max_inning: 9,
            base_four_seam_speed: 40.8,
        };
        let mut game = GameResult::new(1, date, &[], &[]);
        let mut ball = BattedBall::default();
        ball.launch_speed = 40.0;
        ball.launch_angle = 15.0;
        game.add_player_batting(
            1,
            10,
            7,
            PlayerGameBattingMetrics {
                hla_model_version: Some(2),
                final_hla_deg: -15.0,
                ..Default::default()
            },
            ball,
            None,
            BattingResult::Single,
        );
        game.player_runnings.push(PlayerGameRunning {
            count_seq: 1,
            seq: 1,
            defense_time: 4.0,
            runner_time: 3.0,
            throw_target_base: Base::Second,
            target_runner_id: Some(99),
            event: RunningEvent::GrounderPlay,
            play_type: PlayType::ForcePlay,
            ruling: Ruling::Safe,
            runs_scored: 0,
            runner_1st_id: Some(7),
            runner_2nd_id: Some(99),
            runner_3rd_id: None,
        });
        let before = serde_json::to_vec(&game).unwrap();
        let mut audit = BipAudit::default();
        audit.observe(&schedule, &game);
        let report = audit.json();
        assert_eq!(report["summary"]["n"], 1);
        assert_eq!(report["summary"]["hit_rate"], 1.0);
        assert_eq!(report["summary"]["field_time_sec"]["n"], 0);
        assert_eq!(report["coverage"]["actual_target_timing_comparable"], 1);
        assert!(
            report["coverage"]
                .get("actual_batter_target_timing_comparable")
                .is_none()
        );
        assert!(
            report["flags"]
                .get("single_batter_in_second_or_third_snapshot")
                .is_none()
        );
        assert_eq!(serde_json::to_vec(&game).unwrap(), before);
        game.add_player_batting(
            3,
            10,
            7,
            PlayerGameBattingMetrics {
                hla_model_version: Some(2),
                final_hla_deg: -14.9,
                ..Default::default()
            },
            ball,
            None,
            BattingResult::FieldersChoice,
        );
        ball.outbound_result = OutboundResult::GroundRuleDouble;
        game.add_player_batting(
            2,
            10,
            7,
            PlayerGameBattingMetrics::default(),
            ball,
            None,
            BattingResult::Double,
        );
        let mut audit = BipAudit::default();
        audit.observe(&schedule, &game);
        let r = audit.json();
        assert_eq!(r["summary"]["n"], 3);
        assert_eq!(r["coverage"]["automatic_ground_rule_doubles"], 1);
        assert_eq!(r["coverage"]["missing_primary_fielding_record"], 2);
        let cells = &r["tables"]["by_trajectory_ev_vla_hla"];
        let left = &cells["Liner|EV:[40,42.4688)|VLA:[10,25)|HLA:[-15,-5)"];
        assert_eq!(left["n"], 2);
        assert_eq!(left["single_rate"], 0.5);
        assert_eq!(left["out_on_play_rate"], 0.5);
        assert_eq!(left["results"]["FieldersChoice"], 1);
        let unknown = &cells["Liner|EV:[40,42.4688)|VLA:[10,25)|HLA:unknown"];
        assert_eq!(unknown["n"], 1);
        assert_eq!(unknown["doubles"], 1);
        assert_eq!(unknown["out_on_play_rate"], 0.0);
    }

    #[test]
    fn contact_offset_grid_separates_foul_ev_and_counts_home_runs_in_bip_plus_hr_ev() {
        let mut audit = ContactOffsetAudit::default();
        audit.add(0.02, Some(0.025), "WeakContact", BattingResult::Foul, 45.0);
        audit.add(
            0.02,
            Some(0.025),
            "WeakContact",
            BattingResult::Out,
            HARD_HIT_MS - 0.001,
        );
        audit.add(
            0.02,
            Some(0.025),
            "WeakContact",
            BattingResult::HomeRun,
            HARD_HIT_MS,
        );
        audit.add(
            f64::NAN,
            None,
            "MarginalContact",
            BattingResult::Out,
            f64::NAN,
        );
        let report = audit.json();
        let cell = &report["cells"]["length_m:[0.02,0.05)|thickness_m:[0.025,0.04)"];
        assert_eq!(cell["contacts"], 3);
        assert_eq!(cell["fouls"], 1);
        assert_eq!(cell["bip"], 1);
        assert_eq!(cell["home_runs"], 1);
        assert_eq!(cell["by_contact_quality"]["WeakContact"], 3);
        assert_eq!(cell["all_contact_ev"]["hard_hits"], 2);
        assert_eq!(cell["bip_plus_hr_ev"]["ev_ms"]["n"], 2);
        assert_eq!(cell["bip_plus_hr_ev"]["hard_hit_rate"], 0.5);
        let unknown = &report["cells"]["length_m:unknown|thickness_m:unknown"];
        assert_eq!(unknown["contacts"], 1);
        assert_eq!(unknown["bip_plus_hr_ev"]["ev_ms"]["n"], 0);
        assert!(unknown["bip_plus_hr_ev"]["hard_hit_rate"].is_null());
        assert_eq!(report["total"]["contacts"], 4);
        assert_eq!(report["total"]["bip_plus_hr_ev"]["ev_ms"]["n"], 2);
    }

    #[test]
    fn cover_scenario_requires_both_ball_and_receiver_before_runner() {
        let mut scenario = CoverScenario::default();
        scenario.add(3.5, 3.8, 4.0);
        scenario.add(3.5, 4.0, 4.0);
        scenario.add(3.6, 3.7, 4.0);
        scenario.add(4.1, 3.8, 4.0);
        assert_eq!(scenario.modeled_out, 2);
        assert_eq!(scenario.out_with_at_least_025_sec, 1);
        assert_eq!(scenario.cover_later_than_ball, 3);
    }

    #[test]
    fn first_base_phases_follow_the_live_safe_at_tie_rule() {
        let classify =
            |pickup, ready, defense| first_base_phase(Some(4.0), pickup, Some(ready), defense);
        assert_eq!(
            classify(Some(4.0), 4.4, Some(5.2)),
            "pickup_at_or_after_runner"
        );
        assert_eq!(
            classify(Some(3.7), 4.0, Some(4.7)),
            "handling_crossed_runner"
        );
        assert_eq!(
            classify(Some(3.3), 3.7, Some(4.0)),
            "delivery_crossed_runner"
        );
        assert_eq!(
            classify(Some(3.3), 3.7, Some(3.9)),
            "defense_completed_before_runner"
        );
        assert_eq!(
            classify(None, 4.0, None),
            "field_complete_late_pickup_unknown"
        );
        assert_eq!(
            classify(Some(3.3), 3.7, None),
            "field_complete_before_runner_delivery_unobserved"
        );
        assert_eq!(
            first_base_phase(None, None, Some(3.7), None),
            "missing_clock"
        );
    }

    #[test]
    fn bands_use_declared_boundaries_and_preserve_unknowns() {
        assert_eq!(band(35.0, &[20.0, 35.0, 45.0]), "[35,45)");
        assert_eq!(band(f64::NAN, &[20.0]), "unknown");
    }
    #[test]
    fn timing_never_invents_a_runner_from_zero_or_missing_target() {
        assert_eq!(actual_margin(None, 3.0, 0.0), None);
        assert_eq!(actual_margin(Some(7), 3.0, 0.0), None);
        assert_eq!(actual_margin(Some(7), 3.0, 4.0), Some(-1.0));
    }
    #[test]
    fn denominators_exclude_hr_k_and_nonterminal_pitches() {
        assert!(is_bip(BattingResult::DoublePlay));
        for r in [
            BattingResult::HomeRun,
            BattingResult::Strikeout,
            BattingResult::Foul,
            BattingResult::Walk,
        ] {
            assert!(!is_bip(r));
        }
    }
    #[test]
    fn moments_skip_missing_and_nonfinite_samples() {
        let mut m = Moments::default();
        for v in [Some(2.0), None, Some(f64::NAN), Some(4.0)] {
            m.add(v);
        }
        assert_eq!(m.n, 2);
        assert_eq!(m.mean, Some(3.0));
    }
}
