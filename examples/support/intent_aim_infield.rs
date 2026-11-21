use saberbb::domain::shared::{
    game::BattingResult,
    game_state::IntentAimPlay,
    infield_opportunity_audit::{InfieldReach, IntentAimInfield},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const BANDS: [&str; 4] = ["5_to_7_5", "7_5_to_10", "10_to_12_5", "12_5_to_15"];
type Transitions = BTreeMap<String, BTreeMap<String, u64>>;

fn angle_band(play: IntentAimPlay) -> Option<&'static str> {
    let (_, _, angle) = play.batting.flight?;
    play.infield?;
    let index = [7.5, 10.0, 12.5].partition_point(|edge| angle >= *edge);
    Some(BANDS[index])
}

fn fielding_location(play: IntentAimInfield) -> &'static str {
    match (
        play.selected_fielder.is_infielder(),
        play.selected_inside_50m,
    ) {
        (true, true) => "infielder_inside_50m",
        (true, false) => "infielder_at_or_beyond_50m",
        (false, true) => "outfielder_inside_50m",
        (false, false) => "outfielder_at_or_beyond_50m",
    }
}

#[derive(Default)]
struct BandCounts {
    plays: u64,
    singles: u64,
    errors: u64,
    outfield_singles: u64,
    known_infield_reach: u64,
    reachable_with_live_filters: u64,
    reachable_with_fine_grid_without_filters: u64,
    bounce: BTreeMap<&'static str, u64>,
    by_outcome_and_location: BTreeMap<&'static str, BTreeMap<String, u64>>,
    by_infield_reach: BTreeMap<&'static str, u64>,
    outfield_singles_by_infield_reach: BTreeMap<&'static str, u64>,
}

impl BandCounts {
    fn observe(&mut self, fielding: IntentAimInfield, outcome: Option<BattingResult>) {
        let single = outcome == Some(BattingResult::Single);
        let location = fielding_location(fielding);
        let batting = outcome.as_ref().map_or("NoBattingResult", AsRef::as_ref);
        let reach = fielding.infield_reach;
        self.plays += 1;
        self.singles += u64::from(single);
        self.errors += u64::from(fielding.selected_error);
        self.known_infield_reach += u64::from(reach != InfieldReach::MissingInputs);
        self.reachable_with_live_filters += u64::from(reach == InfieldReach::LiveGridAndFilters);
        self.reachable_with_fine_grid_without_filters += u64::from(matches!(
            reach,
            InfieldReach::LiveGridAndFilters
                | InfieldReach::LiveGridWithoutFilters
                | InfieldReach::FineGridWithoutFilters
        ));
        *self.by_infield_reach.entry(reach.label()).or_default() += 1;
        *self
            .by_outcome_and_location
            .entry(location)
            .or_default()
            .entry(batting.into())
            .or_default() += 1;
        if single && fielding.selected_fielder.is_outfielder() {
            self.outfield_singles += 1;
            *self
                .outfield_singles_by_infield_reach
                .entry(reach.label())
                .or_default() += 1;
        }
        let bounce = match fielding.projected_bounce_outside_50m {
            Some(false) => "inside_50m",
            Some(true) => "at_or_beyond_50m",
            None => "unknown",
        };
        *self.bounce.entry(bounce).or_default() += 1;
    }

    fn report(&self) -> Value {
        let selected_inside = self
            .by_outcome_and_location
            .get("infielder_inside_50m")
            .map_or(0, |counts| counts.values().sum());
        let rate = |n: u64, total: u64| (total > 0).then(|| n as f64 / total as f64);
        json!({
            "plays": self.plays, "singles": self.singles, "single_rate": rate(self.singles, self.plays),
            "outfield_singles": self.outfield_singles, "selected_fielding_errors": self.errors,
            "selected_infielder_inside_50m": selected_inside,
            "selected_infielder_inside_50m_rate": rate(selected_inside, self.plays),
            "projected_first_bounce": self.bounce, "batting_results_by_fielding_location": self.by_outcome_and_location,
            "infield_reach": self.by_infield_reach, "outfield_singles_by_infield_reach": self.outfield_singles_by_infield_reach,
            "known_infield_reach": self.known_infield_reach,
            "reachable_with_live_filters": self.reachable_with_live_filters,
            "reachable_with_live_filters_rate": rate(self.reachable_with_live_filters, self.known_infield_reach),
            "reachable_with_fine_grid_without_filters": self.reachable_with_fine_grid_without_filters,
            "reachable_with_fine_grid_without_filters_rate": rate(self.reachable_with_fine_grid_without_filters, self.known_infield_reach),
        })
    }
}

#[derive(Default)]
pub(super) struct InfieldCounts {
    bands: BTreeMap<&'static str, BandCounts>,
    batting_transitions: Transitions,
    fielding_transitions: Transitions,
}

