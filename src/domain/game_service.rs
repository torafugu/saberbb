use super::shared::game_state::{GameProgress, GameState, InningProgress};
use crate::domain::random_provider::RealRng;
use crate::domain::random_provider::RandomProvider;
use crate::domain::resolver::batting_calibration::with_calibrated_batting;
use crate::domain::resolver::batting_resolver::{
    SwingExecutionModel, with_collision_power_trial, with_swing_execution_model,
};
use crate::domain::seeded_diagnostic::derived_seed;
use crate::domain::shared::game::{GameResult, GameSchedule, TB};
use crate::domain::shared::player::Position;
use crate::domain::shared::stats::SeasonPitchingStats;
use crate::domain::strategy::pitcher_evaluation::PitcherAttributeWeights;
use crate::domain::strategy::pitching_usage::PitchingStaff;
use crate::domain::strategy::position_player_usage::PositionPlayers;
use std::collections::BTreeMap;
use crate::domain::strategy::batting_strategy::BattingStrategySettings;
use crate::repositories::game_repository::GameRoundRepository;
use crate::t;
use anyhow::{Context, Result, ensure};
use tracing::info;

/// Legacy swing-speed bonus used as the reference in diagnostic sweeps.
pub const GAME_SWING_SPEED_BONUS_MS: f64 = 2.0;
pub const GAME_BATTING_EYE_REDUCTION: f64 = 0.25;
const LIVE_SWING_SPEED_CAP_MS: f64 = 34.0;
const OPENING_LINEUP_SWING_SPEED_CAP_MS: f64 = 35.5;
const LIVE_COLLISION_POWER_RETAINED_FRACTION: f64 = 0.25;

struct LiveSwingSpeed {
    bonus_ms: f64,
    cap_ms: Option<f64>,
}

impl LiveSwingSpeed {
    fn from_env() -> Result<Self> {
        let read = |name| -> Result<Option<f64>> {
            match std::env::var(name) {
                Ok(value) => {
                    let value: f64 = value.parse().with_context(|| format!("Reading {name}"))?;
                    ensure!(value.is_finite(), "{name} must be finite");
                    Ok(Some(value))
                }
                Err(std::env::VarError::NotPresent) => Ok(None),
                Err(error) => Err(error).with_context(|| format!("Reading {name}")),
            }
        };
        let bonus_ms = read("SABERBB_SWING_SPEED_BONUS_MS")?.unwrap_or(0.0);
        let cap_ms = read("SABERBB_SWING_SPEED_CAP_MS")?.unwrap_or(LIVE_SWING_SPEED_CAP_MS);
        ensure!(
            bonus_ms >= 0.0,
            "SABERBB_SWING_SPEED_BONUS_MS must be nonnegative"
        );
        ensure!(
            cap_ms > 0.0,
            "SABERBB_SWING_SPEED_CAP_MS must be positive"
        );
        Ok(Self { bonus_ms, cap_ms: Some(cap_ms) })
    }

    fn apply(&self, roster_speed_ms: f64) -> f64 {
        let live_speed = roster_speed_ms + self.bonus_ms;
        self.cap_ms.map_or(live_speed, |cap| live_speed.min(cap))
    }
}

fn apply_live_batting_abilities(schedule: &mut GameSchedule, speed: &LiveSwingSpeed) {
    for player in schedule
        .away_team
        .players
        .iter_mut()
        .chain(&mut schedule.home_team.players)
    {
        if let Some(batter) = &mut player.offense_skills.batter {
            batter.swing_speed = speed.apply(batter.swing_speed);
            batter.batting_eye -= GAME_BATTING_EYE_REDUCTION;
        }
    }
}

fn swing_execution_from_env(default: SwingExecutionModel) -> Result<SwingExecutionModel> {
    match std::env::var("SABERBB_SWING_EXECUTION") {
        Err(std::env::VarError::NotPresent) => Ok(default),
        Ok(value) => match value.as_str() {
            "legacy" => Ok(SwingExecutionModel::Legacy),
            "gaussian" => Ok(SwingExecutionModel::Gaussian),
            "mixture" => Ok(SwingExecutionModel::Mixture),
            _ => anyhow::bail!("SABERBB_SWING_EXECUTION must be legacy, gaussian, or mixture"),
        },
        Err(err) => Err(err).context("Reading SABERBB_SWING_EXECUTION"),
    }
}

/// Median calculated with one swing-power value per roster batter.
pub fn roster_swing_power_median(mut powers: Vec<f64>) -> Result<f64> {
    ensure!(!powers.is_empty(), "no roster batters for the collision power median");
    ensure!(powers.iter().all(|power| power.is_finite()), "invalid roster swing power");
    powers.sort_by(f64::total_cmp);
    let count = powers.len();
    Ok(powers[(count - 1) / 2].midpoint(powers[count / 2]))
}

pub struct GameService<R: GameRoundRepository> {
    pub repo: R,
}

impl<R: GameRoundRepository> GameService<R> {
    pub fn process_game_round(&mut self) -> Result<()> {
        self.run_game_round(false)
    }

    /// Run the same games while collecting detailed contact diagnostics.
    pub fn analyze_game_round(&mut self) -> Result<()> {
        self.run_game_round(true)
    }

