//! Full-game seeded offense diagnostics; reads SABERBB_DB and never saves game results.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use saberbb::{
    I18nManager,
    domain::{
        game_service::simulate_game,
        random_provider::RealRng,
        seeded_diagnostic::derived_seed,
        shared::{
            game::{BattingResult, GameResult, GameSchedule},
            team::Team,
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
#[path = "support/throw_origin_audit.rs"]
mod throw_origin_audit;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    season: u16,
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
        v["ra9_reference"] = if self.pitching_outs > 0 {
            serde_json::json!(self.runs_allowed as f64 * 27.0 / self.pitching_outs as f64)
        } else {
            serde_json::Value::Null
        };
        v
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
        let mut total = Counts::default();
        let mut teams = BTreeMap::new();
        let mut players = BTreeMap::new();
        for (index, game) in games.iter().enumerate() {
            let result = simulate_game(
                game.clone(),
                Box::new(RealRng::from_seed(derived_seed(*seed, game.id))),
                BattingStrategySettings::player_driven(),
                Some(args.max_pitches),
                false,
            )
            .with_context(|| {
                format!(
                    "seed {seed}, game {} failed; no partial summary accepted",
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
            accumulate(&mut total, &mut teams, &mut players, game, &result)?;
            if let Some(bip) = bip.as_mut() {
                bip.observe(game, &result);
            }
            if (index + 1) % 12 == 0 || index + 1 == games.len() {
                eprintln!("seed {seed}: {}/{} games complete", index + 1, games.len());
            }
        }
        let mut leaders: Vec<_> = players.into_iter().collect();
        leaders.sort_by(|a, b| b.1.hr.cmp(&a.1.hr).then_with(|| a.0.cmp(&b.0)));
        let mut report = serde_json::json!({"seed":seed.to_string(),"total":total.json(),
            "teams":teams.into_iter().map(|(id,t)|(id.to_string(),t.json())).collect::<BTreeMap<_,_>>(),
            "hr_leaders":leaders.iter().take(10).map(|(id,s)|serde_json::json!({"batter_id":id,"stats":s.json()})).collect::<Vec<_>>()});
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
        reports.push(report);
    }
    println!(
        "{}",
        serde_json::to_string_pretty(
            &serde_json::json!({"engine_source_sha256":saberbb::domain::seeded_diagnostic::ENGINE,
        "inputs_sha256":input_hash,"season":args.season,"games_per_seed":games.len(),"reports":reports,
        "definitions":{"ra9_reference":"Runs allowed * 27 / recorded outs; earned runs not recorded",
            "non_strikeout_outs":"All recorded outs minus defensive strikeouts, including baserunning outs",
            "rosters":"Current source rosters, freshly initialized per game; historical lineups/seeds are not reproduced"}})
        )?
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
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
}
