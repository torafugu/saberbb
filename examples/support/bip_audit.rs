//! Read-only aggregation of completed GameResult records. Never draws RNG or reruns a play.
use saberbb::domain::{
    resolver::{
        fielding_physics::{CatchType, estimate_ball_state_at_time},
        running_resolver::{RunnersOnBase, RunningEvent},
    },
    shared::{
        ball::OutboundResult,
        game::{BattingResult, GameResult, GameSchedule},
        game_state::{ActiveRunner, Ruling},
        stadium::Base,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
        v["hit_rate"] = if self.n > 0 {
            json!(self.hits as f64 / self.n as f64)
        } else {
            Value::Null
        };
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
pub struct BipAudit {
    source_pitch_records: u64,
    contact_population: BTreeMap<String, ContactPopulation>,
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
                let c = self
                    .contact_population
                    .entry(format!("{:?}", b.adjusted_contact_type))
                    .or_default();
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
            let ev = band(b.ball.launch_speed, &[20.0, 35.0, 45.0]);
            let vla = band(b.ball.launch_angle, &[-10.0, 0.0, 10.0, 25.0, 50.0]);
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
            "batter_first_with_lag_sec":self.batter_first_sec,"batter_second_no_lag_sec":self.batter_second_no_lag_sec,"batter_third_no_lag_sec":self.batter_third_no_lag_sec,
            "final_minus_model_catch_distance_m":self.final_minus_model_catch_distance_m,
            "target_straight_line_distance_gap_m":self.target_straight_line_distance_gap_m,
            "second_plan_vs_attempt_lag_sec":self.second_plan_vs_attempt_lag_sec,"third_plan_vs_attempt_lag_sec":self.third_plan_vs_attempt_lag_sec,
            "definitions":["BIP denominator: Single/Double/Triple/Out/FieldersChoice/DoublePlay; HR and nonterminal fouls excluded. Automatic ground-rule doubles remain in the denominator and are separately counted.",
                "Contact population includes all recorded contact types before foul/HR exclusion, while BIP tables include only terminal BIP outcomes. Group n is plays, not outs; double plays are one BIP. Timings have independent n; missing values never become zero.",
                "target_runner_margin_sec = recorded defense_time - recorded runner_time only for a known target with positive runner_time; >=0 means Safe. The target may be a lead runner, not the batter.",
                "Batter first-base timing includes the live acceleration lag; second/third no-lag timings expose legacy RunningPlan thresholds; current planning includes acceleration. Plan-threshold flags use current acceleration-aware times, also included in samples. These are not alternative throw simulations.",
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
            PlayerGameBattingMetrics::default(),
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
        assert_eq!(r["summary"]["n"], 2);
        assert_eq!(r["coverage"]["automatic_ground_rule_doubles"], 1);
        assert_eq!(r["coverage"]["missing_primary_fielding_record"], 1);
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
