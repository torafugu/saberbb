use super::{Args, Counts, GameSchedule, accumulate, derived_seed, simulate_game};
use anyhow::{Context, Result};
use saberbb::domain::{
    random_provider::RealRng,
    shared::{
        ball::OutboundResult,
        game::BattingResult,
        game_state::{CountIntentSwing, with_count_intent_audit},
    },
    strategy::batting_strategy::{BattedBallIntent, BattingStrategySettings},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

#[derive(Default)]
struct StrikeCounts {
    swings: u64,
    contact_first: u64,
    discretionary_swings: u64,
    discretionary_contact_first: u64,
    fair_launches: BTreeMap<&'static str, BTreeMap<&'static str, u64>>,
}

impl StrikeCounts {
    fn observe(&mut self, swing: CountIntentSwing, launch: Option<(f64, f64)>) {
        let contact_first = swing.intent == BattedBallIntent::ContactFirst;
        self.swings += 1;
        self.contact_first += u64::from(contact_first);
        self.discretionary_swings += u64::from(swing.batter_discretion);
        self.discretionary_contact_first += u64::from(swing.batter_discretion && contact_first);
        if let Some((speed, angle)) = launch {
            let speed_band = if speed < 30.0 {
                "below_30"
            } else if speed < 40.0 {
                "30_to_40"
            } else if speed < 45.0 {
                "40_to_45"
            } else if speed < 50.0 {
                "45_to_50"
            } else {
                "at_least_50"
            };
            let angle_band = if angle < 0.0 {
                "below_0"
            } else if angle < 10.0 {
                "0_to_10"
            } else if angle < 20.0 {
                "10_to_20"
            } else if angle < 40.0 {
                "20_to_40"
            } else {
                "at_least_40"
            };
            *self
                .fair_launches
                .entry(speed_band)
                .or_default()
                .entry(angle_band)
                .or_default() += 1;
        }
    }

    fn report(&self) -> Value {
        let rate = |n, total| (total > 0).then(|| n as f64 / total as f64);
        json!({
            "swings":self.swings, "contact_first_swings":self.contact_first,
            "contact_first_rate":rate(self.contact_first, self.swings),
            "discretionary_swings":self.discretionary_swings,
            "discretionary_contact_first_swings":self.discretionary_contact_first,
            "discretionary_contact_first_rate":rate(self.discretionary_contact_first, self.discretionary_swings),
            "fair_launches_by_ev_ms_and_vla_deg":self.fair_launches,
        })
    }
}

pub(super) fn run(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let mut arms = Vec::new();
    for (name, weighted) in [("current", false), ("weighted_before_two_strikes", true)] {
        let mut total = Counts::default();
        let mut pooled: [StrikeCounts; 3] = Default::default();
        let mut reports = Vec::new();
        for &seed in &args.seeds {
            let mut per_seed = Counts::default();
            let mut strikes: [StrikeCounts; 3] = Default::default();
            let mut teams = BTreeMap::new();
            let mut players = BTreeMap::new();
            for game in games {
                let game_seed = derived_seed(seed, game.id);
                let (result, swings) = with_count_intent_audit(
                    weighted.then_some(game_seed ^ 0x9426_3bbb_0bb6_90aa),
                    || {
                        simulate_game(
                            game.clone(),
                            Box::new(RealRng::from_seed(game_seed)),
                            BattingStrategySettings::player_driven(),
                            Some(args.max_pitches),
                            false,
                        )
                    },
                );
                let result = result
                    .with_context(|| format!("{name}, seed {seed}, game {} failed", game.id))?;
                accumulate(&mut per_seed, &mut teams, &mut players, game, &result)?;
                let mut recorded_swings: BTreeMap<_, _> = result
                    .player_battings
                    .iter()
                    .filter(|row| row.swing_decision.is_some_and(|decision| decision.swung))
                    .map(|row| (row.count_seq, row))
                    .collect();
                for swing in swings {
                    let launch = recorded_swings.remove(&swing.count_seq).and_then(|row| {
                        (row.ball.launch_speed > 0.0
                            && row.result != BattingResult::Foul
                            && row.ball.outbound_result != OutboundResult::Foul)
                            .then_some((row.ball.launch_speed, row.ball.launch_angle))
                    });
                    strikes[usize::from(swing.strikes)].observe(swing, launch);
                    pooled[usize::from(swing.strikes)].observe(swing, launch);
                }
                anyhow::ensure!(
                    recorded_swings.is_empty(),
                    "untracked swing in game {}",
                    game.id
                );
            }
            eprintln!(
                "Count intent {name}, seed {seed}: {} games complete",
                games.len()
            );
            reports.push(json!({
                "seed":seed.to_string(), "total":per_seed.vertical_contact_json(true),
                "by_strikes":strikes.iter().map(StrikeCounts::report).collect::<Vec<_>>(),
            }));
            total.merge(per_seed);
        }
        arms.push(json!({
            "name":name, "total":total.vertical_contact_json(true),
            "by_strikes":pooled.iter().map(StrikeCounts::report).collect::<Vec<_>>(),
            "reports":reports,
        }));
    }
    Ok(json!({
        "definition":"Full games on the same schedules, rosters and starting seeds. When the manager leaves the decision to the batter, the trial draws intent at zero or one strike from the existing situational weights after multiplying the LineDrive weight by 1.5. Two-strike selection and explicit manager instructions use the current rule. Intent draws have a separate game-local random stream and apply after the swing decision. Later pitches can diverge. by_strikes is indexed by the pre-pitch strike count; EV/VLA bins include recorded non-foul launches and use inclusive lower bounds and exclusive upper bounds.",
        "arms":arms,
    }))
}
