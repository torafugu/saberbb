use super::{Args, BattingResult, GameResult, GameSchedule, derived_seed, simulate_game};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    random_provider::RealRng,
    shared::game_state::{ContactFirstEvPlay, ContactFirstEvSwing, with_contact_first_ev_audit},
    strategy::batting_strategy::BattingStrategySettings,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

fn reductions(args: &Args) -> Result<&[f64]> {
    let values = if args.exit_speed_reductions_ms.is_empty() {
        &[2.0, 4.0, 6.0][..]
    } else {
        &args.exit_speed_reductions_ms[..]
    };
    ensure!(
        values.iter().all(|&speed| speed > 0.0 && speed < 45.0),
        "exit-speed reductions must be positive and less than 45 m/s"
    );
    ensure!(
        values
            .iter()
            .enumerate()
            .all(|(idx, value)| !values[..idx].contains(value)),
        "duplicate exit-speed reductions"
    );
    Ok(values)
}

fn angle_band(angle: f64) -> &'static str {
    const EDGES: [f64; 8] = [0.0, 5.0, 7.5, 10.0, 12.5, 15.0, 20.0, 30.0];
    const LABELS: [&str; 9] = [
        "below_0",
        "0_to_5",
        "5_to_7_5",
        "7_5_to_10",
        "10_to_12_5",
        "12_5_to_15",
        "15_to_20",
        "20_to_30",
        "30_and_above",
    ];
    LABELS[EDGES.partition_point(|edge| angle >= *edge)]
}

fn transition_label(play: ContactFirstEvPlay) -> String {
    match play.outcome {
        Some(BattingResult::Single) if play.fielder.is_some_and(|f| f.is_outfielder()) => {
            "OutfieldSingle".into()
        }
        Some(BattingResult::Out) if play.fielder.is_some_and(|f| f.is_infielder()) => {
            "InfieldOut".into()
        }
        Some(result) => result.as_ref().into(),
        None => "NoBattingResult".into(),
    }
}

#[derive(Default)]
struct Counts {
    plays: u64,
    outcomes: BTreeMap<String, u64>,
    transitions: BTreeMap<String, BTreeMap<String, u64>>,
    with_first_bounce: u64,
    first_bounce_inside_50m: u64,
    outfield_single_to_infield_out: u64,
    outfield_single_to_batter_out_at_first: u64,
    outfield_single_to_infield_direct_catch: u64,
}

impl Counts {
    fn observe(&mut self, baseline: ContactFirstEvPlay, play: ContactFirstEvPlay) {
        let before = transition_label(baseline);
        let after = transition_label(play);
        self.plays += 1;
        *self.outcomes.entry(after.clone()).or_default() += 1;
        if before == "OutfieldSingle" {
            self.outfield_single_to_infield_out += u64::from(after == "InfieldOut");
            self.outfield_single_to_batter_out_at_first += u64::from(play.batter_out_at_first);
            self.outfield_single_to_infield_direct_catch +=
                u64::from(after == "InfieldOut" && play.direct_catch);
        }
        *self
            .transitions
            .entry(before)
            .or_default()
            .entry(after)
            .or_default() += 1;
        if let Some(distance) = play.projected_bounce_distance_m {
            self.with_first_bounce += 1;
            self.first_bounce_inside_50m += u64::from(distance < 50.0);
        }
    }

    fn report(&self) -> Value {
        let outcome = |name: &str| self.outcomes.get(name).copied().unwrap_or_default();
        let baseline_outfield_singles = self
            .transitions
            .get("OutfieldSingle")
            .map_or(0, |row| row.values().sum());
        json!({
            "plays": self.plays, "batting_outcomes": self.outcomes,
            "singles": outcome("Single") + outcome("OutfieldSingle"),
            "outfield_singles": outcome("OutfieldSingle"),
            "infield_outs": outcome("InfieldOut"),
            "doubles": outcome("Double"), "triples": outcome("Triple"), "home_runs": outcome("HomeRun"),
            "baseline_outfield_singles": baseline_outfield_singles,
            "outfield_single_to_infield_out": self.outfield_single_to_infield_out,
            "outfield_single_to_infield_out_rate": (baseline_outfield_singles > 0)
                .then(|| self.outfield_single_to_infield_out as f64 / baseline_outfield_singles as f64),
            "outfield_single_to_batter_out_at_first": self.outfield_single_to_batter_out_at_first,
            "outfield_single_to_infield_direct_catch": self.outfield_single_to_infield_direct_catch,
            "projected_first_bounce": {
                "observations": self.with_first_bounce, "inside_50m": self.first_bounce_inside_50m,
            },
            "baseline_to_trial": self.transitions,
        })
    }
}

#[derive(Default)]
struct ArmCounts {
    total: Counts,
    by_angle: BTreeMap<&'static str, Counts>,
}

impl ArmCounts {
    fn observe(
        &mut self,
        baseline: ContactFirstEvPlay,
        play: ContactFirstEvPlay,
        band: &'static str,
    ) {
        self.total.observe(baseline, play);
        self.by_angle
            .entry(band)
            .or_default()
            .observe(baseline, play);
    }

    fn report(&self, reduction: f64) -> Value {
        json!({
            "reduction_ms": reduction,
            "total": self.total.report(),
            "by_baseline_launch_angle_deg": self.by_angle.iter()
                .map(|(&band, counts)| (band, counts.report())).collect::<BTreeMap<_, _>>(),
        })
    }
}

