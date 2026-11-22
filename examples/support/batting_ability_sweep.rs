use super::{Args, Counts, GameSchedule, bip_audit::band, derived_seed, simulate_game};
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
    BattingEye(f64),
}

impl Trial {
    fn label(self) -> &'static str {
        match self {
            Self::Baseline => "baseline",
            Self::BatControl(_) => "bat_control",
            Self::SwingSpeed(_) => "swing_speed_ms",
            Self::BattingEye(_) => "batting_eye",
        }
    }

    fn apply(self, batter: &mut BatterInfo) {
        match self {
            Self::Baseline => {}
            Self::BatControl(delta) => batter.bat_control += delta,
            Self::SwingSpeed(delta) => batter.swing_speed += delta,
            Self::BattingEye(delta) => batter.batting_eye += delta,
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

#[derive(Default, serde::Serialize)]
struct LaunchProfile {
    measured_fair_contacts: u64,
    missing_fair_launch_measurements: u64,
    exit_speed_bands_ms: BTreeMap<String, u64>,
    launch_angle_bands_deg: BTreeMap<String, u64>,
}

impl LaunchProfile {
    fn observe(&mut self, ev: f64, angle: f64) {
        if !ev.is_finite() || ev <= 0.0 || !angle.is_finite() {
            self.missing_fair_launch_measurements += 1;
            return;
        }
        self.measured_fair_contacts += 1;
        *self
            .exit_speed_bands_ms
            .entry(band(ev, &[30.0, 40.0, 45.0, 50.0]))
            .or_default() += 1;
        *self
            .launch_angle_bands_deg
            .entry(band(angle, &[0.0, 7.0, 10.0, 25.0, 50.0]))
            .or_default() += 1;
    }
}

// Keep these edges aligned with the saved-game BABIP contact-mix diagnostic.
const MIX_EV_EDGES: [f64; 5] = [30.0, 40.0, 45.0, 47.5, 50.0];
const MIX_VLA_EDGES: [f64; 8] = [-10.0, 0.0, 5.0, 7.0, 10.0, 15.0, 25.0, 50.0];
const MIN_REFERENCE_BALLS: u64 = 20;
type ContactBand = (usize, usize);
type Reference = BTreeMap<ContactBand, (u64, u64)>;

fn contact_band(ev: f64, vla: f64) -> Option<ContactBand> {
    (ev.is_finite() && ev > 0.0 && vla.is_finite()).then(|| {
        (
            MIX_EV_EDGES.partition_point(|&edge| ev >= edge),
            MIX_VLA_EDGES.partition_point(|&edge| vla >= edge),
        )
    })
}

fn non_hr_ball_in_play(result: BattingResult) -> bool {
    use BattingResult::*;
    matches!(
        result,
        Single | Double | Triple | Out | FieldersChoice | DoublePlay
    )
}

#[derive(Default)]
struct ContactMix {
    non_hr_bip: BTreeMap<ContactBand, u64>,
    measured_fair_contacts: u64,
    power_window_contacts: u64,
}

impl ContactMix {
    fn observe(&mut self, ev: f64, vla: f64, ball_in_play: bool) {
        let Some(cell) = contact_band(ev, vla) else {
            return;
        };
        self.measured_fair_contacts += 1;
        self.power_window_contacts += u64::from(ev >= 40.0 && (20.0..50.0).contains(&vla));
        if ball_in_play {
            *self.non_hr_bip.entry(cell).or_default() += 1;
        }
    }

    fn report(&self, reference: &Reference, counts: &Counts) -> Value {
        let mut covered = 0;
        let mut expected_hits = 0.0;
        let bands: Vec<_> = self
            .non_hr_bip
            .iter()
            .map(|(&cell, &balls)| {
                let (reference_balls, reference_hits) =
                    reference.get(&cell).copied().unwrap_or_default();
                let reference_rate = (reference_balls >= MIN_REFERENCE_BALLS)
                    .then(|| reference_hits as f64 / reference_balls as f64);
                if let Some(rate) = reference_rate {
                    covered += balls;
                    expected_hits += balls as f64 * rate;
                }
                json!({
                    "ev_band": cell.0, "vla_band": cell.1, "balls": balls,
                    "reference_balls": reference_balls, "reference_hit_rate": reference_rate,
                })
            })
            .collect();
        let bip = counts.ab - counts.strikeouts - counts.hr;
        json!({
            "measured_non_hr_balls_in_play": self.non_hr_bip.values().sum::<u64>(),
            "scored_balls_in_play": covered,
            "share_of_all_balls_in_play_scored": (bip > 0).then(|| covered as f64 / bip as f64),
            "expected_babip_at_fixed_reference": (covered > 0).then(|| expected_hits / covered as f64),
            "measured_fair_contacts": self.measured_fair_contacts,
            "power_window_contacts": self.power_window_contacts,
            "power_window_per_pa": (counts.pa > 0)
                .then(|| self.power_window_contacts as f64 / counts.pa as f64),
            "power_window_share_of_measured_fair": (self.measured_fair_contacts > 0)
                .then(|| self.power_window_contacts as f64 / self.measured_fair_contacts as f64),
            "non_hr_bip_bands": bands,
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
    let mut trials = vec![Trial::Baseline];
    if args.batting_eye_only {
        trials.extend(STEPS.map(Trial::BattingEye));
    } else {
        trials.extend(STEPS.map(Trial::BatControl));
        trials.extend(STEPS.map(Trial::SwingSpeed));
    }
    let mut reference = Reference::new();
    for trial in trials {
        let mut skill = baseline;
        trial.apply(&mut skill);
        ensure!(
            skill.swing_speed.is_finite()
                && skill.swing_speed > 0.0
                && skill.bat_control.is_finite()
                && (!args.batting_eye_only || skill.batting_eye.is_finite()),
            "invalid batter ability in {}",
            trial.label()
        );
        let mut total = BattingLine::default();
        let mut by_pitcher = BTreeMap::<i64, BattingLine>::new();
        let mut games_with_pa = 0;
        let mut launch_profile = LaunchProfile::default();
        let mut contact_mix = ContactMix::default();
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
                for row in &result.player_battings {
                    let ball_in_play = non_hr_ball_in_play(row.result);
                    if row.batter_id != batter_id {
                        if matches!(trial, Trial::Baseline)
                            && ball_in_play
                            && let Some(cell) =
                                contact_band(row.ball.launch_speed, row.ball.launch_angle)
                        {
                            let (balls, hits) = reference.entry(cell).or_default();
                            *balls += 1;
                            *hits += u64::from(matches!(
                                row.result,
                                BattingResult::Single
                                    | BattingResult::Double
                                    | BattingResult::Triple
                            ));
                        }
                        continue;
                    }
                    if ball_in_play || row.result == BattingResult::HomeRun {
                        contact_mix.observe(
                            row.ball.launch_speed,
                            row.ball.launch_angle,
                            ball_in_play,
                        );
                    }
                    let hard_low = ball_in_play
                        && row.ball.launch_speed >= 45.0
                        && (7.0..10.0).contains(&row.ball.launch_angle);
                    if args.batting_eye_only
                        && (ball_in_play || row.result == BattingResult::HomeRun)
                    {
                        launch_profile.observe(row.ball.launch_speed, row.ball.launch_angle);
                    }
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
            "Batting ability {}, eye {:.2}, control {:.2}, speed {:.2}: {games_with_pa} games with target PA",
            trial.label(),
            skill.batting_eye,
            skill.bat_control,
            skill.swing_speed
        );
        let mut report = json!({
            "changed_ability": trial.label(), "bat_control": skill.bat_control,
            "swing_speed_ms": skill.swing_speed, "games_with_target_pa": games_with_pa,
            "total": total.report(),
            "contact_mix": contact_mix.report(&reference, &total.counts),
            "by_pitcher": by_pitcher.iter().filter(|(_, line)| line.counts.pa > 0)
                .map(|(id, line)| (id, line.report())).collect::<BTreeMap<_, _>>(),
        });
        if args.batting_eye_only {
            report["batting_eye"] = json!(skill.batting_eye);
            report["walks"] = json!(total.counts.walks);
            report["walks_per_pa"] = json!(
                (total.counts.pa > 0).then(|| total.counts.walks as f64 / total.counts.pa as f64)
            );
            report["launch_profile"] = json!(launch_profile);
        }
        reports.push(report);
    }
    ensure!(
        reports[0]["total"]["pa"].as_u64().is_some_and(|pa| pa > 0),
        "target batter had no plate appearances in the baseline schedule"
    );
    Ok(json!({
        "batter_id": batter_id,
        "definition": "Change one ability on a single roster batter per trial. Reuse the scheduled opponents, parks and per-game starting seeds. Games use the independent-game diagnostic without season fatigue or in-game pitching changes. Swing-speed changes may also change the batting order. Later pitch choices and random draws can diverge after a changed play; by_pitcher reports realized opposition and sample sizes. PA, AB and BABIP follow the standard offense replay definitions. Hard low contacts are non-HR balls in play at EV >=45 m/s and VLA [7,10). The optional launch profile includes measured balls in play and home runs.",
        "contact_mix_reference": {
            "definition": "Use only other batters’ measured non-HR balls in play in the baseline replay of these same games. Keep their EV/VLA hit rates fixed for every trial, including baseline. Expected BABIP excludes unmeasured or undersupported bands; use reported coverage when comparing arms. Band indices are zero-based counts of lower edges met. The power window is a measured fair contact with EV >=40 m/s and VLA [20,50), including home runs. It is an opportunity count, independent of whether the ball clears a fence.",
            "exit_speed_edges_ms": MIX_EV_EDGES, "launch_angle_edges_deg": MIX_VLA_EDGES,
            "min_reference_balls_per_band": MIN_REFERENCE_BALLS,
            "reference_balls_in_play": reference.values().map(|(balls, _)| balls).sum::<u64>(),
        },
        "trials": reports,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_reference_scores_only_supported_bip_and_power_can_include_homers() {
        let mut mix = ContactMix::default();
        mix.observe(45.0, 7.0, true);
        mix.observe(45.0, 7.0, true);
        mix.observe(40.0, 20.0, true);
        mix.observe(40.0, 20.0, false);
        mix.observe(0.0, 7.0, true);
        let reference = Reference::from([((3, 4), (20, 15)), ((2, 6), (19, 19))]);
        let counts = Counts {
            ab: 5,
            pa: 5,
            hr: 1,
            ..Counts::default()
        };
        let row = mix.report(&reference, &counts);
        assert_eq!(row["expected_babip_at_fixed_reference"], 0.75);
        assert_eq!(row["share_of_all_balls_in_play_scored"], 0.5);
        assert_eq!(row["measured_non_hr_balls_in_play"], 3);
        assert_eq!(row["power_window_contacts"], 2);
        assert!(
            mix.report(&Reference::new(), &counts)["expected_babip_at_fixed_reference"].is_null()
        );
    }
}