impl InfieldCounts {
    pub(super) fn observe(&mut self, baseline: IntentAimPlay, play: IntentAimPlay) {
        let from_band = angle_band(baseline);
        let to_band = angle_band(play);
        if let Some((band, fielding)) = to_band.zip(play.infield) {
            self.bands
                .entry(band)
                .or_default()
                .observe(fielding, play.batting.outcome);
        }
        if from_band.is_none() && to_band.is_none() {
            return;
        }
        let key = |side: IntentAimPlay, band: Option<&str>, fielding: bool| {
            let band = band.unwrap_or("outside_target");
            if fielding {
                let location = side.infield.map_or("outside_target", fielding_location);
                format!("{band}:{location}")
            } else {
                let result = side
                    .batting
                    .outcome
                    .as_ref()
                    .map_or("NoBattingResult", AsRef::as_ref);
                format!("{band}:{result}")
            }
        };
        for (fielding, table) in [
            (false, &mut self.batting_transitions),
            (true, &mut self.fielding_transitions),
        ] {
            *table
                .entry(key(baseline, from_band, fielding))
                .or_default()
                .entry(key(play, to_band, fielding))
                .or_default() += 1;
        }
    }

    pub(super) fn report(&self) -> Value {
        let bands: BTreeMap<_, _> = BANDS
            .into_iter()
            .map(|band| {
                (
                    band,
                    self.bands
                        .get(band)
                        .map_or_else(|| BandCounts::default().report(), BandCounts::report),
                )
            })
            .collect();
        json!({
            "by_launch_angle_deg": bands,
            "baseline_to_trial_batting": self.batting_transitions,
            "baseline_to_trial_fielding": self.fielding_transitions,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saberbb::domain::shared::{
        ball::OutboundResult, game_state::SwingReplay, player::Position,
    };

    #[test]
    fn same_swing_tracks_angle_location_and_ruling_changes() {
        let play = |angle, position, reach, result| IntentAimPlay {
            batting: SwingReplay {
                launch_hla_deg: None,
                contact_z_m: 0.0,
                launch_spin: None,
                length_offset_terms: None,
                signed_thickness_offset_m: None,
                launch_geometry: None,
                exit_speed_retention: None,
                length_offset_m: 0.0,
                thickness_offset_m: 0.0,
                outfield_direct_out: false,
                contact_type:
                    saberbb::domain::resolver::batting_resolver::SwingContactType::SolidContact,
                double_fence: None,
                outcome: Some(result),
                flight: Some((OutboundResult::InField, 45.0, angle)),
            },
            infield: Some(IntentAimInfield {
                selected_fielder: position,
                selected_inside_50m: position == Position::SS,
                selected_error: false,
                projected_bounce_outside_50m: Some(position == Position::CF),
                infield_reach: reach,
            }),
        };
        let grounder = play(
            7.5,
            Position::SS,
            InfieldReach::LiveGridAndFilters,
            BattingResult::Out,
        );
        let liner = play(
            10.0,
            Position::CF,
            InfieldReach::ArrivalTooLate,
            BattingResult::Single,
        );
        let whiff = IntentAimPlay {
            batting: SwingReplay {
                launch_hla_deg: None,
                contact_z_m: 0.0,
                launch_spin: None,
                length_offset_terms: None,
                signed_thickness_offset_m: None,
                launch_geometry: None,
                exit_speed_retention: None,
                length_offset_m: 0.0,
                thickness_offset_m: 0.0,
                outfield_direct_out: false,
                contact_type:
                    saberbb::domain::resolver::batting_resolver::SwingContactType::SolidContact,
                double_fence: None,
                outcome: Some(BattingResult::StrikeSwung),
                flight: None,
            },
            infield: None,
        };
        let mut counts = InfieldCounts::default();
        counts.observe(grounder, liner);
        counts.observe(liner, whiff);
        let report = counts.report();
        assert_eq!(report["by_launch_angle_deg"]["7_5_to_10"]["plays"], 0);
        assert_eq!(report["by_launch_angle_deg"]["10_to_12_5"]["plays"], 1);
        assert_eq!(
            report["by_launch_angle_deg"]["10_to_12_5"]["projected_first_bounce"]["at_or_beyond_50m"],
            1
        );
        assert_eq!(
            report["by_launch_angle_deg"]["10_to_12_5"]["outfield_singles_by_infield_reach"]["arrival_too_late"],
            1
        );
        assert_eq!(
            report["baseline_to_trial_batting"]["7_5_to_10:Out"]["10_to_12_5:Single"],
            1
        );
        assert_eq!(
            report["baseline_to_trial_batting"]["10_to_12_5:Single"]["outside_target:StrikeSwung"],
            1
        );
        assert_eq!(
            report["baseline_to_trial_fielding"]["7_5_to_10:infielder_inside_50m"]["10_to_12_5:outfielder_at_or_beyond_50m"],
            1
        );
    }
}
