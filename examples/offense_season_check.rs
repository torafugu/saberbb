//! Full-game seeded offense diagnostics; reads SABERBB_DB and never saves game results.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use saberbb::{
    I18nManager,
    domain::{
        game_service::simulate_game,
        random_provider::RealRng,
        resolver::batting_resolver::{CONTACT_TIMING_POSITION_SCALE, with_contact_geometry_scale},
        resolver::fielding_physics::{CatchType, with_direct_catch_margin_scale},
        seeded_diagnostic::derived_seed,
        shared::{
            game::{BattingResult, GameResult, GameSchedule},
            game_state::with_right_field_first_trial,
            infield_opportunity_audit::InfieldOpportunityAudit,
            no_bounce_catch_audit::NoBounceCatchAudit,
            team::Team,
            timing_scale_audit::TimingScaleAudit,
        },
        strategy::batting_strategy::BattingStrategySettings,
    },
    repositories::{
        batting_personnel_repository::load_team_manager,
        db::FromRow,
        game_repository::{GamePlayerReader, SqlGameRepository},
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};

#[path = "support/bip_audit.rs"]
mod bip_audit;
#[path = "support/contact_direction_audit.rs"]
mod contact_direction_audit;
#[path = "support/moving_collision_audit.rs"]
mod moving_collision_audit;
#[path = "support/pickup_timing_audit.rs"]
mod pickup_timing_audit;
#[path = "support/selected_intent_audit.rs"]
mod selected_intent_audit;
#[path = "support/throw_origin_audit.rs"]
mod throw_origin_audit;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    season: u16,
    /// Historical drag control; applies to gameplay and shadow flights on this thread.
    #[arg(long)]
    legacy_flight_drag: bool,
    /// First N scheduled games, regardless of saved completion; omit for the full season.
    #[arg(long)]
    games: Option<usize>,
    #[arg(long, value_delimiter = ',', default_value = "7,42,20261108")]
    seeds: Vec<u64>,
    #[arg(long, default_value_t = 5000)]
    max_pitches: u32,
    /// Include BIP quality, fielding/running timing groups and bounded example plays.
    #[arg(long)]
    bip_diagnostics: bool,
    /// Replay frozen contacts with individual HLA components removed; no game changes.
    #[arg(long)]
    contact_direction_diagnostics: bool,
    /// Compare finite collision responses at frozen legacy contact coordinates.
    #[arg(long)]
    moving_collision_diagnostics: bool,
    /// Collect per-game shadow states in memory and audit only the selected Intent 3-D path.
    #[arg(long)]
    selected_intent_diagnostics: bool,
    /// Compare timing-to-position scales on the same realized swings.
    #[arg(long)]
    timing_scale_diagnostics: bool,
    /// Run each scheduled game at geometry scales 0.75, 0.875 and 1.00.
    #[arg(long, conflicts_with = "timing_scale_diagnostics")]
    timing_geometry_sweep: bool,
    /// Compare baseline games with the experimental right-field force play at first.
    #[arg(long, conflicts_with = "timing_geometry_sweep")]
    right_field_first_comparison: bool,
    /// Audit earlier infield interception opportunities on outfield pickup singles.
    #[arg(long)]
    infield_opportunity_diagnostics: bool,
    /// Replay baseline singles at 100%, 50%, and 0% of the direct-catch margin.
    #[arg(long)]
    no_bounce_catch_diagnostics: bool,
    /// Simulate full games with 100% and 50% of the normal direct-catch margin.
    #[arg(long, conflicts_with_all = ["timing_geometry_sweep", "right_field_first_comparison", "no_bounce_catch_diagnostics"])]
    no_bounce_margin_comparison: bool,
}
#[derive(Default, Serialize)]
struct Counts {
    games: u64,
    pitches: u64,
    pa: u64,
    ab: u64,
    single: u64,
    double: u64,
    triple: u64,
    hr: u64,
    strikeouts: u64,
    walks: u64,
    hit_by_pitch: u64,
    runs: u64,
    runs_allowed: u64,
    pitching_outs: u64,
    non_strikeout_outs: u64,
}
impl Counts {
    fn batting(&mut self, result: BattingResult) {
        use BattingResult::*;
        self.pitches += 1;
        match result {
            Single => self.single += 1,
            Double => self.double += 1,
            Triple => self.triple += 1,
            HomeRun => self.hr += 1,
            Strikeout => self.strikeouts += 1,
            Walk => self.walks += 1,
            HitByPitch => self.hit_by_pitch += 1,
            _ => {}
        }
        if matches!(
            result,
            Single | Double | Triple | HomeRun | Strikeout | Out | FieldersChoice | DoublePlay
        ) {
            self.ab += 1;
            self.pa += 1;
        }
        if matches!(result, Walk | HitByPitch) {
            self.pa += 1;
        }
    }
    fn json(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap();
        v["ba"] = if self.ab > 0 {
            serde_json::json!(
                (self.single + self.double + self.triple + self.hr) as f64 / self.ab as f64
            )
        } else {
            serde_json::Value::Null
        };
        v["hr_per_pa"] = if self.pa > 0 {
            serde_json::json!(self.hr as f64 / self.pa as f64)
        } else {
            serde_json::Value::Null
        };
        let in_play = self.ab - self.strikeouts - self.hr;
        v["babip_balls_in_play"] = serde_json::json!(in_play);
        v["babip"] = serde_json::json!(
            (in_play > 0)
                .then(|| (self.single + self.double + self.triple) as f64 / in_play as f64)
        );
        v["strikeouts_per_pa"] =
            serde_json::json!((self.pa > 0).then(|| self.strikeouts as f64 / self.pa as f64));
        v["singles_per_pa"] =
            serde_json::json!((self.pa > 0).then(|| self.single as f64 / self.pa as f64));
        v["ra9_reference"] = if self.pitching_outs > 0 {
            serde_json::json!(self.runs_allowed as f64 * 27.0 / self.pitching_outs as f64)
        } else {
            serde_json::Value::Null
        };
        v
    }
}
#[derive(Default, Serialize)]
struct FieldingCounts {
    fielded_balls: u64,
    direct_catches: u64,
    candidate_attempts: u64,
    candidate_errors: u64,
    direct_candidate_attempts: u64,
    direct_candidate_errors: u64,
}

