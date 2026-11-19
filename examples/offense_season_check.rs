//! Full-game seeded offense diagnostics; reads SABERBB_DB and never saves game results.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use saberbb::{
    I18nManager,
    domain::{
        game_service::simulate_game,
        random_provider::RealRng,
        resolver::batting_resolver::{
            CONTACT_TIMING_POSITION_SCALE, SwingContactType, with_contact_geometry_scale,
        },
        resolver::fielding_physics::{
            CatchType, with_direct_catch_margin_scale, with_pre_fence_pickup_trial,
        },
        seeded_diagnostic::derived_seed,
        shared::{
            game::{BattingResult, GameResult, GameSchedule},
            game_state::{
                DEFAULT_LINE_DRIVE_LIFT_OFFSET_MM, LineDriveLiftStats, Ruling,
                VERTICAL_CONTACT_WINDOW_MM,
                with_line_drive_lift_trial, with_right_field_first_trial,
                with_vertical_contact_trial, with_vertical_contact_window_trial,
            },
            infield_opportunity_audit::InfieldOpportunityAudit,
            low_line_drive_fielding_audit::LowLineDriveFieldingAudit,
            no_bounce_catch_audit::NoBounceCatchAudit,
            outfield_advance_audit::OutfieldAdvanceAudit,
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
#[path = "support/fair_launch_audit.rs"]
mod fair_launch_audit;
#[path = "support/moving_collision_audit.rs"]
mod moving_collision_audit;
#[path = "support/pickup_timing_audit.rs"]
mod pickup_timing_audit;
#[path = "support/selected_intent_audit.rs"]
mod selected_intent_audit;
#[path = "support/throw_origin_audit.rs"]
mod throw_origin_audit;
#[path = "support/vertical_contact_game_sweep.rs"]
mod vertical_contact_game_sweep;
#[path = "support/vertical_contact_replay.rs"]
mod vertical_contact_replay;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    season: u16,
    /// Compare 0, +10, +15 and +20 mm for swings with the existing LineDrive intent.
    #[arg(long, conflicts_with_all = ["timing_geometry_sweep", "right_field_first_comparison", "no_bounce_margin_comparison"])]
    line_drive_lift_comparison: bool,
    /// Comma-separated vertical offsets in mm; defaults to 0,10,15,20.
    #[arg(long, value_delimiter = ',', requires = "line_drive_lift_comparison")]
    line_drive_lift_offsets: Vec<u16>,
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
    /// Compare exit-speed and launch-angle frequencies over all fair batting outcomes.
    #[arg(long)]
    fair_launch_diagnostics: bool,
    /// Replay simulated swings at -4, -2, 0, +2, and +4 mm vertical contact offsets.
    #[arg(long)]
    vertical_contact_sweep: bool,
    /// Simulate full games at 0 and +4 mm relative to current vertical contact positions.
    #[arg(long, conflicts_with_all = [
        "vertical_contact_sweep", "line_drive_lift_comparison", "timing_geometry_sweep",
        "right_field_first_comparison", "no_bounce_margin_comparison", "pre_fence_pickup_comparison"
    ])]
    vertical_contact_comparison: bool,
    /// Add a +4 mm trial restricted to pre-trial vertical contact positions in [10, 30) mm.
    #[arg(long, requires = "vertical_contact_comparison")]
    vertical_contact_window: bool,
    /// Diagnose catch and first-base out opportunities on 40–47.5 m/s, 10–15 degree singles.
    #[arg(long, requires = "vertical_contact_comparison")]
    vertical_contact_fielding_diagnostics: bool,
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
    /// Compare batter arrivals with direct and second-to-third throws on the same outfield play.
    #[arg(long)]
    outfield_advance_diagnostics: bool,
    /// Replay baseline singles at 100%, 50%, and 0% of the direct-catch margin.
    #[arg(long)]
    no_bounce_catch_diagnostics: bool,
    /// Simulate full games with 100% and 50% of the pre-adjustment direct-catch margin.
    #[arg(long, conflicts_with_all = ["timing_geometry_sweep", "right_field_first_comparison", "no_bounce_catch_diagnostics"])]
    no_bounce_margin_comparison: bool,
    /// Compare current games with outfielders allowed to pick up a bounced ball before wall impact.
    #[arg(long, conflicts_with_all = ["line_drive_lift_comparison", "timing_geometry_sweep", "right_field_first_comparison", "no_bounce_margin_comparison"])]
    pre_fence_pickup_comparison: bool,
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
    fn merge(&mut self, other: Self) {
        self.games += other.games;
        self.pitches += other.pitches;
        self.pa += other.pa;
        self.ab += other.ab;
        self.single += other.single;
        self.double += other.double;
        self.triple += other.triple;
        self.hr += other.hr;
        self.strikeouts += other.strikeouts;
        self.walks += other.walks;
        self.hit_by_pitch += other.hit_by_pitch;
        self.runs += other.runs;
        self.runs_allowed += other.runs_allowed;
        self.pitching_outs += other.pitching_outs;
        self.non_strikeout_outs += other.non_strikeout_outs;
    }

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

    fn vertical_contact_json(&self, include_window_metrics: bool) -> serde_json::Value {
        let mut summary = self.json();
        if include_window_metrics {
            let total_bases = self.single + 2 * self.double + 3 * self.triple + 4 * self.hr;
            let extra_base_hits = self.double + self.triple + self.hr;
            summary["slugging"] =
                serde_json::json!((self.ab > 0).then(|| total_bases as f64 / self.ab as f64));
            summary["extra_base_hits_per_pa"] =
                serde_json::json!((self.pa > 0).then(|| extra_base_hits as f64 / self.pa as f64));
        }
        summary
    }
}

