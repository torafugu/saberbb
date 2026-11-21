use super::outcome_stage;
use saberbb::domain::{
    resolver::batting_resolver::wrap_hla_deg,
    shared::{ball::FOUL_DEGREE, game_state::SwingReplay},
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default, Serialize)]
struct LaunchAngleCell {
    fair: u64,
    foul: u64,
}

fn horizontal_band(angle: Option<f64>) -> &'static str {
    let Some(angle) = angle.filter(|angle| angle.is_finite()).map(wrap_hla_deg) else {
        return "unknown";
    };
    if angle <= -90.0 {
        "le_neg_90"
    } else if angle <= -FOUL_DEGREE {
        "neg_90_to_neg_45"
    } else if angle < -30.0 {
        "neg_45_to_neg_30"
    } else if angle <= 30.0 {
        "neg_30_to_30"
    } else if angle < FOUL_DEGREE {
        "30_to_45"
    } else if angle < 90.0 {
        "45_to_90"
    } else {
        "ge_90"
    }
}

#[derive(Default)]
struct LaunchCounts {
    no_launch: u64,
    fouls: u64,
    fair_below_40_deg: u64,
    fair_40_to_50_deg: u64,
    fair_above_50_deg: u64,
    fair_unresolved: u64,
    by_vla_and_hla: BTreeMap<&'static str, BTreeMap<&'static str, LaunchAngleCell>>,
}

impl LaunchCounts {
    fn observe(&mut self, play: SwingReplay) {
        let Some((_, _, angle)) = play.flight else {
            self.no_launch += 1;
            return;
        };
        let (vla_band, fair_count) = if angle < 40.0 {
            ("below_40", &mut self.fair_below_40_deg)
        } else if angle <= 50.0 {
            ("40_to_50", &mut self.fair_40_to_50_deg)
        } else {
            ("above_50", &mut self.fair_above_50_deg)
        };
        let cell = self
            .by_vla_and_hla
            .entry(vla_band)
            .or_default()
            .entry(horizontal_band(play.launch_hla_deg))
            .or_default();
        let stage = outcome_stage(play);
        if stage == "foul" {
            self.fouls += 1;
            cell.foul += 1;
        } else {
            *fair_count += 1;
            cell.fair += 1;
            self.fair_unresolved += u64::from(stage == "fair_unresolved");
        }
    }

    fn fair_launches(&self) -> u64 {
        self.fair_below_40_deg + self.fair_40_to_50_deg + self.fair_above_50_deg
    }

    fn launches(&self) -> u64 {
        self.fouls + self.fair_launches()
    }

    fn report(&self, all_launches: u64) -> Value {
        let rate = |count, total| (total > 0).then(|| count as f64 / total as f64);
        let fair = self.fair_launches();
        let launches = self.launches();
        json!({
            "swings": self.no_launch + launches,
            "no_launch": self.no_launch,
            "launches": launches,
            "share_of_all_launches": rate(launches, all_launches),
            "fouls": self.fouls,
            "launches_by_vla_and_hla_deg": self.by_vla_and_hla,
            "fouls_per_launch": rate(self.fouls, launches),
            "fair_launches": fair,
            "fair_unresolved": self.fair_unresolved,
            "fair_below_40_deg": self.fair_below_40_deg,
            "fair_40_to_50_deg": self.fair_40_to_50_deg,
            "fair_above_50_deg": self.fair_above_50_deg,
            "fair_40_to_50_deg_per_fair_launch": rate(self.fair_40_to_50_deg, fair),
            "fair_above_50_deg_per_fair_launch": rate(self.fair_above_50_deg, fair),
        })
    }
}

#[derive(Default)]
pub(super) struct ContactLaunchProfile {
    all: LaunchCounts,
    by_contact_type: BTreeMap<String, LaunchCounts>,
}