    fn run_game_round(&mut self, record_contact_states: bool) -> Result<()> {
        let swing_speed = LiveSwingSpeed::from_env()?;
        let lineup_speed = LiveSwingSpeed {
            bonus_ms: swing_speed.bonus_ms,
            cap_ms: Some(OPENING_LINEUP_SWING_SPEED_CAP_MS),
        };
        let execution_model = swing_execution_from_env(SwingExecutionModel::Mixture)?;
        let pitch_location_error_floor = match std::env::var("SABERBB_PITCH_LOCATION_ERROR_FLOOR") {
            Ok(value) => value
                .parse::<f64>()
                .context("Reading SABERBB_PITCH_LOCATION_ERROR_FLOOR")?,
            Err(std::env::VarError::NotPresent) => 0.0,
            Err(error) => return Err(error).context("Reading SABERBB_PITCH_LOCATION_ERROR_FLOOR"),
        };
        ensure!(
            (0.0..=0.5).contains(&pitch_location_error_floor),
            "SABERBB_PITCH_LOCATION_ERROR_FLOOR must be between 0 and 0.5"
        );
        let master_seed = match std::env::var("SABERBB_SCHEDULED_GAME_SEED") {
            Ok(value) => Some(
                value
                    .parse::<u64>()
                    .context("Reading SABERBB_SCHEDULED_GAME_SEED")?,
            ),
            Err(std::env::VarError::NotPresent) => None,
            Err(error) => return Err(error).context("Reading SABERBB_SCHEDULED_GAME_SEED"),
        };
        info!(
            swing_speed_bonus_ms = swing_speed.bonus_ms,
            swing_speed_cap_ms = ?swing_speed.cap_ms,
            collision_power_retained_fraction = LIVE_COLLISION_POWER_RETAINED_FRACTION,
            ?execution_model,
            pitch_location_error_floor,
            ?master_seed,
            "process_game_round() started"
        );

        let game_schedules = self
            .repo
            .load_game_schedules_to_process()
            .context(t!("error", "function" => "load_game_schedules_to_process"))?;

        let mut medians = BTreeMap::new();
        for game in &game_schedules {
            if let std::collections::btree_map::Entry::Vacant(entry) = medians.entry(game.season) {
                let median = roster_swing_power_median(
                    self.repo.load_season_roster_swing_powers(game.season)?,
                )
                .with_context(|| format!("Calculating roster swing-power median for {}", game.season))?;
                entry.insert(median);
            }
        }

        // TODO: Check postponement
        for mut game_schedule in game_schedules {
            info!("new game started");

            let loads = self.repo.load_pitcher_workload(&game_schedule)?;
            let season_stats = self.repo.load_pitcher_season_stats(&game_schedule)?;
            let position_fatigue = self.repo.load_position_player_fatigue(&game_schedule)?;
            let median = medians[&game_schedule.season];
            let mut lineup_source = game_schedule.clone();
            apply_live_batting_abilities(&mut lineup_source, &lineup_speed);
            apply_live_batting_abilities(&mut game_schedule, &swing_speed);
            let rng = match master_seed {
                Some(seed) => RealRng::from_seed(derived_seed(seed, game_schedule.id)),
                None => RealRng::new(),
            };
            let result = with_collision_power_trial(median, LIVE_COLLISION_POWER_RETAINED_FRACTION, || {
                simulate_staffed_game(
                    game_schedule,
                    Box::new(rng),
                    &loads,
                    &season_stats,
                    Some(&position_fatigue),
                    None,
                    record_contact_states,
                    Some(&lineup_source),
                    execution_model,
                    pitch_location_error_floor,
                )
            })?;
            if let Err(e) = self.repo.update_game_result(&result) {
                eprintln!("{}:{}", t!("error", "function" => "save_game_result"), e);
                return Err(e.into());
            }
        }

        if let Err(e) = self.repo.update_current_round_seq() {
            eprintln!("{}:{}", t!("error", "function" => "updated_game_result"), e);
            return Err(e.into());
        }

        Ok(())
    }
}

/// Play a single independent game for seeded diagnostics.
pub fn simulate_game(
    schedule: GameSchedule,
    rng: Box<dyn RandomProvider>,
    settings: BattingStrategySettings,
    pitch_budget: Option<u32>,
    record_contact_states: bool,
) -> Result<GameResult> {
    play_game(
        GameState::new(rng, schedule)?,
        settings,
        pitch_budget,
        record_contact_states,
        None,
        None,
    )
}

/// Play with the regular-game staff and position-player policies and supplied pregame history.
pub fn simulate_game_with_pitching(
    schedule: GameSchedule,
    rng: Box<dyn RandomProvider>,
    loads: &BTreeMap<i64, f64>,
    season_stats: &SeasonPitchingStats,
    position_fatigue: Option<&BTreeMap<i64, f64>>,
    pitch_budget: Option<u32>,
    record_contact_states: bool,
) -> Result<GameResult> {
    simulate_game_with_pitching_and_lineup_source(
        schedule,
        rng,
        loads,
        season_stats,
        position_fatigue,
        pitch_budget,
        record_contact_states,
        None,
    )
}

/// Play with a control schedule whose rosters differ only in batting abilities.
/// Use it for opening lineup selection and use trial abilities during play.
pub fn simulate_game_with_pitching_and_lineup_source(
    schedule: GameSchedule,
    rng: Box<dyn RandomProvider>,
    loads: &BTreeMap<i64, f64>,
    season_stats: &SeasonPitchingStats,
    position_fatigue: Option<&BTreeMap<i64, f64>>,
    pitch_budget: Option<u32>,
    record_contact_states: bool,
    lineup_source: Option<&GameSchedule>,
) -> Result<GameResult> {
    simulate_staffed_game(
        schedule,
        rng,
        loads,
        season_stats,
        position_fatigue,
        pitch_budget,
        record_contact_states,
        lineup_source,
        swing_execution_from_env(SwingExecutionModel::Legacy)?,
        0.0,
    )
}

