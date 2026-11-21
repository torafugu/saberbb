use super::outcome_stage;
use saberbb::domain::{
    resolver::batting_resolver::{CONTACT_LENGTH_LIMIT_M, CONTACT_THICKNESS_LIMIT_M},
    shared::game_state::{SwingLengthOffsetTerms, SwingReplay},
    strategy::batting_strategy::BattedBallIntent,
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

const EXCESS_BANDS: [&str; 5] = [
    "within_limit",
    "over_0_to_5",
    "over_5_to_10",
    "over_10_to_20",
    "over_20",
];

fn excess_index(offset: f64, limit: f64) -> usize {
    let distance = offset.abs();
    if distance <= limit {
        0
    } else if distance <= limit + 0.005 {
        1
    } else if distance <= limit + 0.010 {
        2
    } else if distance <= limit + 0.020 {
        3
    } else {
        4
    }
}

#[derive(Default)]
struct LengthComponent {
    sum_m: f64,
    sum_abs_m: f64,
    largest_abs: u64,
    extends_length_error: u64,
    within_limit_without_term: u64,
}

#[derive(Default)]
struct LengthComponents {
    swings: u64,
    components: [LengthComponent; 4],
}

impl LengthComponents {
    fn observe(&mut self, terms: SwingLengthOffsetTerms) {
        self.swings += 1;
        let values = [
            terms.horizontal_pitch_m,
            terms.horizontal_execution_m,
            terms.horizontal_timing_m,
            terms.vertical_projection_m,
        ];
        let largest_abs = values.iter().map(|value| value.abs()).fold(0.0, f64::max);
        for (component, value) in self.components.iter_mut().zip(values) {
            let remaining = (terms.signed_total_m - value).abs();
            component.sum_m += value;
            component.sum_abs_m += value.abs();
            component.largest_abs += u64::from(value.abs() == largest_abs);
            component.extends_length_error += u64::from(remaining < terms.signed_total_m.abs());
            component.within_limit_without_term += u64::from(remaining <= CONTACT_LENGTH_LIMIT_M);
        }
    }

    fn report(&self, length_misses: u64) -> Value {
        let mean_mm = |sum: f64| (self.swings > 0).then(|| sum * 1000.0 / self.swings as f64);
        let by_component: BTreeMap<_, _> = [
            "horizontal_pitch",
            "horizontal_execution",
            "horizontal_timing",
            "vertical_projection",
        ]
        .into_iter()
        .zip(&self.components)
        .map(|(name, component)| {
            (
                name,
                json!({
                    "mean_signed_mm": mean_mm(component.sum_m),
                    "mean_absolute_mm": mean_mm(component.sum_abs_m),
                    "largest_absolute_component": component.largest_abs,
                    "reduces_absolute_length_when_removed": component.extends_length_error,
                    "length_within_limit_when_removed": component.within_limit_without_term,
                }),
            )
        })
        .collect();
        json!({
            "swings": self.swings,
            "missing_terms": length_misses - self.swings,
            "by_component": by_component,
        })
    }
}

#[derive(Default)]
struct WhiffGeometry {
    // Rows measure length excess. Columns measure thickness excess.
    excess: [[u64; EXCESS_BANDS.len()]; EXCESS_BANDS.len()],
    length_components: LengthComponents,
}

impl WhiffGeometry {
    fn observe(&mut self, play: SwingReplay) {
        let length = excess_index(play.length_offset_m, CONTACT_LENGTH_LIMIT_M);
        let thickness = excess_index(play.thickness_offset_m, CONTACT_THICKNESS_LIMIT_M);
        self.excess[length][thickness] += 1;
        if length > 0
            && let Some(terms) = play.length_offset_terms
        {
            self.length_components.observe(terms);
        }
    }

    fn report(&self) -> Value {
        let within_limits = self.excess[0][0];
        let thickness_only: u64 = self.excess[0][1..].iter().sum();
        let length_only: u64 = self.excess[1..].iter().map(|row| row[0]).sum();
        let both_axes: u64 = self.excess[1..].iter().flat_map(|row| &row[1..]).sum();
        let joint: BTreeMap<_, _> = EXCESS_BANDS
            .into_iter()
            .zip(&self.excess)
            .filter_map(|(length, row)| {
                let by_thickness: BTreeMap<_, _> = EXCESS_BANDS
                    .into_iter()
                    .zip(row.iter().copied())
                    .filter(|&(_, count)| count != 0)
                    .collect();
                (!by_thickness.is_empty()).then_some((length, by_thickness))
            })
            .collect();
        json!({
            "swings": within_limits + thickness_only + length_only + both_axes,
            "misses_by_axis": {
                "thickness_only": thickness_only,
                "length_only": length_only,
                "both_axes": both_axes,
                "within_both_limits": within_limits,
            },
            "thickness_only_within_5_mm": self.excess[0][1],
            "thickness_only_within_10_mm": self.excess[0][1] + self.excess[0][2],
            "excess_by_length_and_thickness_mm": joint,
            "length_components": self.length_components.report(length_only + both_axes),
        })
    }
}

#[derive(Default)]
struct PairedAlignment {
    off: BTreeMap<&'static str, AlignmentCounts>,
    on: BTreeMap<&'static str, AlignmentCounts>,
    entered_bat_below_ball: BTreeMap<&'static str, u64>,
    left_bat_below_ball: BTreeMap<&'static str, u64>,
    whiff_geometry_off: WhiffGeometry,
    whiff_geometry_on: WhiffGeometry,
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
        if before == 2 && off_outcome == "whiff" {
            self.whiff_geometry_off.observe(off);
        }
        if after == 2 && on_outcome == "whiff" {
            self.whiff_geometry_on.observe(on);
        }
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
            "below_ball_whiff_geometry": {
                "without_timing_height": self.whiff_geometry_off.report(),
                "with_timing_height": self.whiff_geometry_on.report(),
            },
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
            "whiff_geometry_definition": "Only whiffs in the bat_below_ball band. Compare the magnitude of each bat-local offset with the corresponding live geometric limit. Excess bins include their upper edge. Rows in the joint table are length bands; columns are thickness bands. Empty cells are omitted.",
            "length_components_definition": "Only below-ball whiffs exceeding the length limit. The signed length is the sum of projected adapted horizontal pitch offset (including any location bias), horizontal swing execution offset, horizontal timing position offset, and world-vertical offset (including any active timing height). Each removal holds bat angle and the other terms fixed and checks the length axis alone. Counts can overlap across components, including ties for the largest magnitude.",
            "whiff_geometry_limits_mm": {
                "length": CONTACT_LENGTH_LIMIT_M * 1000.0,
                "thickness": CONTACT_THICKNESS_LIMIT_M * 1000.0,
            },
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
            length_offset_terms: None,
            signed_thickness_offset_m: None,
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
        let below = SwingReplay {
            contact_z_m: 0.055,
            ..miss
        };
        let thickness_only = SwingReplay {
            length_offset_m: CONTACT_LENGTH_LIMIT_M,
            thickness_offset_m: CONTACT_THICKNESS_LIMIT_M + 0.005,
            ..below
        };
        let length_only = SwingReplay {
            length_offset_m: CONTACT_LENGTH_LIMIT_M + 0.012,
            length_offset_terms: Some(SwingLengthOffsetTerms {
                signed_total_m: 0.172,
                horizontal_pitch_m: -0.025,
                horizontal_execution_m: 0.0,
                horizontal_timing_m: 0.175,
                vertical_projection_m: 0.022,
            }),
            thickness_offset_m: CONTACT_THICKNESS_LIMIT_M,
            ..below
        };
        let both_axes = SwingReplay {
            length_offset_m: CONTACT_LENGTH_LIMIT_M + 0.023,
            thickness_offset_m: CONTACT_THICKNESS_LIMIT_M + 0.007,
            ..below
        };
        let mut geometry = TimingVerticalAlignment::default();
        geometry.observe(BattedBallIntent::ContactFirst, thickness_only, length_only);
        geometry.observe(BattedBallIntent::ContactFirst, foul, both_axes);
        geometry.observe(BattedBallIntent::LineDrive, miss, miss);
        let geometry = geometry.report();
        let metrics = &geometry["all"]["below_ball_whiff_geometry"];
        assert_eq!(metrics["without_timing_height"]["swings"], 1);
        assert_eq!(
            metrics["without_timing_height"]["thickness_only_within_5_mm"],
            1
        );
        assert_eq!(metrics["with_timing_height"]["swings"], 2);
        assert_eq!(
            metrics["with_timing_height"]["misses_by_axis"]["length_only"],
            1
        );
        assert_eq!(
            metrics["with_timing_height"]["misses_by_axis"]["both_axes"],
            1
        );
        assert_eq!(
            metrics["with_timing_height"]["excess_by_length_and_thickness_mm"]["over_20"]["over_5_to_10"],
            1
        );

        let components = &metrics["with_timing_height"]["length_components"];
        assert_eq!(components["swings"], 1);
        assert_eq!(components["missing_terms"], 1);
        let terms = &components["by_component"];
        assert_eq!(terms["horizontal_timing"]["largest_absolute_component"], 1);
        assert_eq!(
            terms["horizontal_timing"]["length_within_limit_when_removed"],
            1
        );
        assert_eq!(
            terms["vertical_projection"]["length_within_limit_when_removed"],
            1
        );
        assert_eq!(
            terms["horizontal_pitch"]["reduces_absolute_length_when_removed"],
            0
        );
        assert_eq!(
            terms["horizontal_pitch"]["length_within_limit_when_removed"],
            0
        );
        assert_eq!(
            metrics["without_timing_height"]["length_components"]["swings"],
            0
        );

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
