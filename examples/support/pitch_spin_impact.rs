use super::outcome_stage;
use anyhow::{Context, Result};
use saberbb::domain::{
    resolver::batting_resolver::PITCH_SPIN_RETENTION,
    shared::{
        ball::{BattedBall, TrajectoryType},
        game::GameResult,
        game_state::{IntentAimSwing, SwingReplay},
        game_stats::PlayerGamePitching,
    },
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const RETENTION: [f64; 4] = [0.0, 0.10, PITCH_SPIN_RETENTION, 0.40];

fn is_popup(vla: f64, spin: (f64, f64), pitch_backspin: f64, retention: f64) -> bool {
    let (rate, axis) = spin;
    if retention == PITCH_SPIN_RETENTION {
        return BattedBall::trajectory_from_launch(vla, rate, axis) == TrajectoryType::PopUp;
    }
    let backspin =
        rate * axis.to_radians().cos() + (retention - PITCH_SPIN_RETENTION) * pitch_backspin;
    let axis = if backspin < 0.0 { 180.0 } else { 0.0 };
    BattedBall::trajectory_from_launch(vla, backspin.abs(), axis) == TrajectoryType::PopUp
}

#[derive(Default)]
struct Counts {
    swings: u64,
    launches: u64,
    fair_launches: u64,
    above_50: u64,
    positive_contact_z: u64,
    contact_z_at_least_40_mm: u64,
    break_sum_m: f64,
    effective_pitch_backspin_sum: f64,
    popups: [u64; RETENTION.len()],
    fair_popups: [u64; RETENTION.len()],
}

impl Counts {
    fn observe(
        &mut self,
        pitch: &PlayerGamePitching,
        break_z: f64,
        play: SwingReplay,
    ) -> Result<()> {
        self.swings += 1;
        self.break_sum_m += break_z;
        let raw_backspin = pitch.ball.spin_rate * pitch.ball.spin_angle.to_radians().cos();
        self.effective_pitch_backspin_sum += raw_backspin * pitch.ball.spin_efficiency;
        let Some((_, _, vla)) = play.flight else {
            return Ok(());
        };
        let spin = play
            .launch_spin
            .context("launch has no spin in pitch spin audit")?;
        let fair = outcome_stage(play) != "foul";
        self.launches += 1;
        self.fair_launches += u64::from(fair);
        self.above_50 += u64::from(vla > 50.0);
        self.positive_contact_z += u64::from(play.contact_z_m > 0.0);
        self.contact_z_at_least_40_mm += u64::from(play.contact_z_m >= 0.04);
        for (index, fraction) in RETENTION.into_iter().enumerate() {
            let popup = is_popup(vla, spin, raw_backspin, fraction);
            self.popups[index] += u64::from(popup);
            self.fair_popups[index] += u64::from(fair && popup);
        }
        Ok(())
    }

    fn report(&self) -> Value {
        let mean = |sum| (self.swings > 0).then(|| sum / self.swings as f64);
        let sensitivity: Vec<_> = RETENTION
            .iter()
            .enumerate()
            .map(|(i, fraction)| {
                json!({
                    "retained_pitch_spin_fraction": fraction,
                    "classified_popups": self.popups[i],
                    "classified_popups_among_baseline_fair_launches": self.fair_popups[i],
                })
            })
            .collect();
        json!({
            "swings": self.swings,
            "launches": self.launches,
            "baseline_fair_launches": self.fair_launches,
            "raw_vla_above_50_deg": self.above_50,
            "launches_with_positive_ball_minus_bat_z": self.positive_contact_z,
            "launches_with_ball_minus_bat_z_at_least_40_mm": self.contact_z_at_least_40_mm,
            "mean_vertical_prediction_break_mm": mean(self.break_sum_m * 1000.0),
            "mean_effective_pitch_backspin_rpm": mean(self.effective_pitch_backspin_sum),
            "retention_sensitivity": sensitivity,
        })
    }
}

#[derive(Default)]
pub(super) struct PitchSpinImpact {
    all: Counts,
    pitch_types: BTreeMap<String, Counts>,
    vertical_break: BTreeMap<&'static str, Counts>,
}

impl PitchSpinImpact {
    pub(super) fn observe_game(
        &mut self,
        result: &GameResult,
        swings: &[IntentAimSwing],
    ) -> Result<()> {
        let pitches: BTreeMap<_, _> = result
            .player_pitchings
            .iter()
            .map(|p| (p.count_seq, p))
            .collect();
        for swing in swings {
            let pitch = pitches
                .get(&swing.count_seq)
                .context("swing has no matching recorded pitch")?;
            let break_z =
                pitch.pitch_displacement.vertical_offset_m - pitch.location_bias.spatial_bias_y;
            let band = if break_z > 0.005 {
                "above_prediction_by_more_than_5_mm"
            } else if break_z < -0.005 {
                "below_prediction_by_more_than_5_mm"
            } else {
                "within_5_mm_of_prediction"
            };
            let play = swing
                .plays
                .first()
                .context("swing has no live replay arm")?
                .batting;
            self.all.observe(pitch, break_z, play)?;
            self.pitch_types
                .entry(pitch.ball.pitch_type.as_ref().to_owned())
                .or_default()
                .observe(pitch, break_z, play)?;
            self.vertical_break
                .entry(band)
                .or_default()
                .observe(pitch, break_z, play)?;
        }
        Ok(())
    }

    pub(super) fn report(&self) -> Value {
        let pitch_types: BTreeMap<_, _> = self
            .pitch_types
            .iter()
            .map(|(key, counts)| (key, counts.report()))
            .collect();
        let break_bands: BTreeMap<_, _> = self
            .vertical_break
            .iter()
            .map(|(key, counts)| (key, counts.report()))
            .collect();
        json!({
            "definition": "Live replay arm only. Vertical prediction break is the pre-adaptation vertical pitch displacement minus the location bias; it measures the modeled break relative to the expected pitch. Positive contact Z means ball center above the bat reference. The spin sensitivity holds raw launch angle, existing collision spin and baseline fair/foul outcomes fixed, and changes only the retained pitch vector for the PopUp classifier. It does not recalculate flight or fair/foul rulings.",
            "all": self.all.report(),
            "by_pitch_type": pitch_types,
            "by_vertical_prediction_break": break_bands,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spin_transfer_can_flip_popup_classification_in_either_direction() {
        assert!(is_popup(49.8, (480.0, 0.0), 2400.0, 0.20));
        assert!(!is_popup(49.8, (480.0, 0.0), 2400.0, 0.0));
        assert!(!is_popup(50.2, (480.0, 180.0), -2400.0, 0.20));
        assert!(is_popup(50.2, (480.0, 180.0), -2400.0, 0.0));
    }
}