fn simulate_staffed_game(
    schedule: GameSchedule,
    rng: Box<dyn RandomProvider>,
    loads: &BTreeMap<i64, f64>,
    season_stats: &SeasonPitchingStats,
    position_fatigue: Option<&BTreeMap<i64, f64>>,
    pitch_budget: Option<u32>,
    record_contact_states: bool,
    lineup_source: Option<&GameSchedule>,
    execution_model: SwingExecutionModel,
    pitch_location_error_floor: f64,
) -> Result<GameResult> {
    let weights = PitcherAttributeWeights::default();
    let staff = |team| {
        PitchingStaff::new(team, loads, schedule.base_four_seam_speed, weights, season_stats)
    };
    let away = staff(&schedule.away_team);
    let home = staff(&schedule.home_team);
    let mut state = GameState::with_lineup_policy(
        rng,
        lineup_source.unwrap_or(&schedule),
        [away.starter(), home.starter()],
        position_fatigue,
    )?;
    state.pitch_location_error_floor = pitch_location_error_floor;
    if lineup_source.is_some() {
        ensure!(
            state.game_result.id == schedule.id,
            "lineup source game does not match trial"
        );
        for (lineup, team) in [
            (&mut state.away_lineup, &schedule.away_team),
            (&mut state.home_lineup, &schedule.home_team),
        ] {
            for active in lineup
                .players
                .iter_mut()
                .filter(|player| player.batter.is_some())
            {
                let player = team
                    .players
                    .iter()
                    .find(|player| player.info.id == active.id)
                    .with_context(|| {
                        format!("Opening batter {} is missing from the trial roster", active.id)
                    })?;
                active.batter = Some(player.batter()?);
            }
        }
    }
    with_calibrated_batting(|| {
        with_swing_execution_model(execution_model, || {
            play_game(
                state,
                BattingStrategySettings::player_driven(),
                pitch_budget,
                record_contact_states,
                Some([away, home]),
                position_fatigue.map(|entering| {
                    [&schedule.away_team, &schedule.home_team].map(|team| PositionPlayers {
                        team,
                        entering,
                        pending: None,
                    })
                }),
            )
        })
    })
}

