use saberbb::domain::{
    resolver::batting_resolver::{
        CONTACT_LENGTH_LIMIT_M, CONTACT_THICKNESS_LIMIT_M, SwingContactType,
    },
    shared::game_state::SwingReplay,
    strategy::batting_strategy::BattedBallIntent,
};
use serde_json::{Value, json};

#[derive(Default)]
struct Crossing {
    swings: u64,
    length_within_at_both_settings: u64,
    contact_changed: u64,
}

impl Crossing {
    fn observe(&mut self, length_within_at_both_settings: bool, contact_changed: bool) {
        self.swings += 1;
        self.length_within_at_both_settings += u64::from(length_within_at_both_settings);
        self.contact_changed += u64::from(contact_changed);
    }

    fn report(&self) -> Value {
        json!({
            "swings": self.swings,
            "length_within_at_both_settings": self.length_within_at_both_settings,
            "contact_changed": self.contact_changed,
        })
    }
}

#[derive(Default)]
struct ThicknessComparison {
    swings: u64,
    absolute_change_in_thickness_magnitude_m: f64,
    absolute_change_in_length_magnitude_m: f64,
    inside_to_outside: Crossing,
    outside_to_inside: Crossing,
    contact_to_whiff: u64,
    whiff_to_contact: u64,
}

impl ThicknessComparison {
    fn observe(&mut self, before: SwingReplay, after: SwingReplay) {
        self.swings += 1;
        self.absolute_change_in_thickness_magnitude_m +=
            (after.thickness_offset_m - before.thickness_offset_m).abs();
        self.absolute_change_in_length_magnitude_m +=
            (after.length_offset_m - before.length_offset_m).abs();
        let before_whiff = before.contact_type == SwingContactType::SwungAndMiss;
        let after_whiff = after.contact_type == SwingContactType::SwungAndMiss;
        self.contact_to_whiff += u64::from(!before_whiff && after_whiff);
        self.whiff_to_contact += u64::from(before_whiff && !after_whiff);

        let before_inside = before.thickness_offset_m <= CONTACT_THICKNESS_LIMIT_M;
        let after_inside = after.thickness_offset_m <= CONTACT_THICKNESS_LIMIT_M;
        let crossing = match (before_inside, after_inside) {
            (true, false) => &mut self.inside_to_outside,
            (false, true) => &mut self.outside_to_inside,
            _ => return,
        };
        let length_within_at_both_settings = before.length_offset_m <= CONTACT_LENGTH_LIMIT_M
            && after.length_offset_m <= CONTACT_LENGTH_LIMIT_M;
        crossing.observe(length_within_at_both_settings, before_whiff != after_whiff);
    }

    fn report(&self) -> Value {
        let mean_mm = |sum: f64| (self.swings > 0).then(|| sum * 1000.0 / self.swings as f64);
        json!({
            "swings": self.swings,
            "mean_absolute_change_in_thickness_magnitude_mm": mean_mm(self.absolute_change_in_thickness_magnitude_m),
            "mean_absolute_change_in_length_magnitude_mm": mean_mm(self.absolute_change_in_length_magnitude_m),
            "thickness_inside_to_outside": self.inside_to_outside.report(),
            "thickness_outside_to_inside": self.outside_to_inside.report(),
            "contact_to_whiff": self.contact_to_whiff,
            "whiff_to_contact": self.whiff_to_contact,
        })
    }
}

#[derive(Default)]
pub(super) struct TimingThicknessCrossings {
    all: ThicknessComparison,
    contact_first: ThicknessComparison,
    line_drive: ThicknessComparison,
}

impl TimingThicknessCrossings {
    pub(super) fn observe(
        &mut self,
        intent: BattedBallIntent,
        before: SwingReplay,
        after: SwingReplay,
    ) {
        self.all.observe(before, after);
        match intent {
            BattedBallIntent::ContactFirst => self.contact_first.observe(before, after),
            BattedBallIntent::LineDrive => self.line_drive.observe(before, after),
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

    fn replay(thickness_m: f64, length_m: f64, contact_type: SwingContactType) -> SwingReplay {
        SwingReplay {
            outcome: None,
            contact_type,
            flight: None,
            launch_hla_deg: None,
            contact_z_m: 0.0,
            launch_spin: None,
            exit_speed_retention: None,
            length_offset_m: length_m,
            length_offset_terms: None,
            thickness_offset_m: thickness_m,
            outfield_direct_out: false,
            double_fence: None,
        }
    }

    #[test]
    fn separates_thickness_crossings_from_contact_changes() {
        use SwingContactType::{MarginalContact, SwungAndMiss};
        let contact = replay(0.070, 0.160, MarginalContact);
        let thickness_miss = replay(0.071, 0.160, SwungAndMiss);
        let length_miss = replay(0.070, 0.161, SwungAndMiss);
        let mut counts = TimingThicknessCrossings::default();
        counts.observe(BattedBallIntent::ContactFirst, contact, thickness_miss);
        counts.observe(BattedBallIntent::ContactFirst, thickness_miss, length_miss);
        counts.observe(BattedBallIntent::LineDrive, thickness_miss, contact);
        counts.observe(BattedBallIntent::LineDrive, length_miss, contact);
        counts.observe(BattedBallIntent::LineDrive, contact, contact);

        let report = counts.report();
        let all = &report["all"];
        assert_eq!(all["swings"], 5);
        assert_eq!(all["thickness_inside_to_outside"]["swings"], 1);
        assert_eq!(
            all["thickness_inside_to_outside"]["length_within_at_both_settings"],
            1
        );
        assert_eq!(all["thickness_inside_to_outside"]["contact_changed"], 1);
        assert_eq!(all["thickness_outside_to_inside"]["swings"], 2);
        assert_eq!(
            all["thickness_outside_to_inside"]["length_within_at_both_settings"],
            1
        );
        assert_eq!(all["thickness_outside_to_inside"]["contact_changed"], 1);
        assert_eq!(all["contact_to_whiff"], 1);
        assert_eq!(all["whiff_to_contact"], 2);
        assert_eq!(report["contact_first"]["swings"], 2);
        assert_eq!(report["line_drive"]["swings"], 3);
        let mean = all["mean_absolute_change_in_thickness_magnitude_mm"]
            .as_f64()
            .unwrap();
        assert!((mean - 0.6).abs() < 1e-10);
    }
}