impl FieldingCounts {
    fn observe(&mut self, result: &GameResult) {
        for fielding in &result.player_fieldings {
            if let Some(catch) = fielding.catch_diagnostics {
                self.fielded_balls += 1;
                if catch.catch_type == CatchType::DirectFly && catch.error_type.is_none() {
                    self.direct_catches += 1;
                }
            }
        }
        for candidate in &result.player_fielding_candidates {
            let d = candidate.diagnostics;
            if d.interception_catch_type.is_none() {
                continue;
            }
            self.candidate_attempts += 1;
            let error = u64::from(d.interception_error_type.is_some());
            self.candidate_errors += error;
            if d.search.is_some_and(|s| s.risk_ok_samples > 0) {
                self.direct_candidate_attempts += 1;
                self.direct_candidate_errors += error;
            }
        }
    }
}

fn schedules(
    conn: &Connection,
    repo: &SqlGameRepository,
    args: &Args,
) -> Result<Vec<GameSchedule>> {
    let mut query=conn.prepare("SELECT g.id,g.season,g.round_seq,g.seq,g.planned_date,g.away_team_id,g.home_team_id,
        a.name AS away_team_name,h.name AS home_team_name,g.game_type,
        st.id AS stadium_id,st.name AS stadium_name,st.foul_pole_distance AS stadium_foul_pole_distance,
        st.center_fence_distance AS stadium_center_fence_distance,st.fence_height AS stadium_fence_height,
        COALESCE(l.max_inning,12) AS max_inning,COALESCE(l.base_four_seam_speed,40.833) AS base_four_seam_speed
        FROM game g JOIN team a ON a.id=g.away_team_id JOIN team h ON h.id=g.home_team_id
        JOIN stadium st ON st.id=g.stadium_id JOIN league l ON l.id=a.league_id
        WHERE g.season=?1 ORDER BY g.round_seq,g.seq,g.id LIMIT ?2")?;
    let mut rows = query.query(rusqlite::params![
        args.season,
        args.games.map(|v| v as i64).unwrap_or(-1)
    ])?;
    let mut teams = BTreeMap::<u16, Team>::new();
    let mut games = Vec::new();
    while let Some(row) = rows.next()? {
        let mut game = GameSchedule::from_row(row)?;
        for team in [&mut game.away_team, &mut game.home_team] {
            if !teams.contains_key(&team.id) {
                team.players = repo.load_team_players(team.id)?;
                team.manager = load_team_manager(conn, team.id)?;
                teams.insert(team.id, team.clone());
            }
            *team = teams[&team.id].clone();
        }
        games.push(game);
    }
    ensure!(!games.is_empty(), "no schedule rows for requested season");
    if let Some(n) = args.games {
        ensure!(
            games.len() == n,
            "requested {n} games but only {} are available",
            games.len()
        );
    }
    Ok(games)
}
fn accumulate(
    total: &mut Counts,
    teams: &mut BTreeMap<u16, Counts>,
    players: &mut BTreeMap<i64, Counts>,
    game: &GameSchedule,
    result: &GameResult,
) -> Result<()> {
    total.games += 1;
    total.runs += u64::from(result.away_total_point) + u64::from(result.home_total_point);
    total.runs_allowed = total.runs;
    let roster: BTreeMap<_, _> = game
        .away_team
        .players
        .iter()
        .map(|p| (p.info.id, game.away_team.id))
        .chain(
            game.home_team
                .players
                .iter()
                .map(|p| (p.info.id, game.home_team.id)),
        )
        .collect();
    for (id, scored, allowed) in [
        (
            game.away_team.id,
            result.away_total_point,
            result.home_total_point,
        ),
        (
            game.home_team.id,
            result.home_total_point,
            result.away_total_point,
        ),
    ] {
        let t = teams.entry(id).or_default();
        t.games += 1;
        t.runs += u64::from(scored);
        t.runs_allowed += u64::from(allowed);
    }
    let mut appearing = std::collections::BTreeSet::new();
    for b in &result.player_battings {
        if appearing.insert(b.batter_id) {
            players.entry(b.batter_id).or_default().games += 1;
        }
        total.batting(b.result);
        teams
            .entry(roster[&b.batter_id])
            .or_default()
            .batting(b.result);
        players.entry(b.batter_id).or_default().batting(b.result);
    }
    for inning in &result.innings {
        let defense = if inning.tb == saberbb::domain::shared::game::TB::Top {
            game.home_team.id
        } else {
            game.away_team.id
        };
        let outs = u64::from(
            inning
                .counts
                .iter()
                .map(|c| c.out)
                .max()
                .unwrap_or(0)
                .min(3),
        );
        total.pitching_outs += outs;
        teams.entry(defense).or_default().pitching_outs += outs;
    }
    ensure!(
        total.pitching_outs >= total.strikeouts,
        "recorded outs less than strikeouts"
    );
    total.non_strikeout_outs = total.pitching_outs - total.strikeouts;
    // Team strikeouts above are batting SO; compute defensive non-K outs from actual pitcher records.
    for id in [game.away_team.id, game.home_team.id] {
        let defensive_so = result
            .player_battings
            .iter()
            .filter(|b| b.result == BattingResult::Strikeout && roster[&b.batter_id] != id)
            .count() as u64;
        let team = teams.get_mut(&id).unwrap();
        let game_outs = result
            .innings
            .iter()
            .filter(|i| {
                (i.tb == saberbb::domain::shared::game::TB::Top) == (id == game.home_team.id)
            })
            .map(|i| u64::from(i.counts.iter().map(|c| c.out).max().unwrap_or(0).min(3)))
            .sum::<u64>();
        ensure!(
            game_outs >= defensive_so,
            "invalid defensive strikeout count"
        );
        team.non_strikeout_outs += game_outs - defensive_so;
    }
    Ok(())
}
fn main() -> Result<()> {
    let args = Args::parse();
    use saberbb::domain::shared::flight_drag::{FlightDragModel, with_model};
    let model = if args.legacy_flight_drag {
        FlightDragModel::Legacy
    } else {
        FlightDragModel::Published2017
    };
    with_model(model, || run(args))
}
fn run(args: Args) -> Result<()> {
    ensure!(
        args.games != Some(0) && !args.seeds.is_empty(),
        "games/seeds must be nonempty"
    );
    ensure!(
        args.seeds
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == args.seeds.len(),
        "duplicate master seeds"
    );
    ensure!(
        args.max_pitches > 0 && args.max_pitches < 65535,
        "invalid pitch budget"
    );
    let path = std::env::var("SABERBB_DB").context("SABERBB_DB is not set")?;
    ensure!(!path.trim().is_empty(), "SABERBB_DB is blank");
    let path = PathBuf::from(path);
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute_batch("PRAGMA query_only=ON;BEGIN;")?;
    let repo = SqlGameRepository::read_only(&path)?;
    let games = schedules(&conn, &repo, &args)?;
    let input_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&games)?));
    I18nManager::init("en-US");
    let mut output = serde_json::json!({
        "engine_source_sha256": saberbb::domain::seeded_diagnostic::ENGINE,
        "flight_drag_model": saberbb::domain::shared::flight_drag::current_model(),
        "flight_drag_settings": {
            "density_kg_m3": saberbb::domain::shared::flight_drag::DENSITY_KG_M3,
            "mass_kg": saberbb::domain::shared::flight_drag::MASS_KG,
            "radius_m": saberbb::domain::shared::flight_drag::RADIUS_M,
            "published_cd": "0.297 + 0.0292 * total_spin_rpm / 1000",
            "lift": "unchanged legacy transverse-spin model"
        },
        "inputs_sha256": input_hash, "season": args.season, "games_per_seed": games.len(),
        "definitions": {
            "ra9_reference": "Runs allowed * 27 / recorded outs; earned runs not recorded",
            "non_strikeout_outs": "All recorded outs minus defensive strikeouts, including baserunning outs",
            "rosters": "Current source rosters, freshly initialized per game; historical lineups/seeds are not reproduced",
            "babip": "(1B + 2B + 3B) / (AB - K - HR); sacrifice flies have no separate result category"
        }
    });
    if args.no_bounce_margin_comparison {
        let mut trials = Vec::new();
        for percent in [100, 50] {
            eprintln!("Direct-catch margin: {percent}%");
            let reports = with_direct_catch_margin_scale(f64::from(percent) / 100.0, || {
                simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, false)
            })?;
            trials.push(serde_json::json!({"margin_percent": percent, "reports": reports}));
        }
        output["no_bounce_margin_trials"] = serde_json::json!(trials);
        output["definitions"]["candidate_errors"] = serde_json::json!(
            "Modeled interception errors across all evaluated fielder attempts; multiple candidates can be counted for one batted ball. Not official scoring errors."
        );
    } else if args.right_field_first_comparison {
        let mut trials = Vec::new();
        for enabled in [false, true] {
            trials.push(serde_json::json!({
                "enabled": enabled,
                "reports": simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, enabled)?,
            }));
        }
        output["right_field_first_trials"] = serde_json::json!(trials);
    } else if args.timing_geometry_sweep {
        let mut trials = Vec::new();
        for scale in [CONTACT_TIMING_POSITION_SCALE, 0.875, 1.0] {
            let reports = with_contact_geometry_scale(scale, || {
                simulate_reports(&args, &games, scale, false)
            })?;
            trials.push(serde_json::json!({
                "geometry_scale": scale,
                "exit_speed_depth_scale": CONTACT_TIMING_POSITION_SCALE,
                "reports": reports,
            }));
        }
        output["timing_geometry_trials"] = serde_json::json!(trials);
    } else {
        output["reports"] = serde_json::json!(simulate_reports(
            &args,
            &games,
            CONTACT_TIMING_POSITION_SCALE,
            false,
        )?);
    }
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn simulate_reports(
    args: &Args,
    games: &[GameSchedule],
    scale: f64,
    right_field_trial: bool,
) -> Result<Vec<serde_json::Value>> {
    let mut reports = Vec::new();
    for seed in &args.seeds {
        let mut bip = args.bip_diagnostics.then(bip_audit::BipAudit::default);
        let mut direction = args
            .contact_direction_diagnostics
            .then(contact_direction_audit::DirectionAudit::default);
        let mut throws = args
            .bip_diagnostics
            .then(throw_origin_audit::ThrowOriginAudit::default);
        let mut moving = args
            .moving_collision_diagnostics
            .then(moving_collision_audit::MovingAudit::default);
        let mut selected = args
            .selected_intent_diagnostics
            .then(selected_intent_audit::SelectedAudit::default);
        let mut timing = args
            .timing_scale_diagnostics
            .then(TimingScaleAudit::default);
        let mut infield = args
            .infield_opportunity_diagnostics
            .then(InfieldOpportunityAudit::default);
        let mut no_bounce = args
            .no_bounce_catch_diagnostics
            .then(NoBounceCatchAudit::default);
        let mut fielding = args
            .no_bounce_margin_comparison
            .then(FieldingCounts::default);
        let mut total = Counts::default();
        let mut right_field_first_outs = 0;
        let mut teams = BTreeMap::new();
        let mut players = BTreeMap::new();
        for (index, game) in games.iter().enumerate() {
            let game_seed = derived_seed(*seed, game.id);
            let run = || {
                simulate_game(
                    game.clone(),
                    Box::new(RealRng::from_seed(game_seed)),
                    BattingStrategySettings::player_driven(),
                    Some(args.max_pitches),
                    args.selected_intent_diagnostics,
                )
            };
            let mut run_with_diagnostics = || {
                let mut run = || match no_bounce.as_mut() {
                    Some(audit) => audit.observe_game(game_seed, run),
                    None => run(),
                };
                let run = || match infield.as_mut() {
                    Some(audit) => audit.observe_game(run),
                    None => run(),
                };
                if let Some(timing) = timing.as_mut() {
                    timing.observe_game(game_seed, run)
                } else {
                    run()
                }
            };
            let result = if right_field_trial {
                let (result, outs) = with_right_field_first_trial(run_with_diagnostics);
                right_field_first_outs += outs;
                result
            } else {
                run_with_diagnostics()
            }
            .with_context(|| {
                format!(
                    "geometry scale {scale}, right-field trial {right_field_trial}, seed {seed}, game {} failed; no partial summary accepted",
                    game.id
                )
            })?;
            if let Some(direction) = direction.as_mut() {
                direction.observe(game, &result)?;
            }
            if let Some(throws) = throws.as_mut() {
                throws.observe(game, &result);
            }
            if let Some(moving) = moving.as_mut() {
                moving.observe(game, &result)?;
            }
            if let Some(selected) = selected.as_mut() {
                selected.observe(game, &result)?;
            }
            if let Some(fielding) = fielding.as_mut() {
                fielding.observe(&result);
            }
            accumulate(&mut total, &mut teams, &mut players, game, &result)?;
            if let Some(bip) = bip.as_mut() {
                bip.observe(game, &result);
            }
            if (index + 1) % 12 == 0 || index + 1 == games.len() {
                eprintln!(
                    "geometry scale {scale}, right-field trial {right_field_trial}, seed {seed}: {}/{} games complete",
                    index + 1,
                    games.len()
                );
            }
        }
        let mut leaders: Vec<_> = players.into_iter().collect();
        leaders.sort_by(|a, b| b.1.hr.cmp(&a.1.hr).then_with(|| a.0.cmp(&b.0)));
        let mut report = serde_json::json!({"seed":seed.to_string(),"total":total.json(),
            "teams":teams.into_iter().map(|(id,t)|(id.to_string(),t.json())).collect::<BTreeMap<_,_>>(),
            "hr_leaders":leaders.iter().take(10).map(|(id,s)|serde_json::json!({"batter_id":id,"stats":s.json()})).collect::<Vec<_>>()});
        if let Some(selected) = selected {
            report["selected_intent_diagnostics"] = selected.json();
        }
        if let Some(moving) = moving {
            report["moving_collision_diagnostics"] = moving.json();
        }
        if let Some(direction) = direction {
            report["contact_direction_diagnostics"] = direction.json();
        }
        if let Some(bip) = bip {
            report["bip_diagnostics"] = bip.json();
            report["bip_diagnostics"]["throw_origin_counterfactual"] = throws.unwrap().json();
        }
        if let Some(timing) = timing {
            report["timing_scale_diagnostics"] = timing.json();
        }
        if let Some(fielding) = fielding {
            report["fielding"] = serde_json::to_value(fielding)?;
            report["total"]["extra_base_hits_per_pa"] = serde_json::json!(
                (total.pa > 0)
                    .then(|| (total.double + total.triple + total.hr) as f64 / total.pa as f64)
            );
        }
        if let Some(no_bounce) = no_bounce {
            ensure!(
                no_bounce.baseline_singles() == total.single,
                "no-bounce audit single count does not match the official batting results"
            );
            report["no_bounce_catch_diagnostics"] = serde_json::to_value(no_bounce)?;
        }
        if let Some(infield) = infield {
            report["infield_opportunity_diagnostics"] = serde_json::to_value(infield)?;
        }
        if right_field_trial {
            report["right_field_first_outs"] = serde_json::json!(right_field_first_outs);
        }
        reports.push(report);
    }
    Ok(reports)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_margin_override_restores_original_thresholds() {
        use saberbb::domain::resolver::fielding_physics::FielderRiskTolerance::*;
        assert_eq!(Balanced.required_margin_sec(), 0.15);
        with_direct_catch_margin_scale(0.5, || {
            assert_eq!(Aggressive.required_margin_sec(), 0.0);
            assert_eq!(Balanced.required_margin_sec(), 0.075);
            assert_eq!(Conservative.required_margin_sec(), 0.175);
        });
        assert_eq!(Balanced.required_margin_sec(), 0.15);
    }

    #[test]
    fn nonterminal_pitches_are_not_plate_appearances_or_at_bats() {
        let mut c = Counts::default();
        for r in [
            BattingResult::Take,
            BattingResult::Foul,
            BattingResult::StrikeSwung,
            BattingResult::HomeRun,
            BattingResult::Walk,
            BattingResult::Strikeout,
        ] {
            c.batting(r);
        }
        assert_eq!((c.pitches, c.pa, c.ab, c.hr, c.strikeouts), (6, 3, 2, 1, 1));
        assert_eq!(c.json()["ba"], 0.5);
    }
    #[test]
    fn batting_rates_use_their_own_denominators() {
        let empty = Counts::default().json();
        assert!(empty["babip"].is_null());
        assert!(empty["strikeouts_per_pa"].is_null());
        assert!(empty["singles_per_pa"].is_null());
        let mut counts = Counts::default();
        for result in [
            BattingResult::Single,
            BattingResult::Double,
            BattingResult::Out,
            BattingResult::FieldersChoice,
            BattingResult::HomeRun,
            BattingResult::Strikeout,
            BattingResult::Walk,
            BattingResult::Foul,
        ] {
            counts.batting(result);
        }
        let json = counts.json();
        assert_eq!(json["babip_balls_in_play"], 4);
        assert_eq!(json["babip"], 0.5);
        assert_eq!(json["strikeouts_per_pa"], 1.0 / 7.0);
        assert_eq!(json["singles_per_pa"], 1.0 / 7.0);
    }
}
