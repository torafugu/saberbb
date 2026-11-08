//! Frozen-contact HLA ablations: flight only, never a game or RNG.
use anyhow::{Result, ensure};
use saberbb::domain::{
    resolver::batting_resolver::{SwingContactType, calculate_trajectory, contact_normal_hla_deg},
    shared::{
        ball::{BattedBall, OutboundResult},
        game::{BattingResult, GameResult, GameSchedule},
        game_stats::PlayerGameBatting,
    },
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
fn wrap(a: f64) -> f64 {
    (a + 180.0).rem_euclid(360.0) - 180.0
}
#[derive(Clone, Copy)]
struct Components {
    normal: f64,
    approach: f64,
    pull: f64,
    timing: f64,
    sign: f64,
}
impl Components {
    fn recorded(b: &PlayerGameBatting) -> Option<Self> {
        if b.hla_model_version != Some(2) {
            return None;
        }
        let angle = b.actual_bat_angle_deg?;
        let x = b.contact_offset_x_m?;
        let z = b.contact_offset_z_m?;
        // Match the live recomputation rather than assuming the optional signed metric agrees.
        let thickness = -x * angle.to_radians().sin() + z * angle.to_radians().cos();
        let c = Self {
            normal: contact_normal_hla_deg(thickness, angle),
            approach: b.approach_variation_deg * 0.85,
            pull: b.pull_bias_deg * 0.85,
            timing: b.timing_angle_error * 0.85,
            sign: b.hla_handedness_sign?,
        };
        ([x, z, angle, c.normal, c.approach, c.pull, c.timing, c.sign]
            .iter()
            .all(|v| v.is_finite())
            && c.sign.abs() == 1.0)
            .then_some(c)
    }
    fn hla(self, omit: &str) -> f64 {
        wrap(
            self.sign
                * (if omit == "normal" { 0.0 } else { self.normal }
                    + if omit == "approach" {
                        0.0
                    } else {
                        self.approach
                    }
                    + if omit == "pull" { 0.0 } else { self.pull }
                    + if omit == "timing" { 0.0 } else { self.timing }),
        )
    }
}
#[derive(Default, Serialize)]
struct Case {
    n: u64,
    geometric_foul: u64,
    non_foul: u64,
    hr: u64,
    ground_rule_double: u64,
    recorded_foul_to_non_foul: u64,
    recorded_non_foul_to_geometric_foul: u64,
    replay_errors: u64,
    abs_hla_sum_deg: f64,
}
impl Case {
    fn add(&mut self, hla: f64, recorded_foul: bool, trial: Option<OutboundResult>) {
        self.n += 1;
        self.abs_hla_sum_deg += hla.abs();
        let Some(trial) = trial else {
            self.replay_errors += 1;
            return;
        };
        let foul = trial == OutboundResult::Foul;
        self.geometric_foul += u64::from(foul);
        self.non_foul += u64::from(!foul);
        self.hr += u64::from(trial == OutboundResult::HomeRun);
        self.ground_rule_double += u64::from(trial == OutboundResult::GroundRuleDouble);
        self.recorded_foul_to_non_foul += u64::from(recorded_foul && !foul);
        self.recorded_non_foul_to_geometric_foul += u64::from(!recorded_foul && foul);
    }
}
// Follow outbound early returns, then the later final-angle test. Foul fly catches
// can still be outs; those require fielding and are deliberately not replayed.
fn direction_class(ball: &BattedBall) -> OutboundResult {
    if ball.outbound_result == OutboundResult::InField && ball.is_foul() {
        OutboundResult::Foul
    } else {
        ball.outbound_result
    }
}
fn flight_matches(a: &BattedBall, b: &BattedBall) -> bool {
    a.outbound_result == b.outbound_result
        && [
            (a.final_position.x, b.final_position.x),
            (a.final_position.y, b.final_position.y),
            (a.total_time, b.total_time),
            (a.max_height, b.max_height),
        ]
        .iter()
        .all(|(a, b)| a.is_finite() && b.is_finite() && (a - b).abs() < 1e-7)
}
#[derive(Default)]
pub struct DirectionAudit {
    contacts: u64,
    missing_components: u64,
    reconstructed: u64,
    recorded_fouls: BTreeMap<String, u64>,
    cases: BTreeMap<String, BTreeMap<String, Case>>,
    components_abs_sum: BTreeMap<String, BTreeMap<String, f64>>,
    examples: Vec<Value>,
}
impl DirectionAudit {
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
            let Some(c) = Components::recorded(b) else {
                self.missing_components += 1;
                continue;
            };
            ensure!(
                wrap(c.hla("none") - b.final_hla_deg).abs() < 1e-8,
                "HLA reconstruction mismatch: game {}, count {}",
                game.id,
                b.count_seq
            );
            let flight = |hla| {
                calculate_trajectory(
                    b.ball.launch_speed,
                    b.ball.launch_angle,
                    hla,
                    b.ball.spin_rate,
                    b.ball.spin_angle,
                    &schedule.stadium,
                )
            };
            let baseline = flight(b.final_hla_deg)?;
            ensure!(
                flight_matches(&baseline, &b.ball),
                "Flight reconstruction mismatch: game {}, count {}",
                game.id,
                b.count_seq
            );
            self.reconstructed += 1;
            let quality = format!("{:?}", b.adjusted_contact_type);
            *self.recorded_fouls.entry(quality.clone()).or_default() +=
                u64::from(b.result == BattingResult::Foul);
            let components = self.components_abs_sum.entry(quality.clone()).or_default();
            for (name, value) in [
                ("normal", c.normal),
                ("approach", c.approach),
                ("pull", c.pull),
                ("timing", c.timing),
            ] {
                *components.entry(name.into()).or_default() += value.abs();
            }
            for omit in ["none", "normal", "approach", "pull", "timing"] {
                let hla = c.hla(omit);
                let trial = if omit == "none" {
                    Some(direction_class(&baseline))
                } else {
                    flight(hla).ok().map(|b| direction_class(&b))
                };
                self.cases
                    .entry(quality.clone())
                    .or_default()
                    .entry(omit.into())
                    .or_default()
                    .add(hla, b.result == BattingResult::Foul, trial);
                if omit == "normal"
                    && b.result == BattingResult::Foul
                    && trial.is_some_and(|v| v != OutboundResult::Foul)
                    && self.examples.len() < 6
                {
                    self.examples.push(json!({"game_id":game.id,"count_seq":b.count_seq,"quality":quality,
                        "ev_ms":b.ball.launch_speed,"vla_deg":b.ball.launch_angle,"actual_hla_deg":b.final_hla_deg,
                        "without_normal_hla_deg":hla,"normal_deg":c.normal,"approach_deg":c.approach,
                        "pull_deg":c.pull,"timing_deg":c.timing,"trial_outbound":trial}));
                }
            }
        }
        Ok(())
    }
    pub fn json(&self) -> Value {
        json!({"contacts":self.contacts,"missing_components":self.missing_components,"reconstructed":self.reconstructed,
            "recorded_fouls_by_quality":self.recorded_fouls,"cases_by_quality_and_omitted_component":self.cases,
            "components_abs_sum_deg":self.components_abs_sum,"examples":self.examples,
            "definitions":["Every contact including foul/HR is frozen. Cases omit exactly one additive HLA component; none is baseline.",
                "Baseline HLA and flight endpoint/time/height/outbound must reconstruct. Live flight integrates spin/fence, followed by the live final-angle foul test for InField balls; no RNG draws.",
                "Geometric fouls can include caught foul fly outs: fielding is not replayed. Recorded fouls are separately counted and used for transition counts.",
                "EV/VLA/spin/stadium are fixed. Removing normal is sensitivity analysis, not a proposed physical model. Non-foul includes HR/automatic doubles, not predicted hits; subsequent pitches, fielding, running and strategy are not replayed.",
                "Case n includes replay_errors; geometric_foul + non_foul + replay_errors = n. Component magnitudes are sums over reconstructed contacts; divide by baseline n per quality for means. Examples are the first six, not random."]})
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn omits_one_component_and_mirrors_after_wrapping() {
        let c = Components {
            normal: 170.0,
            approach: 4.0,
            pull: 6.0,
            timing: 20.0,
            sign: 1.0,
        };
        assert_eq!(c.hla("none"), -160.0);
        assert_eq!(c.hla("normal"), 30.0);
        assert_eq!(Components { sign: -1.0, ..c }.hla("normal"), -30.0);
    }
    #[test]
    fn stopped_foul_uses_final_angle_even_without_boundary_collision() {
        use saberbb::domain::{shared::stadium::Stadium, util::PolarPosition};
        let mut ball =
            calculate_trajectory(5.0, -20.0, 80.0, 0.0, 0.0, &Stadium::default()).unwrap();
        ball.outbound_result = OutboundResult::InField;
        ball.final_position = PolarPosition::new(2.0, 80.0);
        assert_eq!(direction_class(&ball), OutboundResult::Foul);
        ball.final_position = PolarPosition::new(2.0, 0.0);
        assert_eq!(direction_class(&ball), OutboundResult::InField);
    }
    #[test]
    fn flight_reproduction_detects_changed_endpoint_and_outbound() {
        use saberbb::domain::shared::stadium::Stadium;
        let ball = calculate_trajectory(40.0, 10.0, 0.0, 1000.0, 0.0, &Stadium::default()).unwrap();
        assert!(flight_matches(&ball, &ball));
        let mut changed = ball;
        changed.final_position.x += 0.01;
        assert!(!flight_matches(&ball, &changed));
        changed = ball;
        changed.outbound_result = OutboundResult::Foul;
        assert!(!flight_matches(&ball, &changed));
    }
    #[test]
    fn replay_errors_and_foul_transitions_have_distinct_denominators() {
        let mut c = Case::default();
        c.add(0.0, true, Some(OutboundResult::HomeRun));
        c.add(50.0, false, Some(OutboundResult::Foul));
        c.add(0.0, true, None);
        assert_eq!(
            (c.n, c.geometric_foul, c.non_foul, c.replay_errors),
            (3, 1, 1, 1)
        );
        assert_eq!(
            (
                c.recorded_foul_to_non_foul,
                c.recorded_non_foul_to_geometric_foul,
                c.hr
            ),
            (1, 1, 1)
        );
    }
}
