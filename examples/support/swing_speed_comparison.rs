use super::{Args, BattingResult, GameSchedule, derived_seed, simulate_game};
use anyhow::{Context, Result, ensure};
use rusqlite::Connection;
use saberbb::domain::{
    random_provider::{RandomProvider, RealRng},
    shared::{
        ball::OutboundResult,
        game_state::{SwingSpeedPlay, with_swing_speed_audit},
        prob::NormalParam,
    },
    strategy::batting_strategy::BattingStrategySettings,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
struct Counts {
    swings: u64,
    contacts: u64,
    fair: u64,
    hr: u64,
    other_hits: u64,
    ev_sum: f64,
    fast_fair: u64,
    transitions: BTreeMap<String, u64>,
}

impl Counts {
    fn add(&mut self, play: SwingSpeedPlay, baseline: SwingSpeedPlay) {
        self.swings += 1;
        if let Some((trajectory, speed, _)) = play.flight {
            self.contacts += 1;
            self.ev_sum += speed;
            if trajectory != OutboundResult::Foul
                && matches!(
                    play.outcome,
                    Some(
                        BattingResult::Single
                            | BattingResult::Double
                            | BattingResult::Triple
                            | BattingResult::HomeRun
                            | BattingResult::Out
                            | BattingResult::FieldersChoice
                            | BattingResult::DoublePlay
                    )
                )
            {
                self.fair += 1;
                self.fast_fair += u64::from(speed >= 50.0);
                self.hr += u64::from(play.outcome == Some(BattingResult::HomeRun));
                self.other_hits += u64::from(matches!(
                    play.outcome,
                    Some(BattingResult::Single | BattingResult::Double | BattingResult::Triple)
                ));
            }
        }
        let label = |result: Option<BattingResult>| {
            result.map_or("NoBattingResult".to_string(), |r| r.as_ref().to_string())
        };
        *self
            .transitions
            .entry(format!(
                "{} -> {}",
                label(baseline.outcome),
                label(play.outcome)
            ))
            .or_default() += 1;
    }

    fn report(&self, speed: f64) -> Value {
        let rate = |num: u64, n: u64| (n != 0).then(|| num as f64 / n as f64);
        json!({
            "swing_speed_ms": speed, "swings": self.swings, "contacts": self.contacts,
            "fair_contacts": self.fair, "fair_at_least_50_ms": self.fast_fair,
            "home_runs": self.hr, "non_home_run_hits": self.other_hits,
            "contact_per_swing": rate(self.contacts, self.swings),
            "home_run_per_swing": rate(self.hr, self.swings),
            "home_run_per_fair_contact": rate(self.hr, self.fair),
            "non_home_run_hit_per_swing": rate(self.other_hits, self.swings),
            "non_home_run_hit_per_non_home_run_fair": rate(self.other_hits, self.fair - self.hr),
            "mean_exit_speed_ms": (self.contacts != 0).then(|| self.ev_sum / self.contacts as f64),
            "outcome_transitions": self.transitions,
        })
    }
}

pub(super) fn run(
    conn: &Connection,
    args: &Args,
    games: &[GameSchedule],
    batter_id: i64,
    input_hash: &str,
) -> Result<Value> {
    let player = games
        .iter()
        .flat_map(|g| g.away_team.players.iter().chain(&g.home_team.players))
        .find(|p| p.info.id == batter_id)
        .with_context(|| format!("batter {batter_id} is not on the scheduled roster"))?;
    let baseline = player.batter()?.swing_speed;
    let speeds = if args.swing_speed_ms.is_empty() {
        &[33.72, 34.5, 35.5]
    } else {
        args.swing_speed_ms.as_slice()
    };
    ensure!(
        speeds.iter().all(|x| x.is_finite() && *x > 0.0),
        "swing speeds must be positive and finite"
    );
    let mut counts: Vec<_> = (0..=speeds.len()).map(|_| Counts::default()).collect();
    for &seed in &args.seeds {
        for game in games {
            let (result, plays) = with_swing_speed_audit(batter_id, speeds, || {
                simulate_game(
                    game.clone(),
                    Box::new(RealRng::from_seed(derived_seed(seed, game.id))),
                    BattingStrategySettings::player_driven(),
                    Some(args.max_pitches),
                    false,
                )
            });
            let result = result.with_context(|| {
                format!(
                    "swing-speed diagnostic failed in game {}, seed {seed}",
                    game.id
                )
            })?;
            let live: BTreeMap<_, _> = result
                .player_battings
                .iter()
                .filter(|b| {
                    b.batter_id == batter_id && b.swing_decision.as_ref().is_some_and(|d| d.swung)
                })
                .map(|b| (b.count_seq, b))
                .collect();
            ensure!(
                live.len()
                    == plays
                        .iter()
                        .filter(|(_, arms)| arms[0].outcome.is_some())
                        .count(),
                "paired swings do not match live count in game {}",
                game.id
            );
            for (seq, arms) in plays {
                let baseline_play = arms[0];
                let live = live.get(&seq);
                ensure!(
                    baseline_play.outcome == live.map(|b| b.result)
                        && live.is_none_or(|b| match baseline_play.flight {
                            None => b.ball.launch_speed == 0.0,
                            Some((_, speed, angle)) =>
                                b.ball.launch_speed == speed && b.ball.launch_angle == angle,
                        }),
                    "baseline swing mismatch in game {}, count {seq}",
                    game.id
                );
                for (count, play) in counts.iter_mut().zip(arms) {
                    count.add(play, baseline_play);
                }
            }
        }
    }
    ensure!(
        counts[0].swings > 0,
        "no swings for batter {batter_id}; increase --games or use an active batter"
    );
    Ok(json!({
        "batter_id": batter_id, "batter": player.full_name(), "season": args.season,
        "games_per_seed": games.len(), "seeds": args.seeds, "inputs_sha256": input_hash,
        "engine_source_sha256": saberbb::domain::seeded_diagnostic::ENGINE,
        "flight_drag_model": saberbb::domain::shared::flight_drag::current_model(),
        "definition": "New seeded games produce pitches and game states. Each selected swing is replayed with the same pitch, swing decision, count, runners, fielders and starting RNG position. Only swing speed changes; later pitches follow the live baseline game. Rates are per swing or fair contact, not per counterfactual plate appearance.",
        "baseline": counts[0].report(baseline),
        "alternatives": counts[1..].iter().zip(speeds).map(|(c, &speed)| c.report(speed)).collect::<Vec<_>>(),
        "generation": generation(conn, baseline, speeds)?,
    }))
}

fn generation(conn: &Connection, baseline: f64, speeds: &[f64]) -> Result<Value> {
    let mut query = conn.prepare(
        "SELECT mean,std_dev,skew,coefficient,offset FROM normal_param
         WHERE category1='player' AND category2='batter_info' AND name='swing_speed'",
    )?;
    let params = query.query_row([], |r| {
        Ok(NormalParam {
            mean: r.get(0)?,
            std_dev: r.get(1)?,
            skew: r.get(2)?,
            coefficient: r.get(3)?,
            offset: r.get(4)?,
        })
    })?;
    let mut rng = RealRng::from_seed(0);
    let generated: Vec<_> = (0..200_000).map(|_| rng.normal(params)).collect();
    let mut roster = conn.prepare("SELECT b.swing_speed FROM batter_info b JOIN player_info p ON p.id=b.player_id WHERE p.team_id IS NOT NULL")?;
    let roster = roster
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<f64>>>()?;
    Ok(json!({
        "configured_normal_param": params,
        "generated_sample_seed": 0,
        "generated_sample": distribution(generated, baseline, speeds)?,
        "rostered_batters": distribution(roster, baseline, speeds)?,
    }))
}

fn distribution(mut sample: Vec<f64>, baseline: f64, speeds: &[f64]) -> Result<Value> {
    ensure!(
        !sample.is_empty() && sample.iter().all(|x| x.is_finite()),
        "empty or invalid swing-speed distribution"
    );
    sample.sort_by(f64::total_cmp);
    let n = sample.len();
    let percentile = |p: usize| sample[(p * n - 1) / 1000];
    let share = |speed: f64| (n - sample.partition_point(|&x| x < speed)) as f64 / n as f64;
    Ok(json!({
        "n": n, "mean_ms": sample.iter().sum::<f64>() / n as f64, "p50_ms": percentile(500),
        "p95_ms": percentile(950), "p99_ms": percentile(990), "p999_ms": percentile(999),
        "max_ms": sample[n - 1], "share_at_or_above_baseline": share(baseline),
        "alternative_tail_shares": speeds.iter().map(|&speed| json!({
            "speed_ms": speed, "share_at_or_above": share(speed)
        })).collect::<Vec<_>>(),
    }))
}
