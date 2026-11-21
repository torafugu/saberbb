use super::outcome_stage;
use saberbb::domain::{
    resolver::batting_resolver::{CONTACT_LENGTH_LIMIT_M, CONTACT_THICKNESS_LIMIT_M},
    shared::game_state::{IntentAimSwing, SwingReplay},
    strategy::batting_strategy::BattedBallIntent,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

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
}

impl CorrectionCounts {
    fn observe(&mut self, before: SwingReplay, after: SwingReplay, correction: Option<f64>) {
        self.swings += 1;
        if let Some(shift) = correction {
            self.corrected += u64::from(shift != 0.0);
            self.abs_correction_sum_m += shift.abs();
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

    fn report(&self) -> Value {
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