fn play_game(
    mut state: GameState,
    settings: BattingStrategySettings,
    pitch_budget: Option<u32>,
    record_contact_states: bool,
    staffs: Option<[PitchingStaff<'_>; 2]>,
    mut position_players: Option<[PositionPlayers<'_>; 2]>,
) -> Result<GameResult> {
    state.record_contact_states = record_contact_states;
    state.batting_strategy_settings = settings;
    let mut pitch_counts = BTreeMap::new();
    let mut pitches = 0;
    while let GameProgress::Ongoing = state.progress() {
        info!("new inning started");
        state.advance_half_inning()?;
        let home_defending = state.inning_tb == TB::Top;
        if let Some(players) = &mut position_players {
            let defense = &mut players[usize::from(home_defending)];
            if let Some(pinch_hit) = defense.pending.take() {
                state.change_position_player(
                    home_defending,
                    pinch_hit.hitter.info.id,
                    pinch_hit.defender,
                    pinch_hit.position,
                )?;
            }
            if state.inning_seq >= 4 {
                let lineup = if home_defending {
                    &state.home_lineup
                } else {
                    &state.away_lineup
                };
                if let Some((id, replacement, role)) =
                    defense.defensive_change(&state.game_result, lineup)
                {
                    state.change_position_player(home_defending, id, replacement, role)?;
                }
            }
        }
        while let InningProgress::Ongoing = state.inning_state.inning_progress() {
            if let Some(limit) = pitch_budget {
                anyhow::ensure!(pitches < limit, "declared pitch budget exceeded; run must not silently drop this game");
                anyhow::ensure!(state.away_total_point <= 240 && state.home_total_point <= 240, "diagnostic score limit reached");
            }

            if let Some(staffs) = &staffs
                && state.inning_state.active_batter.is_none()
            {
                let staff = &staffs[usize::from(state.inning_tb == TB::Top)];
                let active_id = state.current_pitcher()?.id;
                if let Some(replacement) = staff.reliever(active_id, &pitch_counts) {
                    state.change_pitcher(replacement)?;
                }
            }
            if let Some(players) = &mut position_players
                && state.inning_state.active_batter.is_none()
            {
                manage_pinch_hit(
                    &mut state,
                    &mut players[usize::from(!home_defending)],
                    !home_defending,
                )?;
            }
            state.process_count()?;
            pitches += 1;
            if staffs.is_some() && let Some(pitch) = state.game_result.player_pitchings.last() {
                *pitch_counts.entry(pitch.pitcher_id).or_default() += 1;
            }
            if let GameProgress::WalkOff = state.progress() {
                break;
            }
        }
        state.finish_half_inning();
        if let GameProgress::GameSet = state.progress() {
            break;
        }
    }
    info!("game completed");
    state.finish_game();
    Ok(state.game_result)
}

fn manage_pinch_hit(
    state: &mut GameState,
    offense: &mut PositionPlayers<'_>,
    home: bool,
) -> Result<()> {
    if state.inning_seq < 7 {
        return Ok(());
    }
    let lineup = if home {
        &state.home_lineup
    } else {
        &state.away_lineup
    };
    let Some(active) = lineup.next_position_player() else {
        return Ok(());
    };
    let outgoing = active.id;
    if let Some(pinch_hit) = &offense.pending {
        // If the lineup bats around, resolve the temporary role before its next plate appearance.
        if pinch_hit.hitter.info.id == outgoing {
            let pinch_hit = offense.pending.take().expect("pending pinch hitter");
            state.change_position_player(home, outgoing, pinch_hit.defender, pinch_hit.position)?;
        }
    } else if state.home_total_point.abs_diff(state.away_total_point) <= 3
        && let Some(pinch_hit) = offense.pinch_hit(&state.game_result, active)
    {
        state.change_position_player(home, outgoing, pinch_hit.hitter, Position::PH)?;
        offense.pending = Some(pinch_hit);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::game::{GameResult, GameSchedule, GameType, TB};
    use crate::domain::shared::player::PitcherCharacter;
    use crate::domain::shared::player::{
        ArmSlot, BatterInfo, BatterType, DefenseSkills, FielderInfo, FielderType, PitchSkill,
        PitchType, PitcherInfo, PitcherStyle, Player, PlayerInfo, Position, RL, RunningSkills,
    };
    use crate::domain::shared::stadium::Stadium;
    use crate::domain::shared::stats::SeasonPitchingLine;
    use crate::domain::shared::team::Team;
    use crate::error::AppError;
    use crate::repositories::game_repository::{
        GameResultWriter, GameRoundWriter, GameScheduleReader,
    };
    use anyhow::anyhow;
    use chrono::NaiveDate;

    struct RecordingRepo {
        schedules: Vec<GameSchedule>,
        load_error: bool,
        save_error_at: Option<usize>,
        update_error: bool,
        save_calls: usize,
        saved_results: Vec<GameResult>,
        update_calls: usize,
    }

    impl RecordingRepo {
        fn new(schedules: Vec<GameSchedule>) -> Self {
            Self {
                schedules,
                load_error: false,
                save_error_at: None,
                update_error: false,
                save_calls: 0,
                saved_results: Vec::new(),
                update_calls: 0,
            }
        }
    }

    impl GameResultWriter for RecordingRepo {
        fn update_game_result(&mut self, game: &GameResult) -> std::result::Result<(), AppError> {
            let call_index = self.save_calls;
            self.save_calls += 1;

            if self.save_error_at == Some(call_index) {
                return Err(AppError::Internal(anyhow!("save failed")));
            }

            self.saved_results.push(game.clone());
            Ok(())
        }
    }

    impl GameRoundWriter for RecordingRepo {
        fn update_current_round_seq(&mut self) -> std::result::Result<usize, AppError> {
            self.update_calls += 1;

            if self.update_error {
                return Err(AppError::Internal(anyhow!("update failed")));
            }

            Ok(1)
        }
    }

    impl GameScheduleReader for RecordingRepo {
        fn load_game_schedules_to_process(
            &self,
        ) -> std::result::Result<Vec<GameSchedule>, AppError> {
            if self.load_error {
                return Err(AppError::Internal(anyhow!("load failed")));
            }

            Ok(self.schedules.clone())
        }

        fn load_season_roster_swing_powers(&self, season: u16) -> Result<Vec<f64>, AppError> {
            Ok(self.schedules
                .iter()
                .filter(|game| game.season == season)
                .flat_map(|game| game.away_team.players.iter().chain(&game.home_team.players))
                .filter_map(|p| p.offense_skills.batter.map(|b| (p.info.id, b.swing_power)))
                .collect::<BTreeMap<_, _>>()
                .into_values()
                .collect())
        }

        fn load_team_game_schedules(
            &self,
            _team_id: u16,
            _start_date: chrono::NaiveDate,
            _end_date: chrono::NaiveDate,
        ) -> std::result::Result<Vec<crate::domain::shared::game::TeamGameScheduleView>, AppError>
        {
            Ok(Vec::new())
        }
    }

    fn player(id: u32) -> Player {
        let positions = [
            Position::P,
            Position::C,
            Position::FB,
            Position::SB,
            Position::TB,
            Position::SS,
            Position::LF,
            Position::CF,
            Position::RF,
            Position::DH,
        ];
        let mut player = Player::from_player_info(PlayerInfo::new(
            id as i64,
            format!("First{id}"),
            format!("Last{id}"),
            25,
            id as u8,
        ));
        player.offense_skills.running = RunningSkills {
            speed: 7.5,
            lead_distance: 2.0,
            start_reaction: 0.3,
        };
        player.offense_skills.batter = Some(BatterInfo {
            batting_side: RL::Right,
            batter_type: BatterType::ClassicAnalyst,
            zone_aptitude: crate::domain::shared::player::ZoneAptitude::Balanced,
            hot_zone_scale: 0.1,
            batting_eye: 1.0,
            swing_speed: 32.0,
            swing_power: 0.5,
            attack_angle: 10.0,
            bat_control: 1.0,
            consistency: 0.03,
        });

        let position = positions[((id - 1) as usize) % positions.len()];
        player.defense_skills = DefenseSkills::new(position);
        let fielder_info = FielderInfo {
            fielder_type: fielder_type_for_position(position),
            throw_speed: 38.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.6,
            catching: 0.8,
            reach_height: 2.5,
            reach_range: 1.0,
        };
        if position == Position::P {
            player.offense_skills.batter = None;
            player.defense_skills.pitcher = Some(PitcherInfo {
                height: 1.85,
                extension: 1.8,
                throw_side: RL::Right,
                arm_slot: ArmSlot::ThreeQuarter,
                pitcher_style: PitcherStyle::BalancedPitcher,
                pitcher_character: PitcherCharacter::Balanced,
                velocity: 40.8, // Physical m/s, not km/h.
                spin_rate: 2200.0,
                control: 1.0,
                stamina: 90.0,
                injury_proneness: 0.1,
                clutch: 0.6,
                hpp: 0.5,
                platoon_splitting: 0.2,
                delivery_motion_time: 1.4,
                consistency: 0.03,
                pitch_skills: vec![PitchSkill {
                    pitch_type: PitchType::FourSeamFastball,
                    velocity: 1.0,
                    control: 1.0,
                    stamina: 90.0,
                    injury_proneness: 0.1,
                    spin_rate: 1.0,
                    spin_angle: 0.0,
                    spin_efficiency: 0.9,
                    usage: 1.0,
                }],
                fielder_info,
            });
        } else if position == Position::C {
            player.defense_skills.catcher =
                Some(crate::domain::shared::player::CatcherInfo::from_fielder_info(fielder_info));
        } else if position.is_corner_infielder() {
            player.defense_skills.corner_infielder = Some(fielder_info);
        } else if position.is_middle_infielder() {
            player.defense_skills.middle_infielder = Some(fielder_info);
        } else if position.is_outfielder() {
            player.defense_skills.outfielder = Some(fielder_info);
        }
        player
    }

    fn fielder_type_for_position(position: Position) -> FielderType {
        if position == Position::P {
            FielderType::Pitcher
        } else if position == Position::C {
            FielderType::Catcher
        } else if position.is_corner_infielder() {
            FielderType::CornerInfielder
        } else if position.is_middle_infielder() {
            FielderType::MiddleInfielder
        } else {
            FielderType::Outfielder
        }
    }

    fn team(id: u16, name: &str, first_player_id: u32) -> Team {
        Team {
            manager:None,
            id,
            name: name.into(),
            players: (first_player_id..first_player_id + 10)
                .map(player)
                .collect(),
        }
    }

    #[test]
    fn fatigue_rotates_the_starter_and_relieves_between_batters() {
        let mut schedule = schedule(1);
        schedule.away_team.players.extend([player(21), player(31), player(41)]);
        for (index, stamina) in [(0, 2.0), (10, 1.0), (11, 0.0), (12, -1.0)] {
            schedule.away_team.players[index].defense_skills.pitcher.as_mut().unwrap().stamina = stamina;
        }
        let loads = BTreeMap::from([(1, 250.0), (21, 240.0), (31, 99.5)]);
        let season_stats = SeasonPitchingStats::default();
        let staff = PitchingStaff::new(
            &schedule.away_team,
            &loads,
            schedule.base_four_seam_speed,
            PitcherAttributeWeights::default(),
            &season_stats,
        );
        assert_eq!(staff.starter(), Some(31));

        let game = simulate_game_with_pitching(
            schedule,
            Box::new(RealRng::from_seed(9)),
            &loads,
            &season_stats,
            None,
            None,
            false,
        )
        .unwrap();
        let entries = game.player_entries.iter()
            .filter(|entry| entry.position == Position::P)
            .map(|entry| (entry.player_id, entry.start_count_seq, entry.end_count_seq.unwrap()))
            .collect::<Vec<_>>();
        let relief = *entries.iter().find(|entry| entry.0 == 41).unwrap();
        let starter = entries.iter().find(|entry| entry.0 == 31).unwrap();
        assert_eq!(starter.1, 1);
        assert_eq!(starter.2 + 1, relief.1);
        assert!(!entries.iter().any(|entry| entry.0 == 1 || entry.0 == 21));
        assert!(game.player_pitchings.iter().any(|pitch| pitch.pitcher_id == 31));
        assert!(game.player_pitchings.iter().any(|pitch| pitch.pitcher_id == 41));
        for (player, start, end) in entries {
            assert!(game.player_pitchings.iter().filter(|pitch| pitch.pitcher_id == player)
                .all(|pitch| (start..=end).contains(&pitch.count_seq)));
        }
        // Within a half-inning the preceding play must have completed a plate appearance.
        if game.innings.iter().any(|half| half.counts.iter().skip(1).any(|c| c.seq == relief.1)) {
            use crate::domain::shared::game::BattingResult;
            let prior = game.player_battings.iter().find(|play| play.count_seq == relief.1 - 1).unwrap();
            assert!(!matches!(prior.result, BattingResult::Take | BattingResult::StrikeSwung | BattingResult::Foul | BattingResult::WildPitch));
        }
    }

    #[test]
    fn attribute_rating_changes_rotation_membership_and_reliever_choice() {
        let mut staff_team = Team::min(1, "Test");
        for (id, stamina, control) in [
            (1, 2.0, -2.0),
            (11, 1.5, 0.0),
            (21, 1.0, 0.0),
            (31, 0.5, 0.0),
            (41, 0.25, 0.0),
            (51, 0.0, 2.0),
            (61, -1.0, 3.0),
        ] {
            let mut p = player(id);
            let pitcher = p.defense_skills.pitcher.as_mut().unwrap();
            pitcher.stamina = stamina;
            pitcher.control = control;
            staff_team.players.push(p);
        }
        let loads = BTreeMap::from([
            (1, 5.0), (11, 20.0), (21, 20.0), (31, 20.0), (41, 20.0), (61, 10.0),
        ]);
        let command_first = PitcherAttributeWeights {
            stuff: 0.0,
            command: 1.0,
            arsenal: 0.0,
        };
        let season_stats = SeasonPitchingStats::default();
        for weights in [command_first, PitcherAttributeWeights::default()] {
            let staff = PitchingStaff::new(&staff_team, &loads, 40.833, weights, &season_stats);
            assert_eq!(staff.starter(), Some(51));
            assert!(staff.reliever(51, &BTreeMap::from([(51, 99)])).is_none());
            assert_eq!(
                staff.reliever(51, &BTreeMap::from([(51, 100)])).map(|p| p.info.id),
                Some(61),
            );
            assert_eq!(
                staff.reliever(51, &BTreeMap::from([(51, 100), (61, 15)])).map(|p| p.info.id),
                Some(1),
            );
        }
    }

    #[test]
    fn season_pitching_results_replace_a_struggling_starter_after_several_outings() {
        let mut schedule = schedule(1);
        schedule.away_team.players.extend([player(21), player(31)]);
        for (index, stamina) in [(0, 0.5), (10, 0.5), (11, -0.5)] {
            schedule.away_team.players[index]
                .defense_skills
                .pitcher
                .as_mut()
                .unwrap()
                .stamina = stamina;
        }
        let loads = BTreeMap::from([(21, 10.0)]);
        let starter = |season: &SeasonPitchingStats| {
            PitchingStaff::new(
                &schedule.away_team,
                &loads,
                schedule.base_four_seam_speed,
                PitcherAttributeWeights::default(),
                season,
            )
            .starter()
        };
        let mut season = SeasonPitchingStats::default();
        assert_eq!(starter(&season), Some(1));

        season.season_total = SeasonPitchingLine {
            outs: 1350,
            runs: 300,
        };
        season
            .pitchers
            .insert(1, SeasonPitchingLine { outs: 15, runs: 6 });
        assert_eq!(starter(&season), Some(1));

        // Nearly identical workloads, with rates of 11.25 and 2.48 runs per nine.
        season.pitchers.insert(
            1,
            SeasonPitchingLine {
                outs: 120,
                runs: 50,
            },
        );
        season.pitchers.insert(
            21,
            SeasonPitchingLine {
                outs: 120,
                runs: 11,
            },
        );
        assert_eq!(starter(&season), Some(31));

        let result = simulate_game_with_pitching(
            schedule,
            Box::new(RealRng::from_seed(9)),
            &loads,
            &season,
            None,
            None,
            false,
        )
        .unwrap();
        assert!(result.player_entries.iter().any(|entry| {
            entry.position == Position::P && entry.start_count_seq == 1 && entry.player_id == 31
        }));
    }

    #[test]
    fn live_swing_speed_cap_applies_after_the_bonus() {
        let settings = LiveSwingSpeed {
            bonus_ms: 2.0,
            cap_ms: Some(35.5),
        };
        assert_eq!(settings.apply(32.0), 34.0);
        assert_eq!(settings.apply(34.0), 35.5);
        assert_eq!(
            LiveSwingSpeed {
                bonus_ms: 2.0,
                cap_ms: None
            }
            .apply(34.0),
            36.0
        );
    }

    #[test]
    fn control_roster_preserves_opening_lineups_when_power_changes_selection() {
        use crate::domain::shared::team::MAX_LINEUP_PLAYERS;

        let mut control = schedule(1);
        let mut alternate = player(20);
        alternate.offense_skills.batter.as_mut().unwrap().swing_power = 1.0;
        control.away_team.players.push(alternate);
        let mut trial = control.clone();
        trial
            .away_team
            .players
            .last_mut()
            .unwrap()
            .offense_skills
            .batter
            .as_mut()
            .unwrap()
            .swing_power = -1.0;
        let run = |schedule, lineup_source| {
            simulate_game_with_pitching_and_lineup_source(
                schedule,
                Box::new(RealRng::from_seed(9)),
                &BTreeMap::new(),
                &SeasonPitchingStats::default(),
                Some(&BTreeMap::new()),
                Some(5000),
                false,
                lineup_source,
            )
            .unwrap()
            .player_entries
            .iter()
            .take(2 * MAX_LINEUP_PLAYERS)
            .map(|entry| (entry.player_id, entry.position, entry.batting_order))
            .collect::<Vec<_>>()
        };
        let opening_control = run(control.clone(), None);
        assert!(opening_control.iter().any(|entry| entry.0 == 20));
        assert_ne!(run(trial.clone(), None), opening_control);
        assert_eq!(run(trial, Some(&control)), opening_control);
    }

    #[test]
    fn swing_speed_replays_preserve_the_live_game() {
        use crate::domain::shared::game_state::with_swing_speed_audit;

        let schedule = schedule(1);
        let speed = schedule.away_team.players[1].batter().unwrap().swing_speed;
        let run = || {
            simulate_game(
                schedule.clone(),
                Box::new(RealRng::from_seed(9)),
                BattingStrategySettings::player_driven(),
                Some(5000),
                false,
            )
            .unwrap()
        };
        let baseline = run();
        let (live, swings) = with_swing_speed_audit(2, &[speed, speed - 1.0], run);
        assert!(!swings.is_empty());
        for (_, plays) in swings {
            assert_eq!(plays[0].outcome, plays[1].outcome);
            assert_eq!(plays[0].flight, plays[1].flight);
        }
        assert_eq!(
            serde_json::to_value(baseline).unwrap(),
            serde_json::to_value(live).unwrap()
        );
    }

    #[test]
    fn position_player_policy_rests_tired_starters_and_records_pinch_hit_roles() {
        let mut schedule = schedule(1);
        let mut dh = player(20);
        dh.offense_skills.batter.as_mut().unwrap().swing_power = 5.0;
        let mut hitter = player(30);
        hitter.offense_skills.batter.as_mut().unwrap().swing_power = 8.0;
        schedule.away_team.players.extend([player(12), dh, hitter]);
        let tired_starters = BTreeMap::from([(2, 0.8), (30, 0.8)]);
        let rested = GameState::with_lineup_policy(
            Box::new(RealRng::from_seed(9)),
            &schedule,
            [Some(1), Some(101)],
            Some(&tired_starters),
        )
        .unwrap();
        assert_eq!(rested.away_lineup.catcher().unwrap().id, 12);

        let loads = BTreeMap::from([(30, 0.8)]);
        let mut state = GameState::with_lineup_policy(
            Box::new(RealRng::from_seed(9)),
            &schedule,
            [Some(1), Some(101)],
            Some(&loads),
        )
        .unwrap();
        let mut offense = PositionPlayers {
            team: &schedule.away_team,
            entering: &loads,
            pending: None,
        };
        state
            .game_result
            .position_player_workloads
            .entry(2)
            .or_default()
            .fielding = 0.9;
        let (outgoing, defender, role) = offense
            .defensive_change(&state.game_result, &state.away_lineup)
            .unwrap();
        assert_eq!((outgoing, defender.info.id, role), (2, 12, Position::C));

        state.inning_seq = 7;
        state.inning_tb = TB::Top;
        state.count_seq = 57;
        let catcher = state
            .away_lineup
            .players
            .iter()
            .find(|p| p.id == 2)
            .unwrap();
        let slot = catcher.batting_order.unwrap();
        state.away_lineup.current_index = usize::from(slot - 1);
        manage_pinch_hit(&mut state, &mut offense, false).unwrap();
        let pinch_hit = offense.pending.take().unwrap();
        assert_eq!(
            (pinch_hit.hitter.info.id, pinch_hit.defender.info.id),
            (30, 12)
        );
        assert_eq!(state.current_batter().unwrap().id, 30);
        assert_eq!(
            state
                .game_result
                .player_entries
                .iter()
                .find(|e| e.player_id == 2)
                .unwrap()
                .end_count_seq,
            Some(56)
        );

        state.inning_tb = TB::Bottom;
        state.count_seq = 65;
        state
            .change_position_player(false, 30, pinch_hit.defender, pinch_hit.position)
            .unwrap();
        assert_eq!(state.current_catcher().unwrap().id, 12);
        let entry = |id| {
            state
                .game_result
                .player_entries
                .iter()
                .find(|e| e.player_id == id)
                .unwrap()
        };
        assert_eq!(
            (
                entry(30).position,
                entry(30).batting_order,
                entry(30).end_count_seq
            ),
            (Position::PH, slot, Some(64))
        );
        assert_eq!(
            (
                entry(12).position,
                entry(12).batting_order,
                entry(12).start_count_seq
            ),
            (Position::C, slot, 65)
        );
        assert!(
            state
                .change_position_player(false, 12, &schedule.away_team.players[1], Position::C)
                .is_err()
        );
    }

    fn schedule(id: u32) -> GameSchedule {
        GameSchedule {
            id,
            season: 2026,
            round_seq: 1,
            seq: id as u16,
            planned_date: NaiveDate::from_ymd_opt(2026, 4, id).unwrap(),
            away_team: team(1, "AAA", 1),
            home_team: team(2, "BBB", 101),
            stadium: Stadium::default(),
            game_type: GameType::Regular,
            max_inning: 9,
            base_four_seam_speed: 40.833,
        }
    }

    fn points_by_inning_type(game: &GameResult, inning_type: TB) -> u8 {
        game.innings
            .iter()
            .filter(|inning| inning.tb == inning_type)
            .flat_map(|inning| inning.counts.iter())
            .map(|count| count.point)
            .sum()
    }

    #[test]
    fn process_game_round_saves_one_result_for_one_schedule() {
        let mut scheduled = schedule(1);
        scheduled.away_team.players[1].offense_skills.batter.as_mut().unwrap().swing_speed = 38.0;
        let mut service = GameService {
            repo: RecordingRepo::new(vec![scheduled]),
        };

        let result = service.process_game_round();

        assert!(result.is_ok());
        assert_eq!(service.repo.save_calls, 1);
        assert_eq!(service.repo.saved_results.len(), 1);
        assert_eq!(service.repo.update_calls, 1);

        let saved = &service.repo.saved_results[0];
        assert_eq!(saved.id, 1);
        assert_eq!(
            saved.actual_date,
            NaiveDate::from_ymd_opt(2026, 4, 1).unwrap()
        );
        assert!(!saved.innings.is_empty());
        assert!(saved.player_contact_states.is_empty());
        let mut recorded_speeds = saved
            .player_battings
            .iter()
            .map(|batting| batting.bat_speed)
            .filter(|&speed| speed > 0.0)
            .peekable();
        assert!(recorded_speeds.peek().is_some());
        assert!(recorded_speeds.all(|speed| speed == 32.0 || speed == 34.0));
        assert!(saved.player_battings.iter().any(|batting| batting.bat_speed == 34.0));
        assert!(saved.player_battings.iter().any(|batting| batting.bat_speed == 32.0));
        assert_eq!(
            service.repo.schedules[0].away_team.players[1].batter().unwrap().swing_speed,
            38.0
        );
    }

    #[test]
    fn recording_modes_reach_saved_game_results() {
        let mut schedule = crate::domain::test_support::game_schedule();
        schedule.max_inning = 1;
        for record in [false, true] {
            let mut service = GameService {
                repo: RecordingRepo::new(vec![schedule.clone()]),
            };
            if record {
                service.analyze_game_round().unwrap();
            } else {
                service.process_game_round().unwrap();
            }
            assert_eq!(service.repo.save_calls, 1);
            assert_eq!(service.repo.update_calls, 1);
            assert_eq!(service.repo.saved_results[0].player_contact_states.is_empty(), !record);
        }
    }

    #[test]
    fn contact_recording_does_not_change_seeded_game_results() {
        let mut schedule = crate::domain::test_support::game_schedule();
        schedule.max_inning = 1;
        for seed in [7, 42] {
            let run = |record| simulate_game(
                schedule.clone(), Box::new(RealRng::from_seed(seed)),
                BattingStrategySettings::player_driven(), Some(10_000), record,
            ).unwrap();
            let ordinary = run(false);
            let analysis = run(true);
            assert!(ordinary.player_contact_states.is_empty());
            assert!(!analysis.player_contact_states.is_empty());
            let mut ordinary = serde_json::to_value(ordinary).unwrap();
            let mut analysis = serde_json::to_value(analysis).unwrap();
            ordinary.as_object_mut().unwrap().remove("player_contact_states");
            analysis.as_object_mut().unwrap().remove("player_contact_states");
            assert_eq!(ordinary, analysis, "recording changed game results for seed {seed}");
        }
    }

    #[test]
    fn full_games_generate_unique_fielding_candidate_keys() {
        use std::collections::HashSet;
        for seed in [7, 42, 20261108] {
            let mut schedule = schedule(1);
            schedule.max_inning = 12;
            // Use physical m/s velocities and contact-capable batting values;
            // older lineup fixtures intentionally use extreme values for order.
            for player in schedule.away_team.players.iter_mut().chain(schedule.home_team.players.iter_mut()) {
                if let Some(batter)=player.offense_skills.batter.as_mut() {
                    batter.swing_speed=32.0; batter.swing_power=0.5; batter.attack_angle=10.0;
                    batter.batting_eye=1.0; batter.bat_control=1.0;
                }
                if let Some(pitcher)=player.defense_skills.pitcher.as_mut() {
                    pitcher.velocity=40.8; pitcher.control=1.0;
                    for pitch in &mut pitcher.pitch_skills {pitch.control=1.0;}
                }
            }
            let game = simulate_game(schedule, Box::new(RealRng::from_seed(seed)),
                BattingStrategySettings::player_driven(), Some(10_000), false).unwrap();
            let mut keys = HashSet::new();
            for candidate in &game.player_fielding_candidates {
                assert!(keys.insert((candidate.count_seq, candidate.diagnostics.fielder_position)),
                    "duplicate generated candidate: seed={seed}, count={}, position={:?}",
                    candidate.count_seq, candidate.diagnostics.fielder_position);
            }
            assert!(!keys.is_empty());
            let counts: HashSet<_> = game.innings.iter().flat_map(|i| &i.counts).map(|c| c.seq).collect();
            assert!(keys.iter().all(|(seq,_)|counts.contains(seq)),"candidate without count: seed={seed}");
        }
    }

    #[test]
    fn process_game_round_saves_all_scheduled_games_and_updates_once() {
        let mut service = GameService {
            repo: RecordingRepo::new(vec![schedule(1), schedule(2)]),
        };

        let result = service.process_game_round();

        assert!(result.is_ok());
        assert_eq!(service.repo.save_calls, 2);
        assert_eq!(service.repo.saved_results.len(), 2);
        assert_eq!(service.repo.saved_results[0].id, 1);
        assert_eq!(service.repo.saved_results[1].id, 2);
        assert_eq!(service.repo.update_calls, 1);
    }

    #[test]
    fn process_game_round_generates_valid_game_result_shape() {
        let mut service = GameService {
            repo: RecordingRepo::new(vec![schedule(1)]),
        };

        service.process_game_round().unwrap();

        let saved = &service.repo.saved_results[0];
        // Consider in case canceled due to rain
        assert!((17..=18).contains(&saved.innings.len()));
        assert_eq!(
            saved.away_total_point,
            points_by_inning_type(saved, TB::Top)
        );
        assert_eq!(
            saved.home_total_point,
            points_by_inning_type(saved, TB::Bottom)
        );

        for inning in &saved.innings {
            assert!(inning.seq >= 1);
            assert!(!inning.counts.is_empty());
            for count in &inning.counts {
                assert!(count.seq >= 1);
                assert!(count.out <= 3);
            }
            assert!(
                inning
                    .counts
                    .windows(2)
                    .all(|counts| counts[0].seq < counts[1].seq)
            );
        }
    }

    #[test]
    fn process_game_round_returns_error_when_load_schedules_fails() {
        let mut repo = RecordingRepo::new(Vec::new());
        repo.load_error = true;
        let mut service = GameService { repo };

        let result = service.process_game_round();

        assert!(result.is_err());
        assert_eq!(service.repo.save_calls, 0);
        assert_eq!(service.repo.update_calls, 0);
    }

    #[test]
    fn process_game_round_returns_error_when_save_game_result_fails() {
        let mut repo = RecordingRepo::new(vec![schedule(1), schedule(2)]);
        repo.save_error_at = Some(1);
        let mut service = GameService { repo };

        let result = service.process_game_round();

        assert!(result.is_err());
        assert_eq!(service.repo.save_calls, 2);
        assert_eq!(service.repo.saved_results.len(), 1);
        assert_eq!(service.repo.saved_results[0].id, 1);
        assert_eq!(service.repo.update_calls, 0);
    }

    #[test]
    fn process_game_round_returns_error_when_update_game_result_fails() {
        let mut repo = RecordingRepo::new(vec![schedule(1)]);
        repo.update_error = true;
        let mut service = GameService { repo };

        let result = service.process_game_round();

        assert!(result.is_err());
        assert_eq!(service.repo.save_calls, 1);
        assert_eq!(service.repo.saved_results.len(), 1);
        assert_eq!(service.repo.update_calls, 1);
    }

    #[test]
    fn process_game_round_with_no_schedules_only_updates_round() {
        let mut service = GameService {
            repo: RecordingRepo::new(Vec::new()),
        };

        let result = service.process_game_round();

        assert!(result.is_ok());
        assert_eq!(service.repo.save_calls, 0);
        assert!(service.repo.saved_results.is_empty());
        assert_eq!(service.repo.update_calls, 1);
    }

    #[test]
    #[should_panic]
    fn process_game_round_panics_when_team_has_empty_lineup() {
        let mut game_schedule = schedule(1);
        game_schedule.away_team.players.clear();
        let mut service = GameService {
            repo: RecordingRepo::new(vec![game_schedule]),
        };

        service.process_game_round().unwrap();
    }
}
