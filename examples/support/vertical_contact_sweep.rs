//! Frozen-contact vertical displacement, using the live bat-frame geometry and flight.
use super::{Contact, required, verify_trajectory};
use anyhow::{Context, Result, ensure};
use rusqlite::Statement;
use saberbb::domain::{
    random_provider::RealRng,
    resolver::batting_resolver::{
        SwingContactResult, calculate_collision_spin_from_offsets, calculate_dynamic_attack_angle,
        calculate_launch_speed_with_power, calculate_trajectory,
        calculate_vertical_contact_deflection, contact_geometry_is_miss, contact_normal_hla_deg,
        project_contact_offsets, sample_face_angle_terms, wrap_hla_deg,
    },
    seeded_diagnostic::derived_seed,
    shared::{ball::BattedBall, player::RL, stadium::Stadium},
};
use serde::Serialize;
use std::collections::BTreeMap;

const ALL_SWING_SHIFTS_MM: [i32; 4] = [0, 10, 15, 20];
const SHIFTS_MM: [i32; 9] = [-10, -5, 0, 5, 10, 15, 20, 25, 30];

struct Inputs {
    x: f64,
    z: f64,
    tilt: f64,
    depth: f64,
    pitch_speed: f64,
    pitch_spin: f64,
    pitch_axis: f64,
    bat_speed: f64,
    power: f64,
    handedness: f64,
    face_deg: f64,
    attack: f64,
}

impl Inputs {
    fn flight(&self, z: f64, stadium: &Stadium) -> Result<Option<BattedBall>> {
        let (length, thickness) = project_contact_offsets(self.x, z, self.tilt);
        if contact_geometry_is_miss(length, thickness) {
            return Ok(None);
        }
        let contact = SwingContactResult {
            length_offset_m: length.abs(),
            thickness_offset_m: thickness.abs(),
            timing_impact_x_m: self.depth,
            ..SwingContactResult::default()
        };
        let ev = calculate_launch_speed_with_power(
            &contact,
            self.pitch_speed,
            self.bat_speed,
            self.power,
        );
        let (spin, axis) = calculate_collision_spin_from_offsets(
            length.abs(),
            thickness.abs(),
            self.bat_speed,
            self.pitch_spin,
            self.pitch_axis,
        );
        let vla = self.attack + calculate_vertical_contact_deflection(z);
        let hla = wrap_hla_deg(
            (self.face_deg + contact_normal_hla_deg(thickness, self.tilt)) * self.handedness,
        );
        calculate_trajectory(ev, vla, hla, spin, axis, stadium)
            .map(Some)
            .map_err(|_| anyhow::anyhow!("trajectory_failed"))
    }
}

#[derive(Default, Serialize)]
struct Scenario {
    geometric_misses: u64,
    contacts: u64,
    lost_contacts: u64,
    recovered_contacts: u64,
    recovered_power_window: u64,
    recovered_home_runs: u64,
    fair_trajectories: u64,
    fair_ev_at_least_40: u64,
    power_window: u64,
    entered_power_window: u64,
    entries_with_baseline_ev: u64,
    mean_ev_change_for_entries_ms: f64,
    left_power_window: u64,
    home_runs: u64,
    mean_ev_ms: f64,
    mean_vla_deg: f64,
    mean_spin_rpm: f64,
}

fn in_power_window(ball: &BattedBall) -> bool {
    ball.outbound_result.as_ref() != "Foul"
        && ball.launch_speed >= 40.0
        && (20.0..50.0).contains(&ball.launch_angle)
}