#[derive(Default, Serialize)]
struct FastBallOutcomes {
    plays: u64,
    single: u64,
    double: u64,
    triple: u64,
    home_run: u64,
    out: u64,
    fielders_choice: u64,
    double_play: u64,
}

impl FastBallOutcomes {
    fn observe(&mut self, result: BattingResult) {
        let count = match result {
            BattingResult::Single => &mut self.single,
            BattingResult::Double => &mut self.double,
            BattingResult::Triple => &mut self.triple,
            BattingResult::HomeRun => &mut self.home_run,
            BattingResult::Out => &mut self.out,
            BattingResult::FieldersChoice => &mut self.fielders_choice,
            BattingResult::DoublePlay => &mut self.double_play,
            _ => return,
        };
        *count += 1;
        self.plays += 1;
    }

    fn merge(&mut self, other: Self) {
        self.plays += other.plays;
        self.single += other.single;
        self.double += other.double;
        self.triple += other.triple;
        self.home_run += other.home_run;
        self.out += other.out;
        self.fielders_choice += other.fielders_choice;
        self.double_play += other.double_play;
    }
}

#[derive(Default)]
struct VerticalContactOutcomes {
    swings: u64,
    whiffs: u64,
    low: FastBallOutcomes,
    next: FastBallOutcomes,
    low_line_drive: FastBallOutcomes,
    home_runs_20_to_30_deg: u64,
}

impl VerticalContactOutcomes {
    fn observe_ball(&mut self, speed: f64, angle: f64, result: BattingResult) {
        if LowLineDriveFieldingAudit::matches_launch(speed, angle) {
            self.low_line_drive.observe(result);
        }
        if result == BattingResult::HomeRun && (20.0..30.0).contains(&angle) {
            self.home_runs_20_to_30_deg += 1;
        }
        if !speed.is_finite() || speed < fair_launch_audit::HIGH_EV_MIN_MS {
            return;
        }
        if fair_launch_audit::TARGET_VLA.contains(&angle) {
            self.low.observe(result);
        } else if (10.0..15.0).contains(&angle) {
            self.next.observe(result);
        }
    }

    fn observe_game(&mut self, game: &GameResult) {
        for batting in &game.player_battings {
            if batting.swing_decision.is_some_and(|decision| decision.swung) {
                self.swings += 1;
                self.whiffs += u64::from(
                    batting.adjusted_contact_type == SwingContactType::SwungAndMiss,
                );
            }
            self.observe_ball(
                batting.ball.launch_speed,
                batting.ball.launch_angle,
                batting.result,
            );
        }
    }

    fn merge(&mut self, other: Self) {
        self.swings += other.swings;
        self.whiffs += other.whiffs;
        self.low.merge(other.low);
        self.next.merge(other.next);
        self.low_line_drive.merge(other.low_line_drive);
        self.home_runs_20_to_30_deg += other.home_runs_20_to_30_deg;
    }

