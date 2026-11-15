//! Frozen-contact vertical displacement, using the live bat-frame geometry and flight.
use super::{Contact, required, verify_trajectory};
use anyhow::{Context, Result, ensure};
use rusqlite::Statement;
use saberbb::domain::{
    resolver::batting_resolver::{
        SwingContactResult, calculate_collision_spin_from_offsets,
        calculate_launch_speed_with_power, calculate_trajectory,
        calculate_vertical_contact_deflection, contact_geometry_is_miss, contact_normal_hla_deg,
        project_contact_offsets, wrap_hla_deg,
    },
    shared::{ball::BattedBall, stadium::Stadium},
};
use serde::Serialize;
use std::collections::BTreeMap;

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
    fair_trajectories: u64,
    fair_ev_at_least_40: u64,
    power_window: u64,
    entered_power_window: u64,
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
    fn add(&mut self, ball: Option<BattedBall>, baseline: &BattedBall) {
        let baseline_in_window = in_power_window(baseline);
        let in_window = ball.as_ref().is_some_and(in_power_window);
        self.entered_power_window += u64::from(in_window && !baseline_in_window);
        self.left_power_window += u64::from(!in_window && baseline_in_window);
        let Some(ball) = ball else {
            self.geometric_misses += 1;
            return;
        };
        if in_window && !baseline_in_window {
            let delta = ball.launch_speed - baseline.launch_speed;
            self.mean_ev_change_for_entries_ms +=
                (delta - self.mean_ev_change_for_entries_ms) / self.entered_power_window as f64;
        }
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
    positive_ev_contacts: u64,
    comparable_contacts: u64,
    by_shift_mm: BTreeMap<i32, Scenario>,
    exclusions: BTreeMap<String, u64>,
    exclusion_examples: Vec<serde_json::Value>,
}

impl Sweep {
    pub(super) fn observe(
        &mut self,
        c: &Contact,
        stadium: Option<&Stadium>,
        query: &mut Statement<'_>,
    ) -> Result<()> {
        self.positive_ev_contacts += 1;
        let (values, version) = query.query_row((c.game_id, c.count_seq), |row| {
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
            ))
        })?;
        match replay(c, stadium, values, version) {
            Ok((baseline, balls)) => {
                self.comparable_contacts += 1;
                for (shift, ball) in SHIFTS_MM.into_iter().zip(balls) {
                    self.by_shift_mm
                        .entry(shift)
                        .or_default()
                        .add(ball, &baseline);
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
    let balls = SHIFTS_MM
        .into_iter()
        .map(|shift| {
            if shift == 0 {
                Ok(Some(baseline))
            } else {
                inputs.flight(z + f64::from(shift) / 1000.0, stadium)
            }
        })
        .collect::<Result<_>>()?;
    Ok((baseline, balls))
}

#[cfg(test)]
mod tests {
    use super::*;

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
        summary.add(None, &lifted);
        assert_eq!(summary.geometric_misses, 1);
        assert_eq!(summary.contacts, 0);
    }
}