impl Scenario {
    fn add(&mut self, ball: Option<BattedBall>, baseline: Option<&BattedBall>) {
        let baseline_in_window = baseline.is_some_and(in_power_window);
        let in_window = ball.as_ref().is_some_and(in_power_window);
        self.entered_power_window += u64::from(in_window && !baseline_in_window);
        self.left_power_window += u64::from(!in_window && baseline_in_window);
        self.lost_contacts += u64::from(baseline.is_some() && ball.is_none());
        self.recovered_contacts += u64::from(baseline.is_none() && ball.is_some());
        let Some(ball) = ball else {
            self.geometric_misses += 1;
            return;
        };
        if let Some(baseline) = baseline.filter(|_| in_window && !baseline_in_window) {
            self.entries_with_baseline_ev += 1;
            let delta = ball.launch_speed - baseline.launch_speed;
            self.mean_ev_change_for_entries_ms +=
                (delta - self.mean_ev_change_for_entries_ms) / self.entries_with_baseline_ev as f64;
        }
        self.recovered_power_window += u64::from(baseline.is_none() && in_window);
        self.recovered_home_runs +=
            u64::from(baseline.is_none() && ball.outbound_result.as_ref() == "HomeRun");
        self.contacts += 1;
        let n = self.contacts as f64;
        self.mean_ev_ms += (ball.launch_speed - self.mean_ev_ms) / n;
        self.mean_vla_deg += (ball.launch_angle - self.mean_vla_deg) / n;
        self.mean_spin_rpm += (ball.spin_rate - self.mean_spin_rpm) / n;
        let fair = ball.outbound_result.as_ref() != "Foul";
        self.fair_trajectories += u64::from(fair);
        self.fair_ev_at_least_40 += u64::from(fair && ball.launch_speed >= 40.0);
        self.power_window += u64::from(in_window);
        self.home_runs += u64::from(ball.outbound_result.as_ref() == "HomeRun");
    }
}

#[derive(Default, Serialize)]
pub(super) struct Sweep {
    all_swings: bool,
    source_swings: u64,
    source_misses: u64,
    comparable_baseline_misses: u64,
    positive_ev_contacts: u64,
    comparable_swings: u64,
    comparable_contacts: u64,
    by_shift_mm: BTreeMap<i32, Scenario>,
    exclusions: BTreeMap<String, u64>,
    exclusion_examples: Vec<serde_json::Value>,
}

impl Sweep {
    pub(super) fn new(all_swings: bool) -> Self {
        let shifts = if all_swings {
            ALL_SWING_SHIFTS_MM.as_slice()
        } else {
            SHIFTS_MM.as_slice()
        };
        Self {
            all_swings,
            by_shift_mm: shifts
                .iter()
                .map(|&shift| (shift, Scenario::default()))
                .collect(),
            ..Self::default()
        }
    }

    pub(super) fn observe(
        &mut self,
        c: &Contact,
        stadium: Option<&Stadium>,
        query: &mut Statement<'_>,
        missed: bool,
    ) -> Result<()> {
        self.source_swings += 1;
        self.source_misses += u64::from(missed);
        self.positive_ev_contacts += u64::from(c.ev > 0.0);
        let shifts = if self.all_swings {
            ALL_SWING_SHIFTS_MM.as_slice()
        } else {
            SHIFTS_MM.as_slice()
        };
        let (values, version, miss_inputs) = query.query_row((c.game_id, c.count_seq), |row| {
            Ok((
                [
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                ],
                row.get::<_, Option<i64>>(5)?,
                (
                    row.get::<_, Option<f64>>(9)?,
                    row.get::<_, Option<String>>(10)?,
                    row.get::<_, Option<f64>>(11)?,
                ),
            ))
        })?;
        let replayed = if missed {
            replay_miss(c, stadium, values, miss_inputs, shifts).map(|balls| (None, balls))
        } else {
            replay(c, stadium, values, version, shifts).map(|(base, balls)| (Some(base), balls))
        };
        match replayed {
            Ok((baseline, balls)) => {
                self.comparable_swings += 1;
                self.comparable_contacts += u64::from(!missed);
                self.comparable_baseline_misses += u64::from(missed);
                for (&shift, ball) in shifts.iter().zip(balls) {
                    self.by_shift_mm
                        .entry(shift)
                        .or_default()
                        .add(ball, baseline.as_ref());
                }
            }
            Err(error) => {
                let reason = error.to_string();
                *self.exclusions.entry(reason.clone()).or_default() += 1;
                if self.exclusion_examples.len() < 20 {
                    self.exclusion_examples.push(serde_json::json!({
                        "game_id":c.game_id,"count_seq":c.count_seq,"reason":reason
                    }));
                }
            }
        }
        Ok(())
    }
}

