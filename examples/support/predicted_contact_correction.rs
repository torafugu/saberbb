use super::outcome_stage;
use saberbb::domain::{
    resolver::batting_resolver::{CONTACT_LENGTH_LIMIT_M, CONTACT_THICKNESS_LIMIT_M},
    shared::game_state::{IntentAimSwing, SwingReplay},
    strategy::batting_strategy::BattedBallIntent,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

// Ignore floating-point rounding at unchanged offsets and band boundaries.
const TOLERANCE_M: f64 = 1e-12;

fn correction_band(shift_m: f64) -> &'static str {
    match shift_m.abs() {
        magnitude if magnitude <= 0.005 + TOLERANCE_M => "up_to_5_mm",
        magnitude if magnitude <= 0.010 + TOLERANCE_M => "over_5_to_10_mm",
        magnitude if magnitude <= 0.020 + TOLERANCE_M => "over_10_to_20_mm",
        _ => "over_20_mm",
    }
}

fn clearance_band(clearance_m: f64) -> &'static str {
    match clearance_m {
        clearance if clearance < -0.010 - TOLERANCE_M => "outside_over_10_mm",
        clearance if clearance < -TOLERANCE_M => "outside_up_to_10_mm",
        clearance if clearance <= 0.010 + TOLERANCE_M => "inside_up_to_10_mm",
        _ => "inside_over_10_mm",
    }
}

#[derive(Default)]
struct ThicknessChanges {
    swings: u64,
    toward_center: u64,
    away_from_center: u64,
    unchanged: u64,
    worsened_after_crossing_center: u64,
    contact_to_whiff: u64,
    whiff_to_contact: u64,
}

impl ThicknessChanges {
    fn observe(&mut self, from_m: f64, to_m: f64, before_contact: bool, after_contact: bool) {
        self.swings += 1;
        let change = to_m.abs() - from_m.abs();
        if change < -TOLERANCE_M {
            self.toward_center += 1;
        } else if change > TOLERANCE_M {
            self.away_from_center += 1;
            if (from_m < -TOLERANCE_M && to_m > TOLERANCE_M)
                || (from_m > TOLERANCE_M && to_m < -TOLERANCE_M)
            {
                self.worsened_after_crossing_center += 1;
            }
        } else {
            self.unchanged += 1;
        }
        self.contact_to_whiff += u64::from(before_contact && !after_contact);
        self.whiff_to_contact += u64::from(!before_contact && after_contact);
    }

    fn report(&self) -> Value {
        json!({
            "swings": self.swings,
            "thickness_error_decreased": self.toward_center,
            "thickness_error_increased": self.away_from_center,
            "thickness_error_unchanged": self.unchanged,
            "thickness_error_increased_after_crossing_center": self.worsened_after_crossing_center,
            "contact_to_whiff": self.contact_to_whiff,
            "whiff_to_contact": self.whiff_to_contact,
        })
    }
}

#[derive(Default)]
struct CorrectionCounts {
    swings: u64,
    missing_forecast: u64,
    corrected: u64,
    abs_correction_sum_m: f64,
    whiff_to_contact: u64,
    contact_to_whiff: u64,
    near_thickness_whiffs: u64,
    near_thickness_whiffs_to_contact: u64,
    new_contacts_by_type: BTreeMap<String, u64>,
    new_contacts_by_outcome: BTreeMap<&'static str, u64>,
    high_launches_before: BTreeMap<&'static str, u64>,
    high_launches_after: BTreeMap<&'static str, u64>,
    missing_signed_thickness: u64,
    thickness_by_shift: BTreeMap<&'static str, ThicknessChanges>,
    thickness_by_clearance_and_shift:
        BTreeMap<&'static str, BTreeMap<&'static str, ThicknessChanges>>,
}

impl CorrectionCounts {
    fn observe(&mut self, before: SwingReplay, after: SwingReplay, correction: Option<f64>) {
        self.swings += 1;
        if let Some(shift) = correction {
            self.corrected += u64::from(shift != 0.0);
            self.abs_correction_sum_m += shift.abs();
            if shift != 0.0 {
                self.observe_thickness_change(before, after, shift);
            }
        } else {
            self.missing_forecast += 1;
        }

        let new_contact = before.flight.is_none() && after.flight.is_some();
        self.whiff_to_contact += u64::from(new_contact);
        self.contact_to_whiff += u64::from(before.flight.is_some() && after.flight.is_none());
        if before.flight.is_none()
            && before.length_offset_m <= CONTACT_LENGTH_LIMIT_M
            && before.thickness_offset_m > CONTACT_THICKNESS_LIMIT_M
            && before.thickness_offset_m <= CONTACT_THICKNESS_LIMIT_M + 0.010
        {
            self.near_thickness_whiffs += 1;
            self.near_thickness_whiffs_to_contact += u64::from(new_contact);
        }
        if new_contact {
            *self
                .new_contacts_by_type
                .entry(after.contact_type.as_ref().to_owned())
                .or_default() += 1;
            *self
                .new_contacts_by_outcome
                .entry(outcome_stage(after))
                .or_default() += 1;
        }
        if before
            .flight
            .is_some_and(|(_, _, vla)| vla > super::high_vla_reachability::HIGH_VLA_DEG)
        {
            *self
                .high_launches_before
                .entry(outcome_stage(before))
                .or_default() += 1;
        }
        if after
            .flight
            .is_some_and(|(_, _, vla)| vla > super::high_vla_reachability::HIGH_VLA_DEG)
        {
            *self
                .high_launches_after
                .entry(outcome_stage(after))
                .or_default() += 1;
        }
    }

