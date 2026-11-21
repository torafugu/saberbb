use super::outcome_stage;
use anyhow::{Result, ensure};
use saberbb::domain::{
    resolver::batting_resolver::SwingContactType,
    shared::game_state::{IntentAimSwing, SwingReplay, TwoStrikePitch},
    strategy::batting_strategy::BattedBallIntent,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const NEAR_OUTSIDE_LIMIT: f64 = 0.15;
const ZONES: [&str; 3] = ["in_zone", "just_outside", "farther_outside"];

fn zone_index(distance: f64) -> usize {
    if distance <= 0.0 {
        0
    } else if distance <= NEAR_OUTSIDE_LIMIT {
        1
    } else {
        2
    }
}

fn rate(count: u64, denominator: u64) -> Option<f64> {
    (denominator > 0).then(|| count as f64 / denominator as f64)
}

#[derive(Default)]
struct SwingOutcomes {
    swings: u64,
    whiffs: u64,
    fouls: u64,
    solids: u64,
}

impl SwingOutcomes {
    fn observe(&mut self, play: SwingReplay) {
        self.swings += 1;
        self.whiffs += u64::from(play.contact_type == SwingContactType::SwungAndMiss);
        self.fouls += u64::from(outcome_stage(play) == "foul");
        self.solids += u64::from(play.contact_type == SwingContactType::SolidContact);
    }

    fn report(&self) -> Value {
        json!({
            "swings": self.swings,
            "whiffs": self.whiffs,
            "fouls": self.fouls,
            "solid_contacts": self.solids,
            "whiffs_per_swing": rate(self.whiffs, self.swings),
            "fouls_per_swing": rate(self.fouls, self.swings),
            "solid_contacts_per_swing": rate(self.solids, self.swings),
        })
    }
}

#[derive(Default)]
struct LivePitches {
    pitches: u64,
    outcomes: SwingOutcomes,
}

impl LivePitches {
    fn observe(&mut self, play: Option<SwingReplay>) {
        self.pitches += 1;
        if let Some(play) = play {
            self.outcomes.observe(play);
        }
    }

    fn report(&self) -> Value {
        json!({
            "pitches": self.pitches,
            "takes": self.pitches - self.outcomes.swings,
            "swings_per_pitch": rate(self.outcomes.swings, self.pitches),
            "outcomes": self.outcomes.report(),
        })
    }
}

#[derive(Default)]
struct ZoneCounts {
    all: LivePitches,
    contact_first: LivePitches,
    line_drive: LivePitches,
    paired_contact_first: SwingOutcomes,
    paired_line_drive: SwingOutcomes,
}

impl ZoneCounts {
    fn observe(&mut self, pitch: TwoStrikePitch, replay: Option<&IntentAimSwing>) {
        let live = replay.map(|swing| swing.plays[0].batting);
        self.all.observe(live);
        match pitch.intent {
            BattedBallIntent::ContactFirst => self.contact_first.observe(live),
            BattedBallIntent::LineDrive => self.line_drive.observe(live),
        }
        if let Some(swing) = replay {
            self.paired_contact_first.observe(swing.plays[1].batting);
            self.paired_line_drive.observe(swing.plays[2].batting);
        }
    }

    fn report(&self) -> Value {
        json!({
            "live": {
                "all": self.all.report(),
                "contact_first": self.contact_first.report(),
                "line_drive": self.line_drive.report(),
            },
            "paired_swings": {
                "contact_first": self.paired_contact_first.report(),
                "line_drive": self.paired_line_drive.report(),
            },
        })
    }
}

#[derive(Default)]
pub(super) struct TwoStrikeApproach {
    zones: [ZoneCounts; 3],
}

impl TwoStrikeApproach {
    pub(super) fn observe_game(
        &mut self,
        pitches: Vec<TwoStrikePitch>,
        swings: &[IntentAimSwing],
    ) -> Result<()> {
        let mut swings: BTreeMap<_, _> = swings
            .iter()
            .map(|swing| (swing.count_seq, swing))
            .collect();
        for pitch in pitches {
            let replay = swings.remove(&pitch.count_seq);
            ensure!(
                pitch.swung == replay.is_some()
                    && replay
                        .is_none_or(|swing| swing.intent == pitch.intent && swing.plays.len() == 3),
                "two-strike pitch and swing replay disagree at count {}",
                pitch.count_seq,
            );
            self.zones[zone_index(pitch.distance_from_zone_edge)].observe(pitch, replay);
        }
        Ok(())
    }

    pub(super) fn report(&self) -> Value {
        let by_zone: BTreeMap<_, _> = ZONES
            .into_iter()
            .zip(self.zones.iter().map(ZoneCounts::report))
            .collect();
        json!({
            "definition": "Two strikes before the pitch. Distance is the game's normalized distance outside the strike-zone rectangle. In-zone includes the boundary; just outside includes positive distances through the stated limit. Live swing rates divide by all pitches, including takes. On takes, intent is the plan computed before the swing decision. Outcome rates divide by swings. Both paired arms replay the same live swings in each zone with their respective aim offsets. The live swing policy is common to both intents.",
            "just_outside_max_normalized_distance": NEAR_OUTSIDE_LIMIT,
            "by_zone": by_zone,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saberbb::domain::shared::{
        ball::OutboundResult, game::BattingResult, game_state::IntentAimPlay,
    };

    #[test]
    fn separates_takes_from_swing_outcomes_for_each_zone_and_plan() {
        let pitch = |count_seq, intent, distance_from_zone_edge, swung| TwoStrikePitch {
            count_seq,
            intent,
            distance_from_zone_edge,
            swung,
        };
        let contact = BattedBallIntent::ContactFirst;
        let line = BattedBallIntent::LineDrive;
        let foul = SwingReplay {
            outcome: Some(BattingResult::Foul),
            contact_type: SwingContactType::SolidContact,
            flight: Some((OutboundResult::Foul, 20.0, 15.0)),
            exit_speed_retention: None,
            length_offset_m: 0.02,
            thickness_offset_m: 0.01,
            outfield_direct_out: false,
            double_fence: None,
        };
        let miss = SwingReplay {
            outcome: None,
            contact_type: SwingContactType::SwungAndMiss,
            flight: None,
            ..foul
        };
        let play = |batting| IntentAimPlay {
            batting,
            infield: None,
        };
        let swing = IntentAimSwing {
            count_seq: 2,
            intent: contact,
            plays: vec![play(foul), play(foul), play(miss)],
        };
        let mut counts = TwoStrikeApproach::default();
        counts
            .observe_game(
                vec![
                    pitch(1, contact, 0.0, false),
                    pitch(2, contact, 0.15, true),
                    pitch(3, contact, 0.01, false),
                    pitch(4, line, 0.151, false),
                ],
                &[swing],
            )
            .unwrap();
        let report = counts.report();
        let zones = &report["by_zone"];
        let just_outside = &zones["just_outside"];
        assert_eq!(zones["in_zone"]["live"]["all"]["takes"], 1);
        assert_eq!(zones["farther_outside"]["live"]["line_drive"]["takes"], 1);
        let live = &just_outside["live"]["contact_first"];
        assert_eq!(live["swings_per_pitch"], 0.5);
        assert_eq!(live["outcomes"]["solid_contacts_per_swing"], 1.0);
        assert_eq!(live["outcomes"]["fouls_per_swing"], 1.0);
        assert_eq!(just_outside["paired_swings"]["line_drive"]["whiffs"], 1);
        assert!(just_outside["live"]["line_drive"]["swings_per_pitch"].is_null());
    }
}
