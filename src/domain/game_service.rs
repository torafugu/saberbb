use super::shared::game_state::{GameProgress, GameState, InningProgress};
use crate::domain::shared::game::GameResult;
use crate::domain::shared::player::Player;
use crate::repositories::game_repository::GameRepository;
use crate::t;
use anyhow::{Context, Result};
use tracing::info;

pub struct GameService<R: GameRepository> {
    pub repo: R,
}

impl<R: GameRepository> GameService<R> {
    pub fn process_game_round(&mut self) -> Result<()> {
        info!("process_game_round() started");

        // TODO: move stadium to GameSchedule
        let game_schedules = self
            .repo
            .load_game_schedules_to_process()
            .context(t!("error", "function" => "load_game_schedules_to_process"))?;

        // TODO: Check postponement
        for game_schedule in game_schedules {
            info!("new game started");

            let mut game_state = GameState::new(game_schedule)?;

            while let GameProgress::Ongoing = game_state.progress() {
                info!("new inning started");

                game_state.advance_half_inning();

                while let InningProgress::Ongoing = game_state.inning_state.innning_progress() {
                    // TODO: Consider ball updated
                    // TODO: Consider strike updated
                    game_state.batting_resolve()?;

                    if let GameProgress::WalkOff = game_state.progress() {
                        break;
                    }
                }

                game_state.finish_half_inning();

                if let GameProgress::GameSet = game_state.progress() {
                    break;
                }
            }

            info!("game completed");
            game_state.finish_game();

            if let Err(e) = self.repo.save_game_result(&game_state.game_result) {
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

mod tests {
    use super::*;
    use crate::domain::shared::game::{
        Count, GameDetail, GameHeader, GameSchedule, GameType, Inning, TB,
    };
    use crate::domain::shared::game_stat::{PlayerGameBattingView, PlayerGameEntryView};
    use crate::domain::shared::player::{
        BatterInfo, DefenseSkills, FielderInfo, FielderType, PitchSkill, PitchType, PitcherInfo,
        PitcherStyle, PlayerInfo, Position, RL, RunningSkills,
    };
    use crate::domain::shared::stadium::Stadium;
    use crate::domain::shared::team::Team;
    use crate::error::AppError;
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

    impl GameRepository for RecordingRepo {
        fn save_game_result(&mut self, game: &GameResult) -> std::result::Result<(), AppError> {
            let call_index = self.save_calls;
            self.save_calls += 1;

            if self.save_error_at == Some(call_index) {
                return Err(AppError::Internal(anyhow!("save failed")));
            }

            self.saved_results.push(game.clone());
            Ok(())
        }

        fn update_current_round_seq(&mut self) -> std::result::Result<usize, AppError> {
            self.update_calls += 1;

            if self.update_error {
                return Err(AppError::Internal(anyhow!("update failed")));
            }

            Ok(1)
        }

        fn load_processed_seasons(&self) -> std::result::Result<Vec<u16>, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_processed_game_headers(
            &self,
            _season: u16,
        ) -> std::result::Result<Vec<GameHeader>, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_game_schedules_to_process(
            &self,
        ) -> std::result::Result<Vec<GameSchedule>, AppError> {
            if self.load_error {
                return Err(AppError::Internal(anyhow!("load failed")));
            }

            Ok(self.schedules.clone())
        }

        fn load_game_detail(&self, _game_id: u32) -> std::result::Result<GameDetail, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_team_players(&self, _team_id: u16) -> std::result::Result<Vec<Player>, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_running_skills(
            &self,
            _player_id: i64,
        ) -> std::result::Result<RunningSkills, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_batter_info(&self, _player_id: i64) -> std::result::Result<BatterInfo, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_fielder_info(
            &self,
            _player_id: i64,
            _fielder_type: FielderType,
        ) -> std::result::Result<FielderInfo, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_pitcher_info(&self, _player_id: i64) -> std::result::Result<PitcherInfo, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_pitch_skill(
            &self,
            _player_id: i64,
        ) -> std::result::Result<Vec<PitchSkill>, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_defense_skills(
            &self,
            _player_id: i64,
        ) -> std::result::Result<DefenseSkills, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_innings(&self, _game_id: u32) -> std::result::Result<Vec<Inning>, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_counts(
            &self,
            _game_id: u32,
            _inning_seq: u8,
            _inning_tb: TB,
        ) -> std::result::Result<Vec<Count>, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_player_game_entry_views(
            &self,
            _game_id: u32,
        ) -> std::result::Result<Vec<PlayerGameEntryView>, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_player_game_batting_views(
            &self,
            _game_id: u32,
        ) -> std::result::Result<Vec<PlayerGameBattingView>, AppError> {
            unimplemented!("not used by GameService::process_game_round")
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
            swing_speed: 30.0,
            weight_pull: 0.3,
            weight_center: 0.3,
            weight_opposite: 0.2,
            weight_foul_left: 0.1,
            weight_foul_right: 0.1,
        });

        let position = positions[((id - 1) as usize) % positions.len()];
        player.defense_skills = DefenseSkills::new(position);
        let fielder_info = FielderInfo {
            fielder_type: fielder_type_for_position(position),
            throw_speed: 38.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.6,
        };
        if position == Position::P {
            player.offense_skills.batter = None;
            player.defense_skills.pitcher = Some(PitcherInfo {
                pitcher_style: PitcherStyle::BalancedPitcher,
                velocity: 145.0,
                control: 0.7,
                stamina: 90.0,
                injury_proneness: 0.1,
                clutch: 0.6,
                hpp: 0.5,
                platoon_splitting: 0.2,
                delivery_motion_time: 1.4,
                pitch_skills: vec![PitchSkill {
                    pitch_type: PitchType::FourSeamFastball,
                    velocity: 145.0,
                    control: 0.7,
                    stamina: 90.0,
                    injury_proneness: 0.1,
                    stuff: 0.8,
                    fb: 0.1,
                    gp: 0.4,
                    horizontal_movement: 0.0,
                    vertical_movement: 12.0,
                    spin_rate: 2200.0,
                    usage: 1.0,
                }],
                fielder_info,
            });
        } else if position == Position::C {
            player.defense_skills.catcher =
                Some(crate::domain::shared::player::CatcherInfo { fielder_info });
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
            id,
            name: name.into(),
            players: (first_player_id..first_player_id + 10)
                .map(player)
                .collect(),
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