impl ContactLaunchProfile {
    pub(super) const DEFINITION: &str = "Grouped by adjusted contact type. Fouls include foul trajectories and launches ruled foul. Fair launch angle bands use unadjusted vertical launch angle, with both 40 and 50 degrees included in the middle band. Fair trajectories with no recorded batting result are included in the fair counts and identified as fair_unresolved. Launch rates include fouls in the denominator; rates per fair launch exclude them. An angle above 50 degrees is a pop-up proxy. The joint counts use the same vertical bands for both fair and foul launches. Signed horizontal launch angle is measured from center field (positive to first base) and wrapped to [-180, 180). Absolute angles through 30 degrees are central, greater than 30 and below 45 are just inside the initial foul-line angle, 45 to below 90 are outside, and 90 or more are sideways or back. A cell's fair/foul counts follow the ruling or trajectory, independently of the initial angle. Missing or non-finite horizontal angles use the unknown bin.";

    pub(super) fn observe(&mut self, play: SwingReplay) {
        self.all.observe(play);
        self.by_contact_type
            .entry(play.contact_type.as_ref().to_owned())
            .or_default()
            .observe(play);
    }

    pub(super) fn report(&self) -> Value {
        let launches = self.all.launches();
        let by_contact_type: BTreeMap<_, _> = self
            .by_contact_type
            .iter()
            .map(|(kind, counts)| (kind, counts.report(launches)))
            .collect();
        json!({
            "all": self.all.report(launches),
            "by_contact_type": by_contact_type,
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
    fn counts_high_fair_launches_separately_from_fouls() {
        let play = |flight, outcome, contact_type, launch_hla_deg| SwingReplay {
            outcome,
            contact_type,
            flight,
            launch_hla_deg,
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
        let marginal = SwingContactType::MarginalContact;
        let weak = SwingContactType::WeakContact;
        let fair = |angle| Some((OutboundResult::InField, 20.0, angle));
        let mut profile = ContactLaunchProfile::default();
        for sample in [
            play(None, None, SwingContactType::SwungAndMiss, None),
            play(fair(39.9), Some(BattingResult::Out), marginal, None),
            play(fair(40.0), Some(BattingResult::Out), marginal, Some(-45.0)),
            play(fair(50.0), Some(BattingResult::Out), marginal, Some(30.0)),
            play(fair(50.1), None, marginal, Some(15.0)),
            play(fair(60.0), Some(BattingResult::Foul), weak, Some(45.0)),
            play(
                Some((OutboundResult::Foul, 20.0, 70.0)),
                None,
                weak,
                Some(180.0),
            ),
        ] {
            profile.observe(sample);
        }

        let report = profile.report();
        let all = &report["all"];
        assert_eq!(all["swings"], 7);
        assert_eq!(all["fair_launches"], 4);
        assert_eq!(all["fair_unresolved"], 1);
        assert_eq!(all["fouls"], 2);
        let angles = &all["launches_by_vla_and_hla_deg"];
        assert_eq!(angles["below_40"]["unknown"]["fair"], 1);
        assert_eq!(angles["40_to_50"]["neg_90_to_neg_45"]["fair"], 1);
        assert_eq!(angles["40_to_50"]["neg_30_to_30"]["fair"], 1);
        assert_eq!(angles["above_50"]["neg_30_to_30"]["fair"], 1);
        assert_eq!(angles["above_50"]["45_to_90"]["foul"], 1);
        assert_eq!(angles["above_50"]["le_neg_90"]["foul"], 1);
        let marginal = &report["by_contact_type"]["MarginalContact"];
        assert_eq!(marginal["fair_below_40_deg"], 1);
        assert_eq!(marginal["fair_40_to_50_deg"], 2);
        assert_eq!(marginal["fair_above_50_deg_per_fair_launch"], 0.25);
        assert_eq!(report["by_contact_type"]["WeakContact"]["fouls"], 2);
        assert!(report["by_contact_type"]["SwungAndMiss"]["fouls_per_launch"].is_null());
    }
}
