//! Same recorded contact, several collision/flight responses. Never reruns a game or draws RNG.
use super::contact_direction_audit::{checked_baseline, direction_class};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    resolver::batting_resolver::{SwingContactType, calculate_trajectory},
    shared::{
        ball::{BattedBall, OutboundResult},
        finite_bat_collision as finite, frozen_contact_collision as frozen,
        game::{BattingResult, GameResult, GameSchedule},
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
#[derive(Default, Serialize)]
pub(super) struct Moments {
    n: u64,
    mean: Option<f64>,
    min: Option<f64>,
    max: Option<f64>,
}
impl Moments {
    pub(super) fn add(&mut self, v: Option<f64>) {
        if let Some(v) = v.filter(|v| v.is_finite()) {
            self.n += 1;
            self.mean = Some(self.mean.unwrap_or(v) + (v - self.mean.unwrap_or(v)) / self.n as f64);
            self.min = Some(self.min.map_or(v, |x| x.min(v)));
            self.max = Some(self.max.map_or(v, |x| x.max(v)));
        }
    }
}
pub(super) fn band(v: f64, edges: &[f64]) -> String {
    let i = edges.partition_point(|e| v >= *e);
    if i == 0 {
        format!("<{}", edges[0])
    } else if i == edges.len() {
        format!(">={}", edges[i - 1])
    } else {
        format!("[{},{})", edges[i - 1], edges[i])
    }
}
#[derive(Clone, Copy)]
pub(super) struct Outcome {
    pub(super) ball: BattedBall,
    pub(super) hla: f64,
    pub(super) total_spin: Option<f64>,
    pub(super) gyro_spin: Option<f64>,
}
#[derive(Default, Serialize)]
struct Distribution {
    n: u64,
    ev_ms: Moments,
    vla_deg: Moments,
    hla_deg: Moments,
    abs_hla_deg: Moments,
    trajectory_spin_rpm: Moments,
    total_spin_rpm: Moments,
    gyro_spin_rpm: Moments,
    ev_bands: BTreeMap<String, u64>,
    vla_bands: BTreeMap<String, u64>,
    hla_bands: BTreeMap<String, u64>,
    spin_bands: BTreeMap<String, u64>,
    ev_vla_bands: BTreeMap<String, u64>,
}
impl Distribution {
    fn add(&mut self, o: Outcome) {
        self.n += 1;
        self.ev_ms.add(Some(o.ball.launch_speed));
        self.vla_deg.add(Some(o.ball.launch_angle));
        self.hla_deg.add(Some(o.hla));
        self.abs_hla_deg.add(Some(o.hla.abs()));
        self.trajectory_spin_rpm.add(Some(o.ball.spin_rate));
        self.total_spin_rpm.add(o.total_spin);
        self.gyro_spin_rpm.add(o.gyro_spin);
        let ev = band(o.ball.launch_speed, &[20.0, 35.0, 45.0]);
        let vla = band(o.ball.launch_angle, &[-10.0, 10.0, 25.0, 40.0]);
        *self.ev_bands.entry(ev.clone()).or_default() += 1;
        *self.vla_bands.entry(vla.clone()).or_default() += 1;
        *self
            .hla_bands
            .entry(band(o.hla, &[-90.0, -45.0, -20.0, 0.0, 20.0, 45.0, 90.0]))
            .or_default() += 1;
        *self
            .spin_bands
            .entry(band(o.ball.spin_rate, &[1000.0, 3000.0, 6000.0]))
            .or_default() += 1;
        *self.ev_vla_bands.entry(format!("{ev}|{vla}")).or_default() += 1;
    }
}
#[derive(Default, Serialize)]
pub(super) struct Group {
    all: Distribution,
    non_foul: Distribution,
    geometric_foul: u64,
    hr: u64,
    ground_rule_double: u64,
}
impl Group {
    pub(super) fn add(&mut self, o: Outcome) {
        self.all.add(o);
        match direction_class(&o.ball) {
            OutboundResult::Foul => self.geometric_foul += 1,
            value => {
                self.non_foul.add(o);
                self.hr += u64::from(value == OutboundResult::HomeRun);
                self.ground_rule_double += u64::from(value == OutboundResult::GroundRuleDouble);
            }
        }
    }
}
#[derive(Default, Serialize)]
struct Paired {
    attempted: u64,
    statuses: BTreeMap<String, u64>,
    baseline: Group,
    trial: Group,
    foul_to_non_foul: u64,
    non_foul_to_foul: u64,
}
impl Paired {
    fn accepted(&mut self, baseline: Outcome, trial: Outcome) {
        *self.statuses.entry("accepted".into()).or_default() += 1;
        self.baseline.add(baseline);
        self.trial.add(trial);
        let before = direction_class(&baseline.ball) == OutboundResult::Foul;
        let after = direction_class(&trial.ball) == OutboundResult::Foul;
        self.foul_to_non_foul += u64::from(before && !after);
        self.non_foul_to_foul += u64::from(!before && after);
    }
    fn skip(&mut self, reason: &str) {
        *self.statuses.entry(reason.into()).or_default() += 1;
    }
}
#[derive(Default)]
pub struct MovingAudit {
    contacts: u64,
    reconstructed: u64,
    coverage: BTreeMap<String, u64>,
    coverage_by_quality: BTreeMap<String, BTreeMap<String, u64>>,
    recorded_fouls: BTreeMap<String, u64>,
    baseline: BTreeMap<String, Group>,
    cases: BTreeMap<String, BTreeMap<String, Paired>>,
    controlled_pairs: BTreeMap<String, BTreeMap<String, Paired>>,
    checks_max: BTreeMap<String, f64>,
    examples: BTreeMap<String, Vec<Value>>,
}
pub(super) fn vector(v: &Value, name: &str) -> Result<[f64; 3]> {
    let a = v[name].as_array().context("missing response vector")?;
    ensure!(a.len() == 3, "invalid response vector size");
    let mut out = [0.0; 3];
    for i in 0..3 {
        out[i] = a[i]
            .as_f64()
            .filter(|v| v.is_finite())
            .context("invalid response vector")?;
    }
    Ok(out)
}
impl MovingAudit {
    fn cover(&mut self, quality: &str, key: &str) {
        *self
            .coverage_by_quality
            .entry(quality.into())
            .or_default()
            .entry(key.into())
            .or_default() += 1;
        *self.coverage.entry(key.into()).or_default() += 1;
    }
    pub fn observe(&mut self, schedule: &GameSchedule, game: &GameResult) -> Result<()> {
        for b in &game.player_battings {
            if !matches!(
                b.adjusted_contact_type,
                SwingContactType::SolidContact
                    | SwingContactType::WeakContact
                    | SwingContactType::MarginalContact
            ) {
                continue;
            }
            self.contacts += 1;
            let quality = format!("{:?}", b.adjusted_contact_type);
            *self.recorded_fouls.entry(quality.clone()).or_default() +=
                u64::from(b.result == BattingResult::Foul);
            let Some(ball) = checked_baseline(b, &schedule.stadium)
                .with_context(|| format!("game {} count {}", game.id, b.count_seq))?
            else {
                self.cover(&quality, "missing_baseline_components");
                continue;
            };
            self.reconstructed += 1;
            let baseline = Outcome {
                ball,
                hla: b.final_hla_deg,
                total_spin: None,
                gyro_spin: None,
            };
            self.baseline
                .entry(quality.clone())
                .or_default()
                .add(baseline);
            let pitches: Vec<_> = game
                .player_pitchings
                .iter()
                .filter(|p| p.count_seq == b.count_seq && p.pitcher_id == b.pitcher_id)
                .collect();
            if pitches.len() != 1 {
                self.cover(&quality, "pitch_join_not_unique");
                continue;
            }
            let frozen = match frozen::from_record(b, &pitches[0].ball) {
                Ok(v) => v,
                Err(reason) => {
                    self.cover(&quality, &format!("adapter:{reason}"));
                    continue;
                }
            };
            self.cover(&quality, "frozen_input_available");
            let mut accepted_outcomes = BTreeMap::new();
            for case in [
                "stationary_normal",
                "moving_normal",
                "moving_finite",
                "moving_normal_hla_only",
            ] {
                let paired = self
                    .cases
                    .entry(quality.clone())
                    .or_default()
                    .entry(case.into())
                    .or_default();
                paired.attempted += 1;
                let input = frozen::case_input(&frozen.input, case);
                let response = match finite::solve(&input) {
                    Ok(v) => v,
                    Err(reason) => {
                        paired.skip(&format!("solver_error:{reason}"));
                        continue;
                    }
                };
                let status = response["collision_status"]
                    .as_str()
                    .context("missing collision status")?;
                if status != "impulse_contact" {
                    paired.skip(status);
                    continue;
                }
                let checks = response["checks"]
                    .as_object()
                    .context("missing physics checks")?;
                for (name, v) in checks {
                    let v = v.as_f64().context("invalid physics check")?;
                    ensure!(
                        v.is_finite() && (0.0..=1e-7).contains(&v),
                        "physics identity failed: {name}: {v}"
                    );
                    let maximum = self.checks_max.entry(name.clone()).or_default();
                    *maximum = maximum.max(v);
                }
                let velocity = vector(&response, "outgoing_world_ms")?;
                let spin = match frozen::flight_spin(
                    velocity,
                    vector(&response, "outgoing_spin_rad_s")?,
                ) {
                    Ok(v) => v,
                    Err(reason) => {
                        paired.skip(reason);
                        continue;
                    }
                };
                let hla = response["hla_deg"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                    .context("invalid HLA")?;
                let (ev, vla, rpm, angle, total, gyro) = if case == "moving_normal_hla_only" {
                    (
                        ball.launch_speed,
                        ball.launch_angle,
                        ball.spin_rate,
                        ball.spin_angle,
                        None,
                        None,
                    )
                } else {
                    (
                        response["ev_ms"].as_f64().context("invalid EV")?,
                        response["vla_deg"].as_f64().context("invalid VLA")?,
                        spin.transverse_rpm,
                        spin.angle_deg,
                        Some(spin.total_rpm),
                        Some(spin.gyro_rpm),
                    )
                };
                let trial = match calculate_trajectory(ev, vla, hla, rpm, angle, &schedule.stadium)
                {
                    Ok(ball) => Outcome {
                        ball,
                        hla,
                        total_spin: total,
                        gyro_spin: gyro,
                    },
                    Err(_) => {
                        paired.skip("flight_error");
                        continue;
                    }
                };
                paired.accepted(baseline, trial);
                accepted_outcomes.insert(case, trial);
                let key = format!("{quality}|{case}");
                let samples = self.examples.entry(key).or_default();
                if samples.len() < 2 {
                    samples.push(json!({"game_id":game.id,"count_seq":b.count_seq,"batter_id":b.batter_id,
                        "recorded_result":b.result,"frozen":frozen,"case_input":input,"response":response,"flight_spin":spin,
                        "baseline":{"hla_deg":baseline.hla,"ball":baseline.ball},"trial":{"hla_deg":hla,"ball":trial.ball}}));
                }
            }
            for (name, from, to) in [
                (
                    "stationary_to_moving_normal",
                    "stationary_normal",
                    "moving_normal",
                ),
                ("moving_normal_to_finite", "moving_normal", "moving_finite"),
            ] {
                let pair = self
                    .controlled_pairs
                    .entry(quality.clone())
                    .or_default()
                    .entry(name.into())
                    .or_default();
                pair.attempted += 1;
                if let (Some(before), Some(after)) =
                    (accepted_outcomes.get(from), accepted_outcomes.get(to))
                {
                    pair.accepted(*before, *after);
                } else {
                    pair.skip("one_or_both_unavailable");
                }
            }
        }
        Ok(())
    }
    pub fn json(&self) -> Value {
        json!({"source":frozen::SOURCE,"finite_model":finite::MODEL,"contacts":self.contacts,"reconstructed":self.reconstructed,
            "coverage":self.coverage,"coverage_by_quality":self.coverage_by_quality,"recorded_fouls_by_quality":self.recorded_fouls,"baseline_all_contacts":self.baseline,
            "paired_by_quality_and_case":self.cases,"controlled_pairs":self.controlled_pairs,"physics_checks_max":self.checks_max,"examples":self.examples,
            "definitions":["All recorded contacts frozen; no search or new contact classification. Reconstructed live HLA/flight must match first. Baseline mismatch aborts. Missing data/unsupported geometry are counted, never filled with zero.",
                "Pose is an explicit proxy: retain signed local length/thickness, infer the leading cylindrical surface (radius .070m), mirror X for handedness, rotate tilt/normal by recorded full face angle. Infer attack from recorded VLA minus live vertical deflection. There is no measured 3-D legacy pose.",
                "Reference bat velocity uses recorded bat speed/attack in world Y/Z. Angular velocity is axis cross reference velocity / .9m. Local centerline speed includes axial offset; half length .16m. No observed-intent plan replaces this pose; the full face angle is a geometric yaw, without the live .85 output-angle multiplier.",
                "Existing finite model and restitution profile at .71m+signed length are frozen. Out-of-curve/cap inputs are excluded, not clamped. Incoming plate velocity uses the live constant-acceleration model. Unrecorded gyro sign is omitted (same convention as existing diagnostics).",
                "stationary_normal zeroes bat motion and friction; moving_normal retains motion but zeroes friction. moving_finite uses frozen mu=.2, et=.4. moving_normal_hla_only uses only the moving_normal HLA with recorded EV/VLA/spin, to isolate direction sensitivity.",
                "Controlled pairs use the intersection of successful cases on the very same contact: stationary_to_moving_normal isolates motion; moving_normal_to_finite isolates tangential response. Each case has its own matched baseline/trial denominator. attempted equals sum(statuses); accepted equals both group all.n. Non-foul includes HR and automatic doubles, not predicted hits. Geometric fouls may be caught foul flies; fielding/strategy/subsequent pitches are not replayed.",
                "Flight uses transverse spin only and projects lift into the live integrator spin-angle basis; total/gyro spin are separately reported. Baseline total/gyro spin are unknown, not zero. Fixed EV/VLA HLA-only spin remains the recorded scalar. Histograms and moments cover all and non-foul contacts separately. Arithmetic HLA mean is accompanied by signed bins and absolute mean because angles wrap.",
                "Examples are first two per quality/case, not random. This is model sensitivity with explicit pose assumptions, not a proposed live model or a full-season counterfactual."]})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn histogram_labels_match_half_open_boundaries() {
        assert_eq!(band(35.0, &[20.0, 35.0, 45.0]), "[35,45)");
        assert_eq!(band(45.0, &[20.0, 35.0, 45.0]), ">=45");
    }
    #[test]
    fn separate_matched_denominators_and_rejected_trials() {
        let mut p = Paired::default();
        let mut ball = BattedBall::default();
        ball.final_position.angle = 80.0;
        let baseline = Outcome {
            ball,
            hla: 80.0,
            total_spin: None,
            gyro_spin: None,
        };
        let mut trial = baseline;
        trial.ball.final_position.angle = 0.0;
        trial.hla = 0.0;
        p.attempted = 2;
        p.accepted(baseline, trial);
        p.skip("non_approaching");
        assert_eq!(
            (p.baseline.all.n, p.trial.all.n, p.foul_to_non_foul),
            (1, 1, 1)
        );
        assert_eq!((p.baseline.non_foul.n, p.trial.non_foul.n), (0, 1));
        assert_eq!(p.trial.all.total_spin_rpm.n, 0);
        assert_eq!(p.statuses.values().sum::<u64>(), p.attempted);
    }
}