fn replay(
    c: &Contact,
    stadium: Option<&Stadium>,
    values: [Option<f64>; 8],
    version: Option<i64>,
    shifts: &[i32],
) -> Result<(BattedBall, Vec<Option<BattedBall>>)> {
    ensure!(version == Some(2), "unsupported_hla_model");
    let stadium = stadium.context("missing_stadium")?;
    let [
        x,
        depth,
        pitch_speed,
        power,
        handedness,
        approach,
        pull,
        timing,
    ] = values;
    let z = required(c.z)?;
    let inputs = Inputs {
        x: required(x)?,
        z,
        tilt: required(c.bat_angle)?,
        depth: required(depth)?,
        pitch_speed: required(pitch_speed)?,
        pitch_spin: required(c.pitch_spin)?,
        pitch_axis: required(c.pitch_axis)?,
        bat_speed: required(c.bat_speed)?,
        power: required(power)?,
        handedness: required(handedness)?,
        face_deg: (required(approach)? + required(pull)? + required(timing)?) * 0.85,
        attack: c.vla - calculate_vertical_contact_deflection(z),
    };
    ensure!(
        inputs.handedness.abs() == 1.0
            && inputs.pitch_speed > 0.0
            && inputs.bat_speed > 0.0
            && inputs.pitch_spin >= 0.0,
        "invalid_contact_inputs"
    );
    let (length, thickness) = project_contact_offsets(inputs.x, inputs.z, inputs.tilt);
    ensure!(
        (length.abs() - required(c.length)?).abs() < 1e-7
            && (thickness.abs() - required(c.thickness)?).abs() < 1e-7,
        "baseline_geometry_mismatch"
    );
    let baseline = inputs
        .flight(z, stadium)?
        .context("baseline_outside_contact_limits")?;
    let hla = wrap_hla_deg(
        (inputs.face_deg + contact_normal_hla_deg(thickness, inputs.tilt)) * inputs.handedness,
    );
    ensure!(
        (baseline.launch_speed - c.ev).abs() < 1e-7,
        "baseline_exit_speed_mismatch"
    );
    ensure!(
        wrap_hla_deg(hla - c.hla).abs() < 1e-7,
        "baseline_hla_mismatch"
    );
    verify_trajectory(c, &baseline)?;
    let balls = shifts
        .iter()
        .map(|&shift| {
            if shift == 0 {
                Ok(Some(baseline))
            } else {
                inputs.flight(z + f64::from(shift) / 1000.0, stadium)
            }
        })
        .collect::<Result<_>>()?;
    Ok((baseline, balls))
}

type SavedMissInputs = (Option<f64>, Option<String>, Option<f64>);

