use super::outcome_stage;
use saberbb::domain::{
    resolver::batting_resolver::SwingContactType, shared::game_state::SwingReplay,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const COHORTS: [&str; 4] = [
    "both_miss",
    "line_drive_only_contact",
    "contact_first_only_contact",
    "both_contact",
];

const LENGTH_BANDS_M: [(f64, &str); 4] = [
    (0.02, "0-2"),
    (0.05, ">2-5"),
    (0.10, ">5-10"),
    (0.16, ">10-16"),
];
const THICKNESS_BANDS_M: [(f64, &str); 4] = [
    (0.02, "0-2"),
    (0.025, ">2-2.5"),
    (0.055, ">2.5-5.5"),
    (0.07, ">5.5-7"),
];

fn offset_band(
    offset_m: f64,
    bands: &[(f64, &'static str)],
    overflow: &'static str,
) -> &'static str {
    bands
        .iter()
        .find(|(upper, _)| offset_m <= *upper)
        .map_or(overflow, |(_, label)| *label)
}

fn fraction(numerator: f64, denominator: u64) -> Option<f64> {
    (denominator > 0).then(|| numerator / denominator as f64)
}

fn has_contact(play: SwingReplay) -> bool {
    play.contact_type != SwingContactType::SwungAndMiss
}

#[derive(Default)]
struct ApproachQuality {
    contacts: u64,
    fouls: u64,
    length_sum_m: f64,
    thickness_sum_m: f64,
    length_bands: BTreeMap<&'static str, u64>,
    thickness_bands: BTreeMap<&'static str, u64>,
    retention_launches: u64,
    retention_sum: f64,
    retention_at_least_80: u64,
    retention_at_least_95: u64,
}

impl ApproachQuality {
    fn observe(&mut self, play: SwingReplay) {
        self.contacts += u64::from(has_contact(play));
        self.fouls += u64::from(outcome_stage(play) == "foul");
        self.length_sum_m += play.length_offset_m;
        self.thickness_sum_m += play.thickness_offset_m;
        let length_band = offset_band(play.length_offset_m, &LENGTH_BANDS_M, ">16");
        let thickness_band = offset_band(play.thickness_offset_m, &THICKNESS_BANDS_M, ">7");
        *self.length_bands.entry(length_band).or_default() += 1;
        *self.thickness_bands.entry(thickness_band).or_default() += 1;
        if let Some(retention) = play.exit_speed_retention {
            self.retention_launches += 1;
            self.retention_sum += retention;
            self.retention_at_least_80 += u64::from(retention >= 0.80);
            self.retention_at_least_95 += u64::from(retention >= 0.95);
        }
    }

    fn report(&self, pairs: u64) -> Value {
        json!({
            "contacts": self.contacts,
            "fouls": self.fouls,
            "fouls_per_contact": fraction(self.fouls as f64, self.contacts),
            "mean_abs_length_offset_cm": fraction(self.length_sum_m * 100.0, pairs),
            "mean_abs_thickness_offset_cm": fraction(self.thickness_sum_m * 100.0, pairs),
            "abs_length_offset_counts_by_cm": self.length_bands,
            "abs_thickness_offset_counts_by_cm": self.thickness_bands,
            "retention_launches": self.retention_launches,
            "mean_exit_speed_retention": fraction(self.retention_sum, self.retention_launches),
            "retention_at_least_80_pct": self.retention_at_least_80,
            "retention_at_least_95_pct": self.retention_at_least_95,
        })
    }
}

#[derive(Default)]
struct ContactPairCohort {
    pairs: u64,
    contact_first: ApproachQuality,
    line_drive: ApproachQuality,
    contact_types: BTreeMap<String, BTreeMap<String, u64>>,
}

impl ContactPairCohort {
    fn report(&self) -> Value {
        json!({
            "pairs": self.pairs,
            "contact_first": self.contact_first.report(self.pairs),
            "line_drive": self.line_drive.report(self.pairs),
            "contact_first_to_line_drive_adjusted_contact_types": self.contact_types,
        })
    }
}

#[derive(Default)]
pub(super) struct ContactApproachQuality {
    cohorts: [ContactPairCohort; 4],
}

impl ContactApproachQuality {
    pub(super) fn observe(&mut self, contact_first: SwingReplay, line_drive: SwingReplay) {
        let index =
            usize::from(has_contact(contact_first)) * 2 + usize::from(has_contact(line_drive));
        let cohort = &mut self.cohorts[index];
        cohort.pairs += 1;
        cohort.contact_first.observe(contact_first);
        cohort.line_drive.observe(line_drive);
        *cohort
            .contact_types
            .entry(contact_first.contact_type.as_ref().to_owned())
            .or_default()
            .entry(line_drive.contact_type.as_ref().to_owned())
            .or_default() += 1;
    }

    pub(super) fn report(&self) -> Value {
        let cohorts: BTreeMap<_, _> = COHORTS
            .into_iter()
            .zip(self.cohorts.iter().map(ContactPairCohort::report))
            .collect();
        json!(cohorts)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saberbb::domain::shared::{ball::OutboundResult, game::BattingResult};

    #[test]
    fn separates_rescued_contacts_from_shared_contacts_and_keeps_fouls() {
        let miss = SwingReplay {
            outcome: None,
            contact_type: SwingContactType::SwungAndMiss,
            flight: None,
            launch_hla_deg: None,
            exit_speed_retention: None,
            length_offset_m: 0.02,
            thickness_offset_m: 0.071,
            outfield_direct_out: false,
            double_fence: None,
        };
        let foul = SwingReplay {
            outcome: Some(BattingResult::Foul),
            contact_type: SwingContactType::MarginalContact,
            flight: Some((OutboundResult::Foul, 12.0, 25.0)),
            exit_speed_retention: Some(0.15),
            thickness_offset_m: 0.06,
            ..miss
        };
        let solid = SwingReplay {
            outcome: Some(BattingResult::Single),
            contact_type: SwingContactType::SolidContact,
            flight: Some((OutboundResult::InField, 45.0, 10.0)),
            exit_speed_retention: Some(0.95),
            thickness_offset_m: 0.02,
            ..miss
        };
        let mut quality = ContactApproachQuality::default();
        for (contact_first, line_drive) in [(miss, miss), (miss, foul), (foul, miss), (solid, foul)]
        {
            quality.observe(contact_first, line_drive);
        }
        let report = quality.report();
        for cohort in COHORTS {
            assert_eq!(report[cohort]["pairs"], 1);
        }
        let rescued = &report["contact_first_only_contact"];
        assert_eq!(rescued["contact_first"]["fouls_per_contact"], 1.0);
        assert_eq!(rescued["contact_first"]["mean_exit_speed_retention"], 0.15);
        assert!(rescued["line_drive"]["mean_exit_speed_retention"].is_null());
        assert_eq!(
            rescued["line_drive"]["abs_thickness_offset_counts_by_cm"][">7"],
            1
        );

        let both = &report["both_contact"];
        assert_eq!(both["contact_first"]["retention_at_least_95_pct"], 1);
        assert_eq!(both["line_drive"]["retention_at_least_80_pct"], 0);
        assert_eq!(
            both["contact_first_to_line_drive_adjusted_contact_types"]["SolidContact"]["MarginalContact"],
            1
        );
    }
}