fn observe_game(
    result: &GameResult,
    swings: Vec<ContactFirstEvSwing>,
    total: &mut [ArmCounts],
    seed: &mut [ArmCounts],
) -> Result<()> {
    let live: BTreeMap<_, _> = result
        .player_battings
        .iter()
        .map(|b| (b.count_seq, b))
        .collect();
    for swing in swings {
        ensure!(
            swing.plays.len() == total.len(),
            "exit-speed replay arm count mismatch"
        );
        let baseline = swing.plays[0];
        let played = live.get(&swing.count_seq);
        ensure!(
            baseline.outcome == played.map(|b| b.result)
                && played.is_none_or(|b| b.ball.launch_speed == swing.baseline_exit_speed_ms
                    && b.ball.launch_angle == swing.baseline_launch_angle_deg),
            "exit-speed replay baseline mismatch in game {}, count {}",
            result.id,
            swing.count_seq
        );
        let band = angle_band(swing.baseline_launch_angle_deg);
        for ((total, seed), play) in total.iter_mut().zip(seed.iter_mut()).zip(swing.plays) {
            total.observe(baseline, play, band);
            seed.observe(baseline, play, band);
        }
    }
    Ok(())
}

pub(super) fn run(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let reductions = reductions(args)?;
    let empty = || {
        (0..=reductions.len())
            .map(|_| ArmCounts::default())
            .collect::<Vec<_>>()
    };
    let arms = |counts: &[ArmCounts]| {
        counts
            .iter()
            .zip(std::iter::once(0.0).chain(reductions.iter().copied()))
            .map(|(count, reduction)| count.report(reduction))
            .collect::<Vec<_>>()
    };
    let mut pooled = empty();
    let mut reports = Vec::new();
    for &game_seed in &args.seeds {
        let mut seed = empty();
        for game in games {
            let (result, swings) = with_contact_first_ev_audit(reductions, || {
                simulate_game(
                    game.clone(),
                    Box::new(RealRng::from_seed(derived_seed(game_seed, game.id))),
                    BattingStrategySettings::player_driven(),
                    Some(args.max_pitches),
                    false,
                )
            });
            let result = result.with_context(|| {
                format!(
                    "ContactFirst exit-speed replay failed in game {}, seed {game_seed}",
                    game.id
                )
            })?;
            observe_game(&result, swings, &mut pooled, &mut seed)?;
        }
        eprintln!(
            "ContactFirst exit-speed replay, seed {game_seed}: {} games complete",
            games.len()
        );
        reports.push(json!({"seed":game_seed.to_string(),"arms":arms(&seed)}));
    }
    Ok(json!({
        "definitions": {
            "population":"Fair baseline ContactFirst contact at EV >=45 m/s, including home runs and plays interrupted before a batting ruling. Each arm uses this same population after the exit-speed reduction.",
            "paired_replay":"Hold pitch, selected intent, contact, launch angles, spin, stadium and starting game state fixed. Reintegrate flight at the reduced exit speed; run normal fielding and baserunning from the same post-launch RNG state. Only the baseline game advances.",
            "transitions":"OutfieldSingle is a Single whose first recorded fielder was an outfielder. InfieldOut is an official Out whose first recorded fielder was an infielder, including pitcher and catcher. Batter outs at first are counted from recorded first-base rulings and can include double plays. First bounce is projected irrespective of earlier fielding.",
        },
        "pooled": arms(&pooled),
        "reports": reports,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use saberbb::domain::shared::player::Position;

    #[test]
    fn validates_the_reduction_list() {
        let args = |extra: &[&str]| {
            let mut args = vec!["check", "--season", "2026", "--contact-first-ev-comparison"];
            args.extend_from_slice(extra);
            Args::try_parse_from(args).unwrap()
        };
        assert_eq!(reductions(&args(&[])).unwrap(), [2.0, 4.0, 6.0]);
        assert_eq!(
            reductions(&args(&["--exit-speed-reductions-ms", "1.5,3"])).unwrap(),
            [1.5, 3.0]
        );
        for invalid in ["0", "45", "2,2", "NaN"] {
            assert!(reductions(&args(&["--exit-speed-reductions-ms", invalid])).is_err());
        }
    }

    #[test]
    fn tracks_direct_catches_and_first_base_outs_separately() {
        let single = ContactFirstEvPlay {
            outcome: Some(BattingResult::Single),
            fielder: Some(Position::CF),
            direct_catch: false,
            batter_out_at_first: false,
            projected_bounce_distance_m: Some(60.0),
        };
        let mut totals = Counts::default();
        totals.observe(
            single,
            ContactFirstEvPlay {
                outcome: Some(BattingResult::Out),
                fielder: Some(Position::SS),
                batter_out_at_first: true,
                projected_bounce_distance_m: Some(48.0),
                ..single
            },
        );
        totals.observe(
            single,
            ContactFirstEvPlay {
                outcome: Some(BattingResult::Out),
                fielder: Some(Position::SS),
                direct_catch: true,
                ..single
            },
        );
        totals.observe(
            single,
            ContactFirstEvPlay {
                outcome: Some(BattingResult::DoublePlay),
                fielder: Some(Position::SB),
                batter_out_at_first: true,
                ..single
            },
        );
        let report = totals.report();
        assert_eq!(report["baseline_outfield_singles"], 3);
        assert_eq!(report["outfield_single_to_infield_out"], 2);
        assert_eq!(report["outfield_single_to_batter_out_at_first"], 2);
        assert_eq!(report["outfield_single_to_infield_direct_catch"], 1);
        assert_eq!(report["projected_first_bounce"]["inside_50m"], 1);
        assert_eq!(
            report["baseline_to_trial"]["OutfieldSingle"]["InfieldOut"],
            2
        );
        assert_eq!(angle_band(9.999), "7_5_to_10");
        assert_eq!(angle_band(10.0), "10_to_12_5");
    }
}