fn replay_miss(
    c: &Contact,
    stadium: Option<&Stadium>,
    values: [Option<f64>; 8],
    (bat_speed, side, timing_error): SavedMissInputs,
    shifts: &[i32],
) -> Result<Vec<Option<BattedBall>>> {
    let stadium = stadium.context("missing_stadium")?;
    let [x, depth, pitch_speed, power, ..] = values;
    let x = required(x)?;
    let z = required(c.z)?;
    let tilt = required(c.bat_angle)?;
    let (length, thickness) = project_contact_offsets(x, z, tilt);
    ensure!(
        (length.abs() - required(c.length)?).abs() < 1e-7
            && (thickness.abs() - required(c.thickness)?).abs() < 1e-7,
        "baseline_geometry_mismatch"
    );
    ensure!(
        c.ev == 0.0 && contact_geometry_is_miss(length, thickness),
        "baseline_miss_mismatch"
    );
    let side = side
        .as_deref()
        .context("missing_batting_side")?
        .parse::<RL>()
        .context("invalid_batting_side")?;
    let handedness = side.handedness_sign();
    let bat_speed = required(bat_speed)?;
    let pitch_speed = required(pitch_speed)?;
    let pitch_spin = required(c.pitch_spin)?;
    ensure!(
        bat_speed > 0.0 && pitch_speed > 0.0 && pitch_spin >= 0.0,
        "invalid_contact_inputs"
    );
    let game_id = u32::try_from(c.game_id).context("invalid_game_id")?;
    let pitch_id = u64::try_from(c.count_seq).context("invalid_count_seq")?;
    let mut rng = RealRng::from_seed(derived_seed(pitch_id, game_id));
    let (approach, pull, timing) =
        sample_face_angle_terms(&mut rng, handedness, bat_speed, required(timing_error)?);
    let inputs = Inputs {
        x,
        z,
        tilt,
        depth: required(depth)?,
        pitch_speed,
        pitch_spin,
        pitch_axis: required(c.pitch_axis)?,
        bat_speed,
        power: required(power)?,
        handedness,
        face_deg: ((approach + pull) + timing).to_degrees() * 0.85,
        attack: calculate_dynamic_attack_angle(required(c.base_attack)?, tilt),
    };
    shifts
        .iter()
        .map(|&shift| inputs.flight(z + f64::from(shift) / 1000.0, stadium))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saved_miss_can_recover_and_uses_repeatable_face_draws() {
        let z = -0.075;
        let (length, thickness) = project_contact_offsets(0.0, z, 0.0);
        let c = Contact {
            game_id: 13,
            count_seq: 5,
            batter_id: 42,
            stadium_id: 1,
            kind: "InField".into(),
            ev: 0.0,
            vla: 0.0,
            hla: 0.0,
            distance: 0.0,
            angle: 0.0,
            time: 0.0,
            bat_speed: Some(0.0),
            length: Some(length.abs()),
            thickness: Some(thickness.abs()),
            pitch_spin: Some(2400.0),
            pitch_axis: Some(0.0),
            z: Some(z),
            bat_angle: Some(0.0),
            base_attack: Some(6.0),
            batter_type: None,
        };
        let values = [
            Some(0.0),
            Some(0.0),
            Some(40.0),
            Some(0.0),
            None,
            None,
            None,
            None,
        ];
        let stadium = Stadium::default();
        let replay = || {
            replay_miss(
                &c,
                Some(&stadium),
                values,
                (Some(35.0), Some("Right".into()), Some(0.0)),
                &[0, 20],
            )
            .unwrap()
        };
        let first = replay();
        assert!(first[0].is_none());
        let recovered = first[1].expect("vertical shift brings thickness within the contact limit");
        assert_eq!(
            recovered.final_position.angle,
            replay()[1].unwrap().final_position.angle
        );
        let mut summary = Scenario::default();
        summary.add(Some(recovered), None);
        assert_eq!(summary.recovered_contacts, 1);
        assert_eq!(summary.lost_contacts, 0);
    }

    #[test]
    fn shifting_vertical_contact_changes_flight_and_can_create_a_miss() {
        let inputs = Inputs {
            x: 0.0,
            z: 0.0,
            tilt: 30.0,
            depth: 0.0,
            pitch_speed: 40.0,
            pitch_spin: 2400.0,
            pitch_axis: 0.0,
            bat_speed: 35.0,
            power: 0.0,
            handedness: 1.0,
            face_deg: 0.0,
            attack: 6.0,
        };
        let stadium = Stadium::default();
        let centered = inputs.flight(0.0, &stadium).unwrap().unwrap();
        let lifted = inputs.flight(0.03, &stadium).unwrap().unwrap();
        assert!(lifted.launch_angle > centered.launch_angle);
        assert!(lifted.launch_speed < centered.launch_speed);
        assert_ne!(lifted.spin_rate, centered.spin_rate);
        assert_ne!(lifted.final_position.angle, centered.final_position.angle);
        assert!(inputs.flight(0.09, &stadium).unwrap().is_none());
        let mut summary = Scenario::default();
        summary.add(None, Some(&lifted));
        assert_eq!(summary.geometric_misses, 1);
        assert_eq!(summary.contacts, 0);
    }
}