    fn observe_thickness_change(&mut self, before: SwingReplay, after: SwingReplay, shift: f64) {
        let (Some(from), Some(to)) = (
            before.signed_thickness_offset_m,
            after.signed_thickness_offset_m,
        ) else {
            self.missing_signed_thickness += 1;
            return;
        };
        let clearance = (CONTACT_THICKNESS_LIMIT_M - before.thickness_offset_m)
            .min(CONTACT_LENGTH_LIMIT_M - before.length_offset_m);
        let shift_band = correction_band(shift);
        let before_contact = before.flight.is_some();
        let after_contact = after.flight.is_some();
        self.thickness_by_shift
            .entry(shift_band)
            .or_default()
            .observe(from, to, before_contact, after_contact);
        self.thickness_by_clearance_and_shift
            .entry(clearance_band(clearance))
            .or_default()
            .entry(shift_band)
            .or_default()
            .observe(from, to, before_contact, after_contact);
    }

    fn report(&self) -> Value {
        let by_shift = |counts: &BTreeMap<_, ThicknessChanges>| {
            counts
                .iter()
                .map(|(&band, stats)| (band, stats.report()))
                .collect::<BTreeMap<_, _>>()
        };
        let by_clearance_and_shift: BTreeMap<_, _> = self
            .thickness_by_clearance_and_shift
            .iter()
            .map(|(&band, counts)| (band, by_shift(counts)))
            .collect();
        json!({
            "swings": self.swings,
            "missing_forecast": self.missing_forecast,
            "corrected_swings": self.corrected,
            "mean_abs_correction_mm_on_corrected_swings": (self.corrected > 0)
                .then(|| 1000.0 * self.abs_correction_sum_m / self.corrected as f64),
            "whiff_to_contact": self.whiff_to_contact,
            "contact_to_whiff": self.contact_to_whiff,
            "thickness_only_whiffs_within_10_mm": self.near_thickness_whiffs,
            "thickness_only_whiffs_within_10_mm_to_contact": self.near_thickness_whiffs_to_contact,
            "new_contacts_by_type": self.new_contacts_by_type,
            "new_contacts_by_outcome": self.new_contacts_by_outcome,
            "vla_above_50_deg_before_by_outcome": self.high_launches_before,
            "vla_above_50_deg_after_by_outcome": self.high_launches_after,
            "corrected_thickness_direction": {
                "missing_signed_thickness": self.missing_signed_thickness,
                "by_absolute_z_correction": by_shift(&self.thickness_by_shift),
                "by_baseline_contact_clearance_and_absolute_z_correction": by_clearance_and_shift,
            },
        })
    }
}

#[derive(Default)]
pub(super) struct PredictedContactCorrectionCounts {
    all: CorrectionCounts,
    contact_first: CorrectionCounts,
    line_drive: CorrectionCounts,
}

impl PredictedContactCorrectionCounts {
    pub(super) fn observe(&mut self, swing: &IntentAimSwing) {
        let before = swing.plays[1].batting;
        let after = swing.plays[2].batting;
        let shift = swing.predicted_contact_correction_m;
        self.all.observe(before, after, shift);
        match swing.intent {
            BattedBallIntent::ContactFirst => self.contact_first.observe(before, after, shift),
            BattedBallIntent::LineDrive => self.line_drive.observe(before, after, shift),
        }
    }

    pub(super) fn report(&self) -> Value {
        json!({
            "all": self.all.report(),
            "contact_first": self.contact_first.report(),
            "line_drive": self.line_drive.report(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinguishes_overshoot_from_wrong_way_and_classifies_boundaries() {
        let mut changes = ThicknessChanges::default();
        changes.observe(0.004, -0.008, true, true);
        changes.observe(-0.060, -0.080, true, false);
        changes.observe(0.074, 0.066, false, true);
        changes.observe(-0.004, 0.004, true, true);

        assert_eq!(changes.toward_center, 1);
        assert_eq!(changes.away_from_center, 2);
        assert_eq!(changes.unchanged, 1);
        assert_eq!(changes.worsened_after_crossing_center, 1);
        assert_eq!(changes.contact_to_whiff, 1);
        assert_eq!(changes.whiff_to_contact, 1);
        assert_eq!(clearance_band(0.070 - 0.080), "outside_up_to_10_mm");
        assert_eq!(clearance_band(0.0), "inside_up_to_10_mm");
        assert_eq!(correction_band(-0.020), "over_10_to_20_mm");
        assert_eq!(correction_band(0.030), "over_20_mm");
    }
}
