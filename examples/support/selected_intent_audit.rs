//! Aggregate the selected forward 3-D path on all recorded swings, never a new Intent selection.
use super::{
    contact_direction_audit::checked_baseline,
    moving_collision_audit::{Group, Moments, Outcome, band, vector},
};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    resolver::batting_resolver::{SwingContactType, calculate_trajectory},
    shared::{
        contact_state::ContactStateRecord,
        finite_bat_collision as finite,
        frozen_contact_collision::flight_spin,
        game::{GameResult, GameSchedule},
        game_stats::PlayerGameBatting,
        intent_collision_response::{self, CollisionEvaluation},
        intent_execution_contact::{ContactSearchResult, ContactSearchSettings},
        stadium::Stadium,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn ball_radius_m() -> f64 {
    finite::CollisionParameters::frozen(0.5, 0.07).ball_radius
}
fn height_category(z: f64) -> &'static str {
    if !z.is_finite() {
        "non_finite"
    } else if z < 0.0 {
        "center_below_ground"
    } else if z < ball_radius_m() {
        "sphere_intersects_ground"
    } else {
        "clear_of_ground"
    }
}
fn legacy_kind(b: Option<&PlayerGameBatting>) -> &'static str {
    match b.map(|b| b.adjusted_contact_type) {
        Some(
            SwingContactType::SolidContact
            | SwingContactType::WeakContact
            | SwingContactType::MarginalContact,
        ) => "contact",
        Some(SwingContactType::SwungAndMiss) => "miss",
        _ => "unknown",
    }
}
struct Row<'a> {
    intent: String,
    legacy: &'static str,
    quality: String,
    search_status: String,
    collision_status: String,
    search: Option<&'a ContactSearchResult>,
    evaluation: Option<&'a CollisionEvaluation>,
    outcome: Option<Outcome>,
    baseline: Option<Outcome>,
    band: Option<String>,
    contact_point: Option<[f64; 3]>,
    reference_z: Option<f64>,
    checks: BTreeMap<String, f64>,
}
impl<'a> Row<'a> {
    fn new(b: Option<&PlayerGameBatting>) -> Self {
        Self {
            intent: "unknown".into(),
            legacy: legacy_kind(b),
            quality: b
                .map(|b| format!("{:?}", b.adjusted_contact_type))
                .unwrap_or("unknown".into()),
            search_status: "unavailable:missing_state".into(),
            collision_status: "not_evaluated:search_unavailable".into(),
            search: None,
            evaluation: None,
            outcome: None,
            baseline: None,
            band: None,
            contact_point: None,
            reference_z: None,
            checks: BTreeMap::new(),
        }
    }
}
fn search_category(status: &str) -> &str {
    if status.starts_with("unavailable:") {
        "unavailable"
    } else {
        status
    }
}
fn evaluate<'a>(
    record: &'a ContactStateRecord,
    b: Option<&PlayerGameBatting>,
    stadium: &Stadium,
) -> Result<Row<'a>> {
    let mut row = Row::new(b);
    if let Some(reason) = &record.invalid_reason {
        row.search_status = format!("unavailable:state:{reason}");
        return Ok(row);
    }
    let Some(state) = record.state.as_ref() else {
        return Ok(row);
    };
    row.reference_z = Some(state.input.ball_position_world_m[2]);
    if let Some(observed) = state.observed_intent_plans.as_ref() {
        row.intent = format!("{:?}", observed.decision.selected_intent);
        if let Some(reason) = &observed.invalid_reason {
            row.search_status = format!("unavailable:observed:{reason}");
            return Ok(row);
        }
    }
    let Some(exec) = state.execution_contact.as_ref() else {
        row.search_status = "unavailable:missing_execution".into();
        return Ok(row);
    };
    ensure!(
        exec.version == 1,
        "unsupported execution diagnostic version"
    );
    ensure!(
        serde_json::to_value(exec.settings)?
            == serde_json::to_value(ContactSearchSettings::default())?,
        "mixed contact search settings"
    );
    row.intent = format!("{:?}", exec.decision.selected_intent);
    if let Some(observed) = &state.observed_intent_plans {
        ensure!(
            observed.decision == exec.decision,
            "observed/execution decision mismatch"
        );
    }
    let selected: Vec<_> = exec.probes.iter().filter(|p| p.selected).collect();
    ensure!(selected.len() <= 1, "duplicate selected execution probes");
    let Some(probe) = selected.first() else {
        row.search_status = "unavailable:missing_selected_execution".into();
        return Ok(row);
    };
    ensure!(
        probe.intent == exec.decision.selected_intent,
        "selected execution intent mismatch"
    );
    if let Some(reason) = &probe.invalid_reason {
        row.search_status = format!("unavailable:execution:{reason}");
        return Ok(row);
    }
    let Some(search) = &probe.search else {
        row.search_status = "unavailable:missing_search".into();
        return Ok(row);
    };
    row.search = Some(search);
    row.search_status = search.status.clone();
    ensure!(
        ["contact", "no_contact", "unresolved", "overlap_at_start"]
            .contains(&search.status.as_str()),
        "unknown search status"
    );
    if search.status != "contact" {
        row.collision_status = format!("not_evaluated:search_{}", search.status);
        return Ok(row);
    }
    let geometry = search
        .geometry
        .as_ref()
        .context("contact without geometry")?;
    let time = search.time_offset_sec.context("contact without time")?;
    let point = std::array::from_fn(|i| {
        state.input.ball_position_world_m[i] + state.input.ball_velocity_world_ms[i] * time
    });
    ensure!(
        time.is_finite() && point.iter().all(|v| v.is_finite()),
        "invalid contact point"
    );
    row.contact_point = Some(point);

    ensure!(
        geometry.axial_m.is_finite()
            && geometry
                .normal
                .is_some_and(|n| n.iter().all(|x| x.is_finite())),
        "invalid contact geometry"
    );
    row.band = Some(format!(
        "{}|region:{}|axial_m:{}|normal_z:{}",
        row.intent,
        geometry.contact_region,
        band(geometry.axial_m, &[-0.13, -0.04, 0.04, 0.13]),
        band(geometry.normal.unwrap()[2], &[-0.5, -0.1, 0.1, 0.5])
    ));
    let Some(collision) = &state.intent_collision else {
        row.collision_status = "unavailable:missing_collision".into();
        return Ok(row);
    };
    ensure!(
        collision.version == 1 && collision.model == finite::MODEL,
        "unsupported collision diagnostic model/version"
    );
    ensure!(
        collision.decision == exec.decision,
        "execution/collision decision mismatch"
    );
    let selected: Vec<_> = collision.probes.iter().filter(|p| p.selected).collect();
    ensure!(selected.len() <= 1, "duplicate selected collision probes");
    let Some(cp) = selected.first() else {
        row.collision_status = "unavailable:missing_selected_collision".into();
        return Ok(row);
    };
    ensure!(
        cp.intent == probe.intent,
        "selected collision intent mismatch"
    );
    if let Some(reason) = &cp.invalid_reason {
        ensure!(
            !["collision_identity_mismatch", "normal_control_mismatch"].contains(&reason.as_str()),
            "upstream collision identity failed: {reason}"
        );
        row.collision_status = format!("collision_error:{reason}");
        return Ok(row);
    }
    let Some(ev) = &cp.evaluation else {
        row.collision_status = "unavailable:missing_evaluation".into();
        return Ok(row);
    };
    row.evaluation = Some(ev);
    // Re-derive input/response from the selected forward geometry, not from legacy contact offsets.
    let expected = intent_collision_response::evaluate(state, probe, &collision.spin)
        .map_err(anyhow::Error::msg)?;
    ensure!(
        serde_json::to_value(&expected)? == serde_json::to_value(ev)?,
        "selected collision reconstruction mismatch"
    );
    let status = ev.response["collision_status"]
        .as_str()
        .context("missing collision status")?;
    row.collision_status = status.into();
    for (name, v) in &ev.control_checks {
        row.checks.insert(format!("control:{name}"), *v);
    }
    for (label, response) in [
        ("response", Some(&ev.response)),
        ("control", ev.control.as_ref()),
    ] {
        if let Some(checks) = response.and_then(|v| v["checks"].as_object()) {
            for (name, v) in checks {
                row.checks.insert(
                    format!("{label}:{name}"),
                    v.as_f64().context("invalid identity value")?,
                );
            }
        }
    }
    ensure!(
        row.checks
            .values()
            .all(|v| v.is_finite() && (0.0..=1e-7).contains(v)),
        "selected collision identity failed"
    );
    if status != "impulse_contact" {
        return Ok(row);
    }
    let spin = match flight_spin(
        vector(&ev.response, "outgoing_world_ms")?,
        vector(&ev.response, "outgoing_spin_rad_s")?,
    ) {
        Ok(spin) => spin,
        Err(reason) => {
            row.collision_status = format!("flight_unavailable:{reason}");
            return Ok(row);
        }
    };
    let number = |k: &str| {
        ev.response[k]
            .as_f64()
            .filter(|v| v.is_finite())
            .context("invalid launch metric")
    };
    let (speed, vla, hla) = (number("ev_ms")?, number("vla_deg")?, number("hla_deg")?);
    let ball = match calculate_trajectory(
        speed,
        vla,
        hla,
        spin.transverse_rpm,
        spin.angle_deg,
        stadium,
    ) {
        Ok(ball) => ball,
        Err(_) => {
            row.collision_status = "flight_error".into();
            return Ok(row);
        }
    };
    row.outcome = Some(Outcome {
        ball,
        hla,
        total_spin: Some(spin.total_rpm),
        gyro_spin: Some(spin.gyro_rpm),
    });
    row.collision_status = "accepted_flight".into();
    if row.legacy == "contact" {
        if let Some(b) = b {
            if let Some(ball) = checked_baseline(b, stadium)? {
                row.baseline = Some(Outcome {
                    ball,
                    hla: b.final_hla_deg,
                    total_spin: None,
                    gyro_spin: None,
                });
            }
        }
    }
    Ok(row)
}
#[derive(Default, Serialize)]
struct Cohort {
    swings: u64,
    search_statuses: BTreeMap<String, u64>,
    collision_statuses: BTreeMap<String, u64>,
    agreement: BTreeMap<String, BTreeMap<String, u64>>,
    binary_comparable: u64,
    binary_agree: u64,
    contact_time_offset_ms: Moments,
    contact_x_m: Moments,
    contact_y_m: Moments,
    contact_z_m: Moments,
    reference_z_m: Moments,
    reference_height_statuses: BTreeMap<String, u64>,
    contact_height_statuses: BTreeMap<String, u64>,
    height_transitions: BTreeMap<String, BTreeMap<String, u64>>,
    ground_valid_outcomes: Group,
    ground_invalid_outcomes: Group,
    axial_m: Moments,
    normal_z: Moments,
    centerline_speed_ms: Moments,
    outcomes: Group,
    paired_live: Group,
    paired_3d: Group,
    paired_baseline_statuses: BTreeMap<String, u64>,
}
impl Cohort {
    fn add(&mut self, row: &Row) {
        self.swings += 1;
        *self
            .search_statuses
            .entry(row.search_status.clone())
            .or_default() += 1;
        *self
            .collision_statuses
            .entry(row.collision_status.clone())
            .or_default() += 1;
        let category = search_category(&row.search_status);
        *self
            .agreement
            .entry(row.legacy.into())
            .or_default()
            .entry(category.into())
            .or_default() += 1;
        if let Some(agree) = binary_agreement(row.legacy, category) {
            self.binary_comparable += 1;
            self.binary_agree += u64::from(agree);
        }
        self.reference_z_m.add(row.reference_z);
        let reference_category = row.reference_z.map(height_category).unwrap_or("no_state");
        let contact_category = row
            .contact_point
            .map(|p| height_category(p[2]))
            .unwrap_or("no_geometry");
        *self
            .reference_height_statuses
            .entry(reference_category.into())
            .or_default() += 1;
        *self
            .contact_height_statuses
            .entry(contact_category.into())
            .or_default() += 1;
        if let Some(point) = row.contact_point {
            *self
                .height_transitions
                .entry(reference_category.into())
                .or_default()
                .entry(contact_category.into())
                .or_default() += 1;
            self.contact_x_m.add(Some(point[0]));
            self.contact_y_m.add(Some(point[1]));
            self.contact_z_m.add(Some(point[2]));
        }
        if let Some(search) = row.search.filter(|s| s.status == "contact") {
            self.contact_time_offset_ms
                .add(search.time_offset_sec.map(|x| x * 1000.0));
            if let Some(g) = &search.geometry {
                self.axial_m.add(Some(g.axial_m));
                self.normal_z.add(g.normal.map(|n| n[2]));
                self.centerline_speed_ms.add(Some(
                    g.bat_velocity.iter().map(|x| x * x).sum::<f64>().sqrt(),
                ));
            }
        }
        if let Some(outcome) = row.outcome {
            self.outcomes.add(outcome);
            if contact_category == "clear_of_ground" {
                self.ground_valid_outcomes.add(outcome);
            } else {
                self.ground_invalid_outcomes.add(outcome);
            }
            let status = if row.baseline.is_some() {
                "paired"
            } else if row.legacy == "contact" {
                "missing_live_launch_metrics"
            } else if row.legacy == "miss" {
                "legacy_miss"
            } else {
                "unknown_legacy"
            };
            *self
                .paired_baseline_statuses
                .entry(status.into())
                .or_default() += 1;
        }
        if let (Some(baseline), Some(outcome)) = (row.baseline, row.outcome) {
            self.paired_live.add(baseline);
            self.paired_3d.add(outcome);
        }
    }
    fn json(&self) -> Value {
        let mut v = serde_json::to_value(self).unwrap();
        v["binary_agreement_rate"] = if self.binary_comparable > 0 {
            json!(self.binary_agree as f64 / self.binary_comparable as f64)
        } else {
            Value::Null
        };
        v
    }
}
fn binary_agreement(legacy: &str, search: &str) -> Option<bool> {
    match (legacy, search) {
        ("contact", "contact") | ("miss", "no_contact") => Some(true),
        ("contact", "no_contact") | ("miss", "contact") => Some(false),
        _ => None,
    }
}
#[derive(Default)]
pub struct SelectedAudit {
    records: u64,
    legacy_joins: BTreeMap<String, u64>,
    by_intent: BTreeMap<String, Cohort>,
    by_contact_band: BTreeMap<String, Cohort>,
    by_legacy_quality: BTreeMap<String, Cohort>,
    by_height: BTreeMap<String, Cohort>,
    invalid_ground_examples: BTreeMap<String, Vec<Value>>,
    checks_max: BTreeMap<String, f64>,
    examples: BTreeMap<String, Vec<Value>>,
}
impl SelectedAudit {
    pub fn observe(&mut self, schedule: &GameSchedule, game: &GameResult) -> Result<()> {
        let mut seen = std::collections::BTreeSet::new();
        for record in &game.player_contact_states {
            self.records += 1;
            ensure!(
                seen.insert(record.count_seq),
                "duplicate swing contact-state count_seq"
            );
            if let Some(observed) = record
                .state
                .as_ref()
                .and_then(|s| s.observed_intent_plans.as_ref())
            {
                ensure!(
                    observed.game_id == game.id && observed.count_seq == record.count_seq,
                    "contact diagnostic clock identity mismatch"
                );
            }
            let batter = record
                .state
                .as_ref()
                .and_then(|s| s.intent_batter.as_ref())
                .map(|b| b.batter_id);
            let candidates: Vec<_> = game
                .player_battings
                .iter()
                .filter(|b| b.count_seq == record.count_seq && Some(b.batter_id) == batter)
                .collect();
            let b = if candidates.len() == 1 {
                Some(candidates[0])
            } else {
                None
            };
            let join = if batter.is_none() {
                "missing_batter_identity"
            } else if candidates.is_empty() {
                "missing_batting_record"
            } else if candidates.len() > 1 {
                "duplicate_batting_records"
            } else {
                "unique_batting_record"
            };
            *self.legacy_joins.entry(join.into()).or_default() += 1;
            let row = evaluate(record, b, &schedule.stadium)
                .with_context(|| format!("game {}, count {}", game.id, record.count_seq))?;
            self.by_intent
                .entry(row.intent.clone())
                .or_default()
                .add(&row);
            self.by_legacy_quality
                .entry(format!("{}|{}", row.intent, row.quality))
                .or_default()
                .add(&row);
            if let Some(key) = &row.band {
                self.by_contact_band
                    .entry(key.clone())
                    .or_default()
                    .add(&row);
            }
            if let Some(point) = row.contact_point {
                self.by_height
                    .entry(format!("{}|{}", row.intent, height_category(point[2])))
                    .or_default()
                    .add(&row);
            }
            for (key, v) in &row.checks {
                let m = self.checks_max.entry(key.clone()).or_default();
                *m = m.max(*v);
            }
            let sample = || {
                json!({"game_id":game.id,"count_seq":record.count_seq,"batter_id":batter,"legacy":row.legacy,
                "legacy_quality":row.quality,"search_status":row.search_status,"collision_status":row.collision_status,
                "reference_position_world_m":record.state.as_ref().map(|s|s.input.ball_position_world_m),
                "reference_velocity_world_ms":record.state.as_ref().map(|s|s.input.ball_velocity_world_ms),
                "contact_height_category":row.contact_point.map(|p|height_category(p[2])),
                "contact_point_world_m":row.contact_point,"selected_search":row.search,"selected_evaluation":row.evaluation})
            };
            let examples = self
                .examples
                .entry(format!(
                    "{}|{}|{}",
                    row.intent,
                    search_category(&row.search_status),
                    row.collision_status
                ))
                .or_default();
            if examples.len() < 2 {
                examples.push(sample());
            }
            if let Some(point) = row
                .contact_point
                .filter(|p| height_category(p[2]) != "clear_of_ground")
            {
                let examples = self
                    .invalid_ground_examples
                    .entry(format!("{}|{}", row.intent, height_category(point[2])))
                    .or_default();
                if examples.len() < 2 {
                    examples.push(sample());
                }
            }
        }
        Ok(())
    }
    pub fn json(&self) -> Value {
        let groups = |groups: &BTreeMap<String, Cohort>| {
            groups
                .iter()
                .map(|(k, v)| (k.clone(), v.json()))
                .collect::<BTreeMap<_, _>>()
        };
        json!({"source":"selected_intent_forward_3d_v2","finite_model":finite::MODEL,"search_settings":ContactSearchSettings::default(),
            "swing_records":self.records,"legacy_join_coverage":self.legacy_joins,"by_intent":groups(&self.by_intent),
            "by_intent_and_contact_band":groups(&self.by_contact_band),"by_intent_and_legacy_quality":groups(&self.by_legacy_quality),
            "ball_radius_m":ball_radius_m(),"by_intent_and_height_validity":groups(&self.by_height),"invalid_ground_examples":self.invalid_ground_examples,
            "physics_checks_max":self.checks_max,"examples":self.examples,
            "definitions":["All recorded swings, including legacy misses, are the source denominator; takes have no contact state. Only the selected Intent is aggregated. Missing/duplicate legacy joins are unknown, never misses.",
                "Agreement contact is a successful forward search (including unsupported caps), miss is only no_contact. Unresolved/overlap_at_start/unavailable remain distinct and are excluded from binary agreement. Every source swing has one search status and one final collision/flight status.",
                "Use selected observed ExecutionSwingPlan geometry/velocity/time and existing finite collision response, not a pose inferred from legacy offsets. Decisions/identity/unique selection and settings must agree. Re-derived selected collision input/control/response must exactly reproduce stored diagnostics; physics identity residuals must be <=1e-7.",
                "Ground validity classifies the searched ball center: z<0 is center_below_ground, 0<=z<ball_radius is sphere_intersects_ground, z>=ball_radius is clear_of_ground. Existing outcomes are retained; ground_valid/invalid_outcomes partition accepted flights. Reference-to-contact height transitions and dedicated invalid samples trace the origin; no clamping or live behavior changes.",
                "Contact bands cover geometric contacts even when collision/flight is unavailable. Outcomes cover accepted selected 3-D flights including those on legacy misses. paired_live/paired_3d cover only accepted flights with a reproducible live contact baseline; they exclude legacy misses and cannot represent all swings.",
                "Flight uses the existing standard impact height (.90m) and home-plate origin, not the searched 3-D contact position. Launch metrics/normal/velocity come directly from the 3-D collision; actual contact point is reported separately. Transverse spin drives flight; total/gyro spin are reported separately. Geometry and dynamics are still diagnostic model assumptions, not measured reality. Non-foul includes HR/automatic doubles, not predicted hits. No downstream fielding/running or alternate game is replayed.",
                "States are collected only for one in-memory game and discarded after aggregation. No DB/contact-state writes. Existing deterministic sensor noise is keyed by game/count and does not consume the game RNG; match ordinary game reports to verify recording parity.",
                "Examples are the first two per intent/search-category/final-status, not random. Geometry summaries use every successful search, with separate n for missing values. Unknown legacy comparison coverage remains explicit."]})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use saberbb::domain::shared::{
        contact_state::{ContactStateSnapshot, IntentBatterContext},
        game::GameType,
        intent_collision_response::IntentCollisionDiagnostics,
        team::Team,
    };
    fn fixture(index: usize) -> (GameSchedule, GameResult) {
        let fixtures: Value = serde_json::from_str(include_str!(
            "../../tests/fixtures/finite_collision_contract.json"
        ))
        .unwrap();
        let case = &fixtures["integration"][index];
        let mut state: ContactStateSnapshot =
            serde_json::from_value(case["state"].clone()).unwrap();
        state.intent_batter = Some(IntentBatterContext {
            batter_id: 7,
            is_top: true,
        });
        let mut collision: IntentCollisionDiagnostics =
            serde_json::from_value(case["expected"].clone()).unwrap();
        for cp in &mut collision.probes {
            let p = state
                .execution_contact
                .as_ref()
                .unwrap()
                .probes
                .iter()
                .find(|p| p.intent == cp.intent)
                .unwrap();
            cp.evaluation =
                Some(intent_collision_response::evaluate(&state, p, &collision.spin).unwrap());
        }
        state.intent_collision = Some(collision);
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
        game.player_contact_states.push(ContactStateRecord {
            count_seq: 1,
            state_version: 1,
            state_source: "fixture".into(),
            invalid_reason: None,
            state: Some(state),
        });
        (schedule, game)
    }
    #[test]
    fn below_ground_contact_has_dedicated_examples_and_preserves_shadow_flight() {
        let (schedule, mut game) = fixture(0);
        let state = game.player_contact_states[0].state.as_mut().unwrap();
        // Translate ball and execution pivots together; local contact geometry/velocity is unchanged.
        state.input.ball_position_world_m[2] -= 2.0;
        for plan in &mut state
            .observed_intent_plans
            .as_mut()
            .unwrap()
            .intent_plans
            .as_mut()
            .unwrap()
            .plans
        {
            if let Some(plan) = &mut plan.execution_plan {
                plan.start_pivot_world_m[2] -= 2.0;
            }
        }
        let mut audit = SelectedAudit::default();
        audit.observe(&schedule, &game).unwrap();
        let v = audit.json();
        let c = &v["by_intent"]["LineDrive"];
        assert_eq!(c["contact_height_statuses"]["center_below_ground"], 1);
        assert_eq!(c["outcomes"]["all"]["n"], 1);
        assert_eq!(c["ground_invalid_outcomes"]["all"]["n"], 1);
        assert_eq!(c["ground_valid_outcomes"]["all"]["n"], 0);
        let examples = v["invalid_ground_examples"]["LineDrive|center_below_ground"]
            .as_array()
            .unwrap();
        assert_eq!(examples.len(), 1);
        assert!(examples[0]["contact_point_world_m"][2].as_f64().unwrap() < 0.0);
    }
    #[test]
    fn height_boundaries_use_ball_center_and_radius_without_clamping() {
        assert_eq!(height_category(-0.1), "center_below_ground");
        assert_eq!(height_category(0.0), "sphere_intersects_ground");
        assert_eq!(
            height_category(ball_radius_m() - 1e-8),
            "sphere_intersects_ground"
        );
        assert_eq!(height_category(ball_radius_m()), "clear_of_ground");
        assert_eq!(height_category(f64::NAN), "non_finite");
    }
    #[test]
    fn invalid_height_keeps_original_outcome_and_has_separate_valid_cohort() {
        use saberbb::domain::shared::ball::BattedBall;
        let mut row = Row::new(None);
        row.reference_z = Some(-0.2);
        row.contact_point = Some([0.0, 0.0, -0.1]);
        row.search_status = "contact".into();
        row.collision_status = "accepted_flight".into();
        row.outcome = Some(Outcome {
            ball: BattedBall::default(),
            hla: 0.0,
            total_spin: None,
            gyro_spin: None,
        });
        let mut c = Cohort::default();
        c.add(&row);
        let v = c.json();
        assert_eq!(v["outcomes"]["all"]["n"], 1);
        assert_eq!(v["ground_invalid_outcomes"]["all"]["n"], 1);
        assert_eq!(v["ground_valid_outcomes"]["all"]["n"], 0);
        assert_eq!(
            v["height_transitions"]["center_below_ground"]["center_below_ground"],
            1
        );
        row.contact_point = Some([0.0, 0.0, 0.1]);
        c.add(&row);
        let v = c.json();
        assert_eq!(v["outcomes"]["all"]["n"], 2);
        assert_eq!(v["ground_invalid_outcomes"]["all"]["n"], 1);
        assert_eq!(v["ground_valid_outcomes"]["all"]["n"], 1);
    }
    #[test]
    fn aggregates_only_selected_response_without_mutating_or_inventing_legacy_join() {
        for (index, name) in [(0, "LineDrive"), (4, "ContactFirst")] {
            let (s, mut g) = fixture(index);
            let state = g.player_contact_states[0].state.as_mut().unwrap();
            let unselected = state
                .intent_collision
                .as_mut()
                .unwrap()
                .probes
                .iter_mut()
                .find(|p| !p.selected)
                .unwrap();
            unselected.evaluation.as_mut().unwrap().response =
                json!({"collision_status":"invalid_unselected_response"});
            let before = serde_json::to_value(&g).unwrap();
            let mut audit = SelectedAudit::default();
            audit.observe(&s, &g).unwrap();
            assert_eq!(before, serde_json::to_value(&g).unwrap());
            let v = audit.json();
            assert_eq!(v["swing_records"], 1);
            assert_eq!(v["legacy_join_coverage"]["missing_batting_record"], 1);
            let c = &v["by_intent"][name];
            assert_eq!(c["outcomes"]["all"]["n"], 1);
            assert_eq!(c["binary_comparable"], 0);
            assert_eq!(c["binary_agreement_rate"], Value::Null);
            assert_eq!(c["agreement"]["unknown"]["contact"], 1);
            assert_eq!(c["paired_baseline_statuses"]["unknown_legacy"], 1);
            assert_eq!(c["paired_live"]["all"]["n"], 0);
        }
    }
    #[test]
    fn legacy_miss_with_3d_contact_is_a_disagreement_and_keeps_its_outcome() {
        use saberbb::domain::shared::{
            ball::BattedBall, game::BattingResult, game_stats::PlayerGameBattingMetrics,
        };
        let (s, mut g) = fixture(0);
        g.add_player_batting(
            1,
            10,
            7,
            PlayerGameBattingMetrics {
                adjusted_contact_type: SwingContactType::SwungAndMiss,
                ..Default::default()
            },
            BattedBall::default(),
            None,
            BattingResult::StrikeSwung,
        );
        let mut audit = SelectedAudit::default();
        audit.observe(&s, &g).unwrap();
        let v = audit.json();
        let c = &v["by_intent"]["LineDrive"];
        assert_eq!(c["agreement"]["miss"]["contact"], 1);
        assert_eq!(c["binary_comparable"], 1);
        assert_eq!(c["binary_agreement_rate"], 0.0);
        assert_eq!(c["outcomes"]["all"]["n"], 1);
        assert_eq!(c["paired_baseline_statuses"]["legacy_miss"], 1);
    }
    #[test]
    fn invalid_pitch_spin_keeps_geometric_contact_but_no_flight() {
        let (s, g) = fixture(1);
        let mut audit = SelectedAudit::default();
        audit.observe(&s, &g).unwrap();
        let v = audit.json();
        let c = &v["by_intent"]["LineDrive"];
        assert_eq!(c["search_statuses"]["contact"], 1);
        assert_eq!(
            c["collision_statuses"]["not_evaluated:invalid_pitch_spin"],
            1
        );
        assert_eq!(c["outcomes"]["all"]["n"], 0);
    }
    #[test]
    fn duplicate_selection_and_tampered_selected_responses_are_rejected() {
        let (s, mut g) = fixture(0);
        let mut bad = g.clone();
        for p in &mut bad.player_contact_states[0]
            .state
            .as_mut()
            .unwrap()
            .execution_contact
            .as_mut()
            .unwrap()
            .probes
        {
            p.selected = true;
        }
        assert!(SelectedAudit::default().observe(&s, &bad).is_err());
        let p = g.player_contact_states[0]
            .state
            .as_mut()
            .unwrap()
            .intent_collision
            .as_mut()
            .unwrap()
            .probes
            .iter_mut()
            .find(|p| p.selected)
            .unwrap();
        p.evaluation.as_mut().unwrap().response["ev_ms"] = json!(999.0);
        assert!(SelectedAudit::default().observe(&s, &g).is_err());
    }
    #[test]
    fn incomplete_searches_never_become_misses_or_comparable_agreements() {
        for status in ["no_contact", "unresolved", "overlap_at_start"] {
            let (s, mut g) = fixture(0);
            let p = g.player_contact_states[0]
                .state
                .as_mut()
                .unwrap()
                .execution_contact
                .as_mut()
                .unwrap()
                .probes
                .iter_mut()
                .find(|p| p.selected)
                .unwrap();
            p.search.as_mut().unwrap().status = status.into();
            let mut audit = SelectedAudit::default();
            audit.observe(&s, &g).unwrap();
            let v = audit.json();
            let c = &v["by_intent"]["LineDrive"];
            assert_eq!(c["search_statuses"][status], 1);
            assert_eq!(c["outcomes"]["all"]["n"], 0);
            assert_eq!(c["binary_comparable"], 0);
        }
        assert_eq!(binary_agreement("miss", "no_contact"), Some(true));
        assert_eq!(binary_agreement("contact", "no_contact"), Some(false));
        assert_eq!(binary_agreement("contact", "contact"), Some(true));
        assert_eq!(binary_agreement("miss", "unresolved"), None);
        assert_eq!(binary_agreement("contact", "overlap_at_start"), None);
    }
}
