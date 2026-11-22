use super::{Args, Counts, GameSchedule, derived_seed, simulate_game};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    random_provider::RealRng,
    shared::{game::BattingResult, player::BatterInfo},
    strategy::batting_strategy::BattingStrategySettings,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const STEPS: [f64; 4] = [-2.0, -1.0, 1.0, 2.0];

#[derive(Clone, Copy)]
enum Trial {
    Baseline,
    BatControl(f64),
    SwingSpeed(f64),
}

impl Trial {
    fn label(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::BatControl(_) => "bat_control",
            Self::SwingSpeed(_) => "swing_speed_ms",
        }
    }

    fn apply(self, batter: &mut BatterInfo) {
        match self {
            Self::Baseline => {}
            Self::BatControl(delta) => batter.bat_control += delta,
            Self::SwingSpeed(delta) => batter.swing_speed += delta,
        }
    }
}

#[derive(Default)]
struct BattingLine {
    counts: Counts,
    hard_low_contacts: u64,
    hard_low_singles: u64,
}

impl BattingLine {
    fn report(&self) -> Value {
        let counts = &self.counts;
        let rate = |n, d| (d > 0).then(|| n as f64 / d as f64);
        json!({
            "pa": counts.pa, "ab": counts.ab,
            "ba": rate(counts.single + counts.double + counts.triple + counts.hr, counts.ab),
            "strikeouts": counts.strikeouts, "strikeouts_per_ab": rate(counts.strikeouts, counts.ab),
            "balls_in_play": counts.ab - counts.strikeouts - counts.hr,
            "babip": rate(counts.single + counts.double + counts.triple, counts.ab - counts.strikeouts - counts.hr),
            "singles": counts.single, "doubles": counts.double, "triples": counts.triple,
            "hr": counts.hr, "hr_per_pa": rate(counts.hr, counts.pa),
            "hard_low_contacts": self.hard_low_contacts, "hard_low_singles": self.hard_low_singles,
        })
    }
}

pub(super) fn run(args: &Args, games: &[GameSchedule], batter_id: i64) -> Result<Value> {
    let baseline = games
        .iter()
        .flat_map(|game| game.away_team.players.iter().chain(&game.home_team.players))
        .find(|player| player.info.id == batter_id)
        .with_context(|| format!("batter {batter_id} is not on the scheduled roster"))?
        .batter()?;
    let mut reports = Vec::new();
    let trials = std::iter::once(Trial::Baseline)
        .chain(STEPS.map(Trial::BatControl))
        .chain(STEPS.map(Trial::SwingSpeed));
    for trial in trials {
        let mut skill = baseline;
        trial.apply(&mut skill);
        ensure!(
            skill.swing_speed.is_finite()
                && skill.swing_speed > 0.0
                && skill.bat_control.is_finite(),
            "invalid batter ability in {}",
            trial.label()
        );
        let mut total = BattingLine::default();
        let mut by_pitcher = BTreeMap::<i64, BattingLine>::new();
        let mut games_with_pa = 0;
        for &seed in &args.seeds {
            for original in games {
                let mut game = original.clone();
                let player = game
                    .away_team
                    .players
                    .iter_mut()
                    .chain(&mut game.home_team.players)
                    .find(|player| player.info.id == batter_id)
                    .context("target batter disappeared from a scheduled roster")?;
                player.offense_skills.batter = Some(skill);
                let game_id = game.id;
                let result = simulate_game(
                    game,
                    Box::new(RealRng::from_seed(derived_seed(seed, game_id))),
                    BattingStrategySettings::player_driven(),
                    Some(args.max_pitches),
                    false,
                )
                .with_context(|| {
                    format!("{} failed in game {game_id}, seed {seed}", trial.label())
                })?;
                let before = total.counts.pa;
                for row in result
                    .player_battings
                    .iter()
                    .filter(|row| row.batter_id == batter_id)
                {
                    let hard_low = matches!(
                        row.result,
                        BattingResult::Single
                            | BattingResult::Double
                            | BattingResult::Triple
                            | BattingResult::Out
                            | BattingResult::FieldersChoice
                            | BattingResult::DoublePlay
                    ) && row.ball.launch_speed >= 45.0
                        && (7.0..10.0).contains(&row.ball.launch_angle);
                    for line in [&mut total, by_pitcher.entry(row.pitcher_id).or_default()] {
                        line.counts.batting(row.result);
                        line.hard_low_contacts += u64::from(hard_low);
                        line.hard_low_singles +=
                            u64::from(hard_low && row.result == BattingResult::Single);
                    }
                }
                games_with_pa += u64::from(before != total.counts.pa);
            }
        }
        eprintln!(
            "Batting ability {}, control {:.2}, speed {:.2}: {games_with_pa} games with target PA",
            trial.label(),
            skill.bat_control,
            skill.swing_speed
        );
        reports.push(json!({
            "changed_ability": trial.label(), "bat_control": skill.bat_control,
            "swing_speed_ms": skill.swing_speed, "games_with_target_pa": games_with_pa,
            "total": total.report(),
            "by_pitcher": by_pitcher.iter().filter(|(_, line)| line.counts.pa > 0)
                .map(|(id, line)| (id, line.report())).collect::<BTreeMap<_, _>>(),
        }));
    }
    ensure!(
        reports[0]["total"]["pa"].as_u64().is_some_and(|pa| pa > 0),
        "target batter had no plate appearances in the baseline schedule"
    );
    Ok(json!({
        "batter_id": batter_id,
        "definition": "Change one ability on a single roster batter per trial. Reuse the scheduled opponents, parks and per-game starting seeds. Games use the independent-game diagnostic without season fatigue or in-game pitching changes. Swing-speed changes may also change the batting order. Later pitch choices and random draws can diverge after a changed play; by_pitcher reports realized opposition and sample sizes. PA, AB and BABIP follow the standard offense replay definitions. Hard low contacts are non-HR balls in play at EV >=45 m/s and VLA [7,10).",
        "trials": reports,
    }))
}
