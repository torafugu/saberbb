use super::outcome_stage;
use saberbb::domain::{
    shared::game_state::SwingReplay, strategy::batting_strategy::BattedBallIntent,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const VERTICAL_TOLERANCE_M: f64 = 0.0508;
const ALIGNMENT_BANDS: [&str; 3] = ["bat_above_ball", "lined_up", "bat_below_ball"];

fn alignment_index(contact_z_m: f64) -> usize {
    if contact_z_m < -VERTICAL_TOLERANCE_M {
        0
    } else if contact_z_m <= VERTICAL_TOLERANCE_M {
        1
    } else {
        2
    }
}

fn outcome(play: SwingReplay) -> &'static str {
    match outcome_stage(play) {
        "fair_hit" | "fair_non_hit" => "resolved_fair",
        stage => stage,
    }
}

#[derive(Default)]
struct AlignmentCounts {
    bands: [u64; 3],
    raw_vla_above_50_deg: u64,
}

impl AlignmentCounts {
    fn observe(&mut self, alignment: usize, play: SwingReplay) {
        self.bands[alignment] += 1;
        self.raw_vla_above_50_deg += u64::from(play.flight.is_some_and(|(_, _, vla)| vla > 50.0));
    }

    fn report(&self) -> Value {
        let swings: u64 = self.bands.iter().sum();
        let vertical_alignment: BTreeMap<_, _> = ALIGNMENT_BANDS
            .into_iter()
            .zip(self.bands)
            .map(|(name, count)| {
                let share = (swings > 0).then(|| count as f64 / swings as f64);
                (name, json!({"swings": count, "share_of_outcome": share}))
            })
            .collect();
        json!({
            "swings": swings,
            "vertical_alignment": vertical_alignment,
            "raw_vla_above_50_deg": self.raw_vla_above_50_deg,
        })
    }
}

#[derive(Default)]
struct PairedAlignment {
    off: BTreeMap<&'static str, AlignmentCounts>,
    on: BTreeMap<&'static str, AlignmentCounts>,
    entered_bat_below_ball: BTreeMap<&'static str, u64>,
    left_bat_below_ball: BTreeMap<&'static str, u64>,
}

impl PairedAlignment {
    fn observe(&mut self, off: SwingReplay, on: SwingReplay) {
        let before = alignment_index(off.contact_z_m);
        let after = alignment_index(on.contact_z_m);
        let off_outcome = outcome(off);
        let on_outcome = outcome(on);
        self.off
            .entry(off_outcome)
            .or_default()
            .observe(before, off);
        self.on.entry(on_outcome).or_default().observe(after, on);
        match (before == 2, after == 2) {
            (false, true) => *self.entered_bat_below_ball.entry(on_outcome).or_default() += 1,
            (true, false) => *self.left_bat_below_ball.entry(off_outcome).or_default() += 1,
            _ => {}
        }
    }

    fn report(&self) -> Value {
        let reports = |counts: &BTreeMap<&'static str, AlignmentCounts>| {
            counts
                .iter()
                .map(|(&result, counts)| (result, counts.report()))
                .collect::<BTreeMap<_, _>>()
        };
        json!({
            "without_timing_height": reports(&self.off),
            "with_timing_height": reports(&self.on),
            "entered_bat_below_ball_by_trial_outcome": self.entered_bat_below_ball,
            "left_bat_below_ball_by_baseline_outcome": self.left_bat_below_ball,
        })
    }
}

#[derive(Default)]
pub(super) struct TimingVerticalAlignment {
    all: PairedAlignment,
    contact_first: PairedAlignment,
    line_drive: PairedAlignment,
}

impl TimingVerticalAlignment {
    pub(super) fn observe(&mut self, intent: BattedBallIntent, off: SwingReplay, on: SwingReplay) {
        self.all.observe(off, on);
        match intent {
            BattedBallIntent::ContactFirst => self.contact_first.observe(off, on),
            BattedBallIntent::LineDrive => self.line_drive.observe(off, on),
        }
    }

    pub(super) fn report(&self) -> Value {
        json!({
            "definition": "Same live swings with their original aim and attack angle, with timing height off or on. Uses the signed world-vertical ball-minus-bat offset at the modeled contact evaluation, including swings that miss. The +/- 50.8 mm band matches the Statcast cutoff; its closest-approach measurement is different. Fouls follow the launch or ruling. Fair flights without a final result are listed separately from resolved fair balls.",
            "lined_up_max_abs_offset_mm": VERTICAL_TOLERANCE_M * 1000.0,
            "all": self.all.report(),
            "contact_first": self.contact_first.report(),
            "line_drive": self.line_drive.report(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saberbb::domain::{
        resolver::batting_resolver::SwingContactType,
        shared::{ball::OutboundResult, game::BattingResult},
    };

    #[test]
    fn counts_inclusive_alignment_boundaries_and_paired_outcome_changes() {
        assert_eq!(alignment_index(-VERTICAL_TOLERANCE_M), 1);
        assert_eq!(alignment_index(VERTICAL_TOLERANCE_M), 1);
        assert_eq!(alignment_index(-0.051), 0);
        assert_eq!(alignment_index(0.051), 2);

        let miss = SwingReplay {
            outcome: None,
            contact_type: SwingContactType::SwungAndMiss,
            flight: None,
            launch_hla_deg: None,
            contact_z_m: 0.0,
            launch_spin: None,
            exit_speed_retention: None,
            length_offset_m: 0.0,
            thickness_offset_m: 0.0,
            outfield_direct_out: false,
            double_fence: None,
        };
        let foul = SwingReplay {
            outcome: Some(BattingResult::Foul),
            contact_type: SwingContactType::MarginalContact,
            flight: Some((OutboundResult::Foul, 14.0, 52.0)),
            contact_z_m: 0.053,
            ..miss
        };
        let mut summary = TimingVerticalAlignment::default();
        summary.observe(BattedBallIntent::ContactFirst, miss, foul);
        summary.observe(BattedBallIntent::LineDrive, foul, miss);
        let report = summary.report();
        let all = &report["all"];
        assert_eq!(all["entered_bat_below_ball_by_trial_outcome"]["foul"], 1);
        assert_eq!(all["left_bat_below_ball_by_baseline_outcome"]["foul"], 1);
        assert_eq!(all["with_timing_height"]["foul"]["raw_vla_above_50_deg"], 1);
        assert_eq!(
            all["with_timing_height"]["whiff"]["vertical_alignment"]["lined_up"]["swings"],
            1
        );
    }
}
