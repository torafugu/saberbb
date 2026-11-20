use super::shared::game_state::{GameProgress, GameState, InningProgress};
use crate::domain::random_provider::RealRng;
use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::game::{GameResult, GameSchedule, TB};
use crate::domain::strategy::pitcher_evaluation::PitcherAttributeWeights;
use crate::domain::strategy::pitching_usage::PitchingStaff;
use std::collections::BTreeMap;
use crate::domain::strategy::batting_strategy::BattingStrategySettings;
use crate::repositories::game_repository::GameRoundRepository;
use crate::t;
use anyhow::{Context, Result};
use tracing::info;

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
        info!("process_game_round() started");

        let game_schedules = self
            .repo
            .load_game_schedules_to_process()
            .context(t!("error", "function" => "load_game_schedules_to_process"))?;

        // TODO: Check postponement
        for game_schedule in game_schedules {
            info!("new game started");

            let loads = self.repo.load_pitcher_workload(&game_schedule)?;
            let result = simulate_game_with_pitching(
                &game_schedule,
                Box::new(RealRng::new()),
                &loads,
                record_contact_states,
            )?;
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
    )
}

fn simulate_game_with_pitching(
    schedule: &GameSchedule,
    rng: Box<dyn RandomProvider>,
    loads: &BTreeMap<i64, f64>,
    record_contact_states: bool,
) -> Result<GameResult> {
    let weights = PitcherAttributeWeights::default();
    let away = PitchingStaff::new(&schedule.away_team, loads, schedule.base_four_seam_speed, weights);
    let home = PitchingStaff::new(&schedule.home_team, loads, schedule.base_four_seam_speed, weights);
    let state = GameState::with_starters(rng, schedule, away.starter(), home.starter())?;
    play_game(
        state,
        BattingStrategySettings::player_driven(),
        None,
        record_contact_states,
        Some([away, home]),
    )
}

fn play_game(
    mut state: GameState,
    settings: BattingStrategySettings,
    pitch_budget: Option<u32>,
    record_contact_states: bool,
    staffs: Option<[PitchingStaff<'_>; 2]>,
) -> Result<GameResult> {
    state.record_contact_states = record_contact_states;
    state.batting_strategy_settings = settings;
    let mut pitch_counts = BTreeMap::new();
    let mut pitches = 0;
    while let GameProgress::Ongoing = state.progress() {
        info!("new inning started");
        state.advance_half_inning()?;
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
        let staff = PitchingStaff::new(
            &schedule.away_team,
            &loads,
            schedule.base_four_seam_speed,
            PitcherAttributeWeights::default(),
        );
        assert_eq!(staff.starter(), Some(31));

        let game = simulate_game_with_pitching(
            &schedule,
            Box::new(RealRng::from_seed(9)),
            &loads,
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
        for weights in [command_first, PitcherAttributeWeights::default()] {
            let staff = PitchingStaff::new(&staff_team, &loads, 40.833, weights);
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
        let mut service = GameService {
            repo: RecordingRepo::new(vec![schedule(1)]),
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
