use super::super::outcome_stage;
use saberbb::domain::{
    resolver::batting_resolver::SwingContactType, shared::game_state::SwingReplay,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) const HIGH_VLA_DEG: f64 = 50.0;
const CONTACT_TYPES: [&str; 3] = ["SolidContact", "WeakContact", "MarginalContact"];
const VLA_TRANSITIONS: [&str; 5] = [
    "stayed_at_or_below_threshold",
    "entered_above_threshold",
    "left_above_threshold",
    "stayed_above_threshold",
    "missing_launch_angle",
];

fn contact_index(contact: SwingContactType) -> Option<usize> {
    match contact {
        SwingContactType::SolidContact => Some(0),
        SwingContactType::WeakContact => Some(1),
        SwingContactType::MarginalContact => Some(2),
        SwingContactType::SwungAndMiss | SwingContactType::Take => None,
    }
}

fn high_vla(play: SwingReplay) -> Option<bool> {
    let (_, _, vla) = play.flight?;
    vla.is_finite().then_some(vla > HIGH_VLA_DEG)
}

#[derive(Default)]
struct ContactPairs {
    swings: u64,
    transitions: [[u64; CONTACT_TYPES.len()]; CONTACT_TYPES.len()],
    thickness_pairs: u64,
    sum_thickness_before_m: f64,
    sum_thickness_after_m: f64,
    sum_abs_thickness_change_m: f64,
    fair_high_before: u64,
    fair_high_after: u64,
}

impl ContactPairs {
    fn observe(&mut self, contacts: (usize, usize), before: SwingReplay, after: SwingReplay) {
        self.swings += 1;
        self.transitions[contacts.0][contacts.1] += 1;
        if let (Some(from), Some(to)) = (
            before.signed_thickness_offset_m,
            after.signed_thickness_offset_m,
        ) {
            self.thickness_pairs += 1;
            self.sum_thickness_before_m += from;
            self.sum_thickness_after_m += to;
            self.sum_abs_thickness_change_m += (to - from).abs();
        }
        let fair_high = |play| {
            high_vla(play) == Some(true)
                && matches!(
                    outcome_stage(play),
                    "fair_hit" | "fair_non_hit" | "fair_unresolved"
                )
        };
        self.fair_high_before += u64::from(fair_high(before));
        self.fair_high_after += u64::from(fair_high(after));
    }

    fn report(&self) -> Value {
        let transitions: BTreeMap<_, _> = CONTACT_TYPES
            .into_iter()
            .zip(self.transitions)
            .map(|(from, row)| {
                let destinations: BTreeMap<_, _> = CONTACT_TYPES.into_iter().zip(row).collect();
                (from, destinations)
            })
            .collect();
        let mean_mm = |sum: f64| {
            (self.thickness_pairs > 0).then(|| sum * 1000.0 / self.thickness_pairs as f64)
        };
        json!({
            "swings": self.swings,
            "contact_type_transitions": transitions,
            "signed_thickness": {
                "paired_offsets": self.thickness_pairs,
                "mean_from_mm": mean_mm(self.sum_thickness_before_m),
                "mean_to_mm": mean_mm(self.sum_thickness_after_m),
                "mean_change_mm": mean_mm(self.sum_thickness_after_m - self.sum_thickness_before_m),
                "mean_absolute_change_mm": mean_mm(self.sum_abs_thickness_change_m),
            },
            "fair_above_threshold_from": self.fair_high_before,
            "fair_above_threshold_to": self.fair_high_after,
        })
    }
}

#[derive(Default)]
pub(super) struct RetainedContact {
    all: ContactPairs,
    by_vla_transition: [ContactPairs; VLA_TRANSITIONS.len()],
}

impl RetainedContact {
    pub(super) fn observe(&mut self, before: SwingReplay, after: SwingReplay) {
        let (Some(from), Some(to)) = (
            contact_index(before.contact_type),
            contact_index(after.contact_type),
        ) else {
            return;
        };
        let transition = match (high_vla(before), high_vla(after)) {
            (Some(false), Some(false)) => 0,
            (Some(false), Some(true)) => 1,
            (Some(true), Some(false)) => 2,
            (Some(true), Some(true)) => 3,
            _ => 4,
        };
        self.all.observe((from, to), before, after);
        self.by_vla_transition[transition].observe((from, to), before, after);
    }

    pub(super) fn report(&self) -> Value {
        let by_vla_transition: BTreeMap<_, _> = VLA_TRANSITIONS
            .into_iter()
            .zip(&self.by_vla_transition)
            .map(|(name, counts)| (name, counts.report()))
            .collect();
        json!({ "all": self.all.report(), "by_vla_transition": by_vla_transition })
    }
}
