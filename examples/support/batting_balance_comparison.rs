use super::{Args, Counts, GameSchedule, accumulate, derived_seed, simulate_game};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    random_provider::RealRng, shared::game_state::with_line_drive_attack_angle_trial,
    strategy::batting_strategy::BattingStrategySettings,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Serialize)]
struct UpperTail {
    threshold: f64,
    players_above_threshold: usize,
}

impl UpperTail {
    fn from_values(mut values: Vec<f64>) -> Self {
        values.sort_by(f64::total_cmp);
        // Nearest-rank 90th percentile of unique roster batters.
        let threshold = values[(9 * values.len()).div_ceil(10) - 1];
        let players_above_threshold = values.iter().filter(|&&value| value > threshold).count();
        Self {
            threshold,
            players_above_threshold,
        }
    }

    fn compress(&self, value: f64) -> f64 {
        if value > self.threshold {
            self.threshold + 0.5 * (value - self.threshold)
        } else {
            value
        }
    }
}

#[derive(Default, Serialize)]
struct QualifiedDistribution {
    below_200: u64,
    from_200_below_250: u64,
    from_250_through_300: u64,
    above_300_below_400: u64,
    from_400_below_500: u64,
    at_least_500: u64,
}

impl QualifiedDistribution {
    fn observe(&mut self, hits: u64, ab: u64) {
        // Compare exact batting averages so .250 and .300 are both included.
        let scaled_hits = 1000 * hits;
        match scaled_hits {
            n if n < 200 * ab => self.below_200 += 1,
            n if n < 250 * ab => self.from_200_below_250 += 1,
            n if n <= 300 * ab => self.from_250_through_300 += 1,
            n if n < 400 * ab => self.above_300_below_400 += 1,
            n if n < 500 * ab => self.from_400_below_500 += 1,
            _ => self.at_least_500 += 1,
        }
    }
}

fn rates(counts: &Counts) -> Value {
    let mut summary = counts.json();
    let per_100_pa = |events| (counts.pa != 0).then(|| 100.0 * events as f64 / counts.pa as f64);
    summary["hr_per_100_pa"] = json!(per_100_pa(counts.hr));
    summary["walks_per_100_pa"] = json!(per_100_pa(counts.walks));
    summary["strikeouts_per_100_pa"] = json!(per_100_pa(counts.strikeouts));
    summary
}

pub(super) fn run(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let roster: BTreeMap<_, _> = games
        .iter()
        .flat_map(|game| [&game.away_team, &game.home_team])
        .flat_map(|team| {
            team.players
                .iter()
                .map(move |player| (player.info.id, (team.id, player)))
        })
        .collect();
    let skills: Vec<_> = roster
        .values()
        .filter_map(|(_, p)| p.offense_skills.batter)
        .collect();
    ensure!(
        !skills.is_empty(),
        "no roster batters for the batting balance comparison"
    );
    ensure!(
        skills
            .iter()
            .all(|b| b.bat_control.is_finite() && b.batting_eye.is_finite()),
        "non-finite roster batting ability"
    );
    let bat_control = UpperTail::from_values(skills.iter().map(|b| b.bat_control).collect());
    let batting_eye = UpperTail::from_values(skills.iter().map(|b| b.batting_eye).collect());
    let angle_deg = f64::from(args.balance_line_drive_attack_deg.unwrap_or(2));

    let mut arms = Vec::new();
    for (label, compress, delta_deg) in [
        ("current", false, 0.0),
        ("compression_only", true, 0.0),
        ("angle_only", false, angle_deg),
        ("combined", true, angle_deg),
    ] {
        let mut pooled = Counts::default();
        let mut reports = Vec::new();
        for &seed in &args.seeds {
            let mut total = Counts::default();
            let mut teams = BTreeMap::new();
            let mut players = BTreeMap::new();
            for original in games {
                let mut game = original.clone();
                if compress {
                    for player in game
                        .away_team
                        .players
                        .iter_mut()
                        .chain(&mut game.home_team.players)
                    {
                        if let Some(batter) = &mut player.offense_skills.batter {
                            batter.bat_control = bat_control.compress(batter.bat_control);
                            batter.batting_eye = batting_eye.compress(batter.batting_eye);
                        }
                    }
                }
                let result = with_line_drive_attack_angle_trial(delta_deg, || {
                    simulate_game(
                        game,
                        Box::new(RealRng::from_seed(derived_seed(seed, original.id))),
                        BattingStrategySettings::player_driven(),
                        Some(args.max_pitches),
                        false,
                    )
                })
                .with_context(|| {
                    format!("batting balance {label}, seed {seed}, game {}", original.id)
                })?;
                accumulate(&mut total, &mut teams, &mut players, original, &result)?;
            }
            let mut distribution = QualifiedDistribution::default();
            let mut qualified_batters = 0;
            for (id, counts) in players {
                let team_games = teams[&roster[&id].0].games;
                let required_pa = (31 * team_games + 5) / 10;
                if counts.ab > 0 && counts.pa >= required_pa {
                    qualified_batters += 1;
                    distribution.observe(
                        counts.single + counts.double + counts.triple + counts.hr,
                        counts.ab,
                    );
                }
            }
            let at_least_400 = distribution.from_400_below_500 + distribution.at_least_500;
            reports.push(json!({
                "seed": seed.to_string(), "league": rates(&total),
                "team_games": teams.iter().map(|(&id, c)| (id, c.games)).collect::<BTreeMap<_, _>>(),
                "qualified_batters": qualified_batters, "qualified_at_least_400": at_least_400,
                "qualified_batting_average_distribution": distribution,
            }));
            eprintln!(
                "Batting balance {label}, seed {seed}: {} games complete",
                games.len()
            );
            pooled.merge(total);
        }
        arms.push(json!({
            "arm": label, "compress_upper_tail": compress, "line_drive_attack_delta_deg": delta_deg,
            "pooled_league": rates(&pooled), "reports": reports,
        }));
    }
    Ok(json!({
        "definition": "Replay independent regular-season games with the same original rosters, schedules and starting seeds. Later pitches and results can diverge. Qualification is evaluated separately per seed, using each batter's team game count and 3.1 PA per game rounded to the nearest whole PA. Batting averages are unrounded for banding. The attack delta changes the live LineDrive execution angle only.",
        "compression": {
            "definition": "Find each attribute's nearest-rank 90th percentile among unique roster batters in the selected schedules. Keep values at or below it and halve the excess above it. Use the same thresholds across all arms and seeds.",
            "roster_batters": skills.len(), "retained_excess_fraction": 0.5,
            "bat_control": bat_control, "batting_eye": batting_eye,
        },
        "arms": arms,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn upper_tail_compression_preserves_the_threshold_and_values_below_it() {
        let tail = UpperTail::from_values((1..=10).map(f64::from).collect());
        assert_eq!(tail.threshold, 9.0);
        assert_eq!(tail.players_above_threshold, 1);
        assert_eq!(tail.compress(1.0), 1.0);
        assert_eq!(tail.compress(9.0), 9.0);
        assert_eq!(tail.compress(10.0), 9.5);
    }

    #[test]
    fn batting_average_bands_include_the_requested_boundaries() {
        let mut bands = QualifiedDistribution::default();
        for hits in [199, 200, 249, 250, 300, 301, 399, 400, 499, 500] {
            bands.observe(hits, 1000);
        }
        assert_eq!(
            json!(bands),
            json!({
                "below_200": 1, "from_200_below_250": 2, "from_250_through_300": 2,
                "above_300_below_400": 2, "from_400_below_500": 2, "at_least_500": 1,
            })
        );
    }
}