    fn json(&self, include_window_metrics: bool) -> serde_json::Value {
        let mut report = serde_json::json!({
            "swings": self.swings,
            "whiffs": self.whiffs,
            "whiffs_per_swing": (self.swings > 0)
                .then(|| self.whiffs as f64 / self.swings as f64),
            "fast_fair_by_launch_angle_deg": {
                "[7,10)": self.low,
                "[10,15)": self.next,
            },
        });
        if include_window_metrics {
            report["low_line_drive"] = serde_json::json!(self.low_line_drive);
            report["home_runs_20_to_30_deg"] = serde_json::json!(self.home_runs_20_to_30_deg);
        }
        report
    }
}

#[derive(Default)]
struct VerticalContactSummary {
    total: Counts,
    outcomes: VerticalContactOutcomes,
    low_line_drive_fielding: LowLineDriveFieldingAudit,
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
fn line_drive_lift_offsets(args: &Args) -> Result<&[u16]> {
    let offsets = if args.line_drive_lift_offsets.is_empty() {
        &[0, 10, 15, 20]
    } else {
        args.line_drive_lift_offsets.as_slice()
    };
    ensure!(
        offsets
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == offsets.len(),
        "duplicate LineDrive lift offsets"
    );
    Ok(offsets)
}

fn run(args: Args) -> Result<()> {
    let lift_offsets = line_drive_lift_offsets(&args)?;
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
        "default_direct_catch_margin_percent": 50,
        "default_line_drive_lift_offset_mm": DEFAULT_LINE_DRIVE_LIFT_OFFSET_MM,
        "inputs_sha256": input_hash, "season": args.season, "games_per_seed": games.len(),
        "definitions": {
            "ra9_reference": "Runs allowed * 27 / recorded outs; earned runs not recorded",
            "non_strikeout_outs": "All recorded outs minus defensive strikeouts, including baserunning outs",
            "rosters": "Current source rosters, freshly initialized per game; historical lineups/seeds are not reproduced",
            "babip": "(1B + 2B + 3B) / (AB - K - HR); sacrifice flies have no separate result category"
        }
    });
    if args.vertical_contact_sweep {
        output["definitions"]["vertical_contact_sweep"] = serde_json::json!(
            "Fixed simulated swings replayed at signed world-vertical contact offsets in mm. Positive shifts lift the vertical deflection. All shifts use the same baseline-matched swing pool. Recovered swings share one deterministic set of face-angle draws. Fair and home-run counts are trajectory outcomes; official baseline counts use recorded game outcomes."
        );
        output["definitions"]["paired_zero_to_plus_four_mm"] = serde_json::json!(
            "Paired trajectories on the same comparable swings. Low line drives are fair at exit speed [40,47.5) m/s and angle [10,15) degrees. The home-run cohort requires a home-run trajectory and angle [20,30) degrees. Input cells group by baseline world-vertical contact displacement, actual bat angle and baseline exit speed. A geometric miss has no baseline exit speed."
        );
    }
    if args.vertical_contact_comparison {
        let mut trials = Vec::new();
        let policies = [("baseline", 0), ("uniform", 4)]
            .into_iter()
            .chain(args.vertical_contact_window.then_some(("contact_z_window", 4)));
        for (policy, offset_mm) in policies {
            eprintln!("Full-game vertical contact comparison: {policy}, +{offset_mm} mm");
            let mut summary = VerticalContactSummary::default();
            let run_trial = || {
                simulate_reports_inner(
                    &args,
                    &games,
                    CONTACT_TIMING_POSITION_SCALE,
                    false,
                    None,
                    Some(&mut summary),
                )
            };
            let windowed = policy == "contact_z_window";
            let reports = if windowed {
                with_vertical_contact_window_trial(offset_mm, run_trial)
            } else {
                with_vertical_contact_trial(offset_mm, run_trial)
            }
            .with_context(|| format!("vertical contact trial {policy}, +{offset_mm} mm"))?;
            let mut trial = serde_json::json!({
                "additional_vertical_contact_offset_mm": offset_mm,
                "total": summary.total.vertical_contact_json(args.vertical_contact_window),
                "vertical_contact_outcomes": summary.outcomes.json(args.vertical_contact_window),
                "reports": reports,
            });
            if args.vertical_contact_window {
                trial["policy"] = serde_json::json!(policy);
                if windowed {
                    trial["pre_trial_contact_z_range_mm"] = serde_json::json!({
                        "min_inclusive": VERTICAL_CONTACT_WINDOW_MM.start,
                        "max_exclusive": VERTICAL_CONTACT_WINDOW_MM.end,
                    });
                }
            }
            if args.vertical_contact_fielding_diagnostics {
                trial["low_line_drive_fielding"] =
                    serde_json::to_value(summary.low_line_drive_fielding)?;
            }
            trials.push(trial);
        }
        output["vertical_contact_trials"] = serde_json::json!(trials);
        output["definitions"]["vertical_contact_trials"] = serde_json::json!(if args.vertical_contact_window {
            "Full games under the baseline, uniform +4 mm, and [10,30) mm contact-Z window +4 mm policies. The window uses the live pre-trial world-vertical contact coordinate after the normal swing intent offset. Trials share schedule, rosters and starting RNG seeds, and pool counts before calculating rates. Changed plays can alter later decisions and RNG draws."
        } else {
            "Full games with an additional world-vertical contact offset on every swing, on top of the selected intent's normal offset. Trials share the schedule, rosters and starting RNG seed per game. Changed plays can change subsequent decisions and RNG draws. Trial totals pool all seeds before calculating rates."
        });
        if args.vertical_contact_window {
            output["definitions"]["low_line_drive"] = serde_json::json!(
                "Official fair batting outcomes at exit speed [40,47.5) m/s and vertical launch angle [10,15) degrees."
            );
        }
        output["definitions"]["fast_fair_by_launch_angle_deg"] = serde_json::json!(
            "Recorded fair batting outcomes with exit speed at least 45 m/s. Launch-angle intervals include the lower bound and exclude the upper bound. Each batted ball counts once, including a double play."
        );
        if args.vertical_contact_fielding_diagnostics {
            output["definitions"]["low_line_drive_fielding"] = serde_json::json!(
                "Official singles with exit speed [40,47.5) m/s and vertical launch angle [10,15) degrees. Direct catches replay eligible live candidates with 100%, 50%, or 0% of the current required catch margin. First-base scenarios use the recorded fielding point and ready time, the modeled batter time, and the faster of a direct throw with cover and an infield carry. For a throw, the receiver starts either at first or from the live position at ball control. The combined zero-margin catch or late-cover first-base count counts each single once. These are fixed-play opportunities."
            );
        }
    } else if args.pre_fence_pickup_comparison {
        eprintln!("Pre-fence pickup comparison: baseline");
        let baseline = simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, false, None)?;
        eprintln!("Pre-fence pickup comparison: enabled");
        let enabled = with_pre_fence_pickup_trial(|| {
            simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, false, None)
        })?;
        output["pre_fence_pickup_trials"] = serde_json::json!([
            {"enabled": false, "reports": baseline},
            {"enabled": true, "reports": enabled},
        ]);
        output["definitions"]["pre_fence_pickup"] = serde_json::json!(
            "Selected pickups on recorded plays before projected wall impact. Throw timing starts at the pickup position and running is resolved with the new defensive clock. Both trials share schedule, rosters and starting seed per game; changed plays can shift later draws."
        );
        output["definitions"]["recorded_running_outs_by_event"] = serde_json::json!(
            "Recorded running rulings of Out grouped by event; GrounderPlay can include the batter-runner, and BaseSteal is counted separately."
        );
    } else if args.line_drive_lift_comparison {
        let mut trials = Vec::new();
        for &offset_mm in lift_offsets {
            eprintln!("LineDrive vertical contact offset: +{offset_mm} mm");
            let reports = simulate_reports(
                &args,
                &games,
                CONTACT_TIMING_POSITION_SCALE,
                false,
                Some(offset_mm),
            )?;
            trials.push(
                serde_json::json!({"vertical_contact_offset_mm":offset_mm,"reports":reports}),
            );
        }
        output["line_drive_lift_trials"] = serde_json::json!(trials);
        output["definitions"]["line_drive_lift_trials"] = serde_json::json!(
            "Positive world-vertical contact offset applies only to swings whose selected intent is LineDrive. All trials share rosters, schedule and per-game starting RNG seeds; outcomes can change later draws."
        );
    } else if args.no_bounce_margin_comparison {
        let mut trials = Vec::new();
        for percent in [100, 50] {
            eprintln!("Direct-catch margin: {percent}%");
            let reports = with_direct_catch_margin_scale(f64::from(percent) / 100.0, || {
                simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, false, None)
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
                "reports": simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, enabled, None)?,
            }));
        }
        output["right_field_first_trials"] = serde_json::json!(trials);
    } else if args.timing_geometry_sweep {
        let mut trials = Vec::new();
        for scale in [CONTACT_TIMING_POSITION_SCALE, 0.875, 1.0] {
            let reports = with_contact_geometry_scale(scale, || {
                simulate_reports(&args, &games, scale, false, None)
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
            None,
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
    lift_offset_mm: Option<u16>,
) -> Result<Vec<serde_json::Value>> {
    simulate_reports_inner(args, games, scale, right_field_trial, lift_offset_mm, None)
}

fn simulate_reports_inner(
    args: &Args,
    games: &[GameSchedule],
    scale: f64,
    right_field_trial: bool,
    lift_offset_mm: Option<u16>,
    mut trial_summary: Option<&mut VerticalContactSummary>,
) -> Result<Vec<serde_json::Value>> {
    let mut reports = Vec::new();
    for seed in &args.seeds {
        let mut vertical_outcomes = trial_summary.is_some().then(VerticalContactOutcomes::default);
        let mut bip = args.bip_diagnostics.then(bip_audit::BipAudit::default);
        let mut vertical_contact = args
            .vertical_contact_sweep
            .then(vertical_contact_game_sweep::Sweep::default);
        let mut fair_launch = args
            .fair_launch_diagnostics
            .then(fair_launch_audit::FairLaunchAudit::default);
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
        let mut outfield = args
            .outfield_advance_diagnostics
            .then(OutfieldAdvanceAudit::default);
        let mut no_bounce = args
            .no_bounce_catch_diagnostics
            .then(NoBounceCatchAudit::default);
        let mut low_line_drive_fielding = args
            .vertical_contact_fielding_diagnostics
            .then(LowLineDriveFieldingAudit::default);
        let mut official_low_line_drive_singles = 0;
        let mut fielding = args
            .no_bounce_margin_comparison
            .then(FieldingCounts::default);
        let mut total = Counts::default();
        let mut running_outs = BTreeMap::new();
        let mut pre_fence_pickups = 0;
        let mut right_field_first_outs = 0;
        let mut lift_stats = LineDriveLiftStats::default();
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
                let mut run = || match low_line_drive_fielding.as_mut() {
                    Some(audit) => audit.observe_game(game_seed, run),
                    None => run(),
                };
                let mut run = || match no_bounce.as_mut() {
                    Some(audit) => audit.observe_game(game_seed, run),
                    None => run(),
                };
                let run = || match infield.as_mut() {
                    Some(audit) => audit.observe_game(run),
                    None => run(),
                };
                let run = || match outfield.as_mut() {
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
            } else if let Some(offset_mm) = lift_offset_mm {
                let (result, stats) = with_line_drive_lift_trial(offset_mm, run_with_diagnostics);
                lift_stats.swings += stats.swings;
                lift_stats.whiffs += stats.whiffs;
                lift_stats.line_drive_swings += stats.line_drive_swings;
                lift_stats.line_drive_whiffs += stats.line_drive_whiffs;
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
            if low_line_drive_fielding.is_some() {
                official_low_line_drive_singles += result
                    .player_battings
                    .iter()
                    .filter(|b| {
                        b.result == BattingResult::Single
                            && LowLineDriveFieldingAudit::matches(&b.ball)
                    })
                    .count() as u64;
            }
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
            if args.pre_fence_pickup_comparison {
                for running in &result.player_runnings {
                    if running.ruling == Ruling::Out {
                        *running_outs
                            .entry(running.event.as_ref().to_owned())
                            .or_insert(0u64) += 1;
                    }
                }
                let balls: BTreeMap<_, _> = result
                    .player_battings
                    .iter()
                    .map(|b| (b.count_seq, &b.ball))
                    .collect();
                pre_fence_pickups += result
                    .player_fieldings
                    .iter()
                    .filter(|f| {
                        f.catch_diagnostics
                            .zip(balls.get(&f.count_seq))
                            .is_some_and(|(catch, ball)| {
                                catch.pre_fence_pickup_position(ball).is_some()
                            })
                    })
                    .count();
            }
            accumulate(&mut total, &mut teams, &mut players, game, &result)?;
            if let Some(outcomes) = vertical_outcomes.as_mut() {
                outcomes.observe_game(&result);
            }
            if let Some(sweep) = vertical_contact.as_mut() {
                sweep.observe_game(game, &result, game_seed, scale);
            }
            if let Some(audit) = fair_launch.as_mut() {
                audit.observe_launch_speed_terms(game, &result, scale);
                audit.observe(&result);
            }
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
        let mut report = serde_json::json!({"seed":seed.to_string(),"total":total.vertical_contact_json(args.vertical_contact_window),
            "teams":teams.into_iter().map(|(id,t)|(id.to_string(),t.json())).collect::<BTreeMap<_,_>>(),
            "hr_leaders":leaders.iter().take(10).map(|(id,s)|serde_json::json!({"batter_id":id,"stats":s.json()})).collect::<Vec<_>>()});
        if args.pre_fence_pickup_comparison {
            report["pre_fence_pickup"] = serde_json::json!({
                "selected_pickups": pre_fence_pickups,
                "recorded_running_outs_by_event": running_outs,
                "doubles_per_pa": (total.pa > 0).then(|| total.double as f64 / total.pa as f64),
                "triples_per_pa": (total.pa > 0).then(|| total.triple as f64 / total.pa as f64),
            });
        }
        if lift_offset_mm.is_some() {
            report["line_drive_lift"] = serde_json::json!({
                "swings": lift_stats.swings,
                "whiffs": lift_stats.whiffs,
                "whiffs_per_swing": (lift_stats.swings > 0).then(|| lift_stats.whiffs as f64 / lift_stats.swings as f64),
                "line_drive_swings": lift_stats.line_drive_swings,
                "line_drive_whiffs": lift_stats.line_drive_whiffs,
                "line_drive_whiffs_per_swing": (lift_stats.line_drive_swings > 0)
                    .then(|| lift_stats.line_drive_whiffs as f64 / lift_stats.line_drive_swings as f64),
                "doubles_per_pa": (total.pa > 0).then(|| total.double as f64 / total.pa as f64),
                "triples_per_pa": (total.pa > 0).then(|| total.triple as f64 / total.pa as f64),
                "runs_per_game": total.runs as f64 / total.games as f64,
            });
        }
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
        if let Some(outfield) = outfield {
            report["outfield_advance_diagnostics"] = serde_json::to_value(outfield)?;
        }
        if let Some(sweep) = vertical_contact {
            report["vertical_contact_sweep"] = serde_json::to_value(sweep)?;
        }
        if let Some(audit) = fair_launch {
            ensure!(
                audit.non_hr_in_play == total.ab - total.strikeouts - total.hr
                    && audit.home_runs == total.hr,
                "fair launch audit counts do not match the official batting results"
            );
            report["fair_launch_diagnostics"] = serde_json::to_value(audit)?;
        }
        if let Some(infield) = infield {
            report["infield_opportunity_diagnostics"] = serde_json::to_value(infield)?;
        }
        if right_field_trial {
            report["right_field_first_outs"] = serde_json::json!(right_field_first_outs);
        }
        if let (Some(summary), Some(outcomes)) = (trial_summary.as_deref_mut(), vertical_outcomes) {
            report["vertical_contact_outcomes"] = outcomes.json(args.vertical_contact_window);
            summary.total.merge(total);
            summary.outcomes.merge(outcomes);
            if let Some(audit) = low_line_drive_fielding {
                ensure!(
                    audit.singles() == official_low_line_drive_singles,
                    "low line-drive fielding audit count does not match official singles for seed {seed}"
                );
                report["low_line_drive_fielding"] = serde_json::to_value(&audit)?;
                summary.low_line_drive_fielding.merge(audit);
            }
        }
        reports.push(report);
    }
    Ok(reports)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_contact_comparison_separates_replay_and_counts_official_results() {
        let args = ["check", "--season", "2026", "--vertical-contact-comparison"];
        assert!(Args::try_parse_from(args).unwrap().vertical_contact_comparison);
        let fielding_flag = "--vertical-contact-fielding-diagnostics";
        let window_flag = "--vertical-contact-window";
        assert!(
            Args::try_parse_from(args.into_iter().chain([fielding_flag]))
                .unwrap()
                .vertical_contact_fielding_diagnostics
        );
        assert!(Args::try_parse_from(["check", "--season", "2026", fielding_flag]).is_err());
        assert!(
            Args::try_parse_from(args.into_iter().chain([window_flag]))
                .unwrap()
                .vertical_contact_window
        );
        assert!(Args::try_parse_from(["check", "--season", "2026", window_flag]).is_err());
        assert!(
            Args::try_parse_from(args.into_iter().chain(["--vertical-contact-sweep"])).is_err()
        );
        assert!(
            Args::try_parse_from(args.into_iter().chain(["--line-drive-lift-comparison"])).is_err()
        );

        use BattingResult::*;
        let mut first = VerticalContactOutcomes::default();
        for (speed, angle, result) in [
            (45.0, 7.0, Single),
            (44.9, 8.0, Single),
            (46.0, 9.0, Foul),
            (45.0, 10.0, Double),
            (46.0, 14.9, DoublePlay),
            (46.0, 15.0, Triple),
            (f64::INFINITY, 12.0, Out),
            (40.0, 10.0, Single),
            (47.5, 10.0, Out),
            (49.0, 20.0, HomeRun),
            (46.0, 30.0, HomeRun),
        ] {
            first.observe_ball(speed, angle, result);
        }
        let mut second = VerticalContactOutcomes::default();
        second.observe_ball(46.0, 8.0, Out);
        second.observe_ball(46.0, 12.0, FieldersChoice);
        first.merge(second);
        assert!(first.json(false).get("low_line_drive").is_none());
        let report = first.json(true);
        let low = &report["fast_fair_by_launch_angle_deg"]["[7,10)"];
        let next = &report["fast_fair_by_launch_angle_deg"]["[10,15)"];
        assert_eq!((low["plays"].as_u64(), next["plays"].as_u64()), (Some(2), Some(4)));
        assert_eq!(low["single"], 1);
        assert_eq!(low["out"], 1);
        assert_eq!(next["double"], 1);
        assert_eq!(next["double_play"], 1);
        assert_eq!(next["fielders_choice"], 1);
        assert_eq!(report["low_line_drive"]["plays"], 4);
        assert_eq!(report["low_line_drive"]["single"], 1);
        assert_eq!(report["home_runs_20_to_30_deg"], 1);
        assert!(report["whiffs_per_swing"].is_null());
    }

    #[test]
    fn lift_offsets_keep_default_and_accept_full_schedule_shortlist() {
        let comparison = ["check", "--season", "2026", "--line-drive-lift-comparison"];
        let defaults = Args::try_parse_from(comparison).unwrap();
        assert_eq!(line_drive_lift_offsets(&defaults).unwrap(), [0, 10, 15, 20]);

        let select = |list| {
            Args::try_parse_from(
                comparison
                    .into_iter()
                    .chain(["--line-drive-lift-offsets", list]),
            )
        };
        let shortlist = select("0,10,20").unwrap();
        assert!(shortlist.games.is_none());
        assert_eq!(line_drive_lift_offsets(&shortlist).unwrap(), [0, 10, 20]);
        assert!(line_drive_lift_offsets(&select("0,10,10").unwrap()).is_err());
        assert!(
            Args::try_parse_from([
                "check",
                "--season",
                "2026",
                "--line-drive-lift-offsets",
                "0,10,20"
            ])
            .is_err()
        );
    }

    #[test]
    fn live_margin_override_restores_original_thresholds() {
        use saberbb::domain::resolver::fielding_physics::FielderRiskTolerance::*;
        assert_eq!(Aggressive.required_margin_sec(), 0.0);
        assert_eq!(Balanced.required_margin_sec(), 0.075);
        assert_eq!(Conservative.required_margin_sec(), 0.175);
        with_direct_catch_margin_scale(1.0, || {
            assert_eq!(Balanced.required_margin_sec(), 0.15);
            assert_eq!(Conservative.required_margin_sec(), 0.35);
        });
        assert_eq!(Balanced.required_margin_sec(), 0.075);
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
        assert!(counts.vertical_contact_json(false).get("slugging").is_none());
        let window_json = counts.vertical_contact_json(true);
        assert_eq!(window_json["slugging"], 7.0 / 6.0);
        assert_eq!(window_json["extra_base_hits_per_pa"], 2.0 / 7.0);
    }
}
