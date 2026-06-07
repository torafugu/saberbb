use super::shared::game_state::{GameProgress, GameState, InningProgress, InningState};
use crate::domain::shared::game::GameResult;
use crate::domain::shared::player::Player;
use crate::repositories::game_repository::GameRepository;
use crate::t;
use anyhow::{Context, Result};

pub struct GameService<R: GameRepository> {
    pub repo: R,
}

impl<R: GameRepository> GameService<R> {
    pub fn process_game_round(&mut self) -> Result<()> {
        let game_schedules = self
            .repo
            .load_game_schedules_to_process()
            .context(t!("error", "function" => "load_game_schedules_to_process"))?;

        // TODO: Check postponement
        for mut game_schedule in game_schedules {
            // TODO: Implement DH case
            let mut game_state = GameState::new(
                game_schedule.away_team.id,
                game_schedule.home_team.id,
                game_schedule.away_team.lineup(false)?,
                game_schedule.home_team.lineup(false)?,
            )?;
            let mut game_result = GameResult::new(
                game_schedule.id,
                game_schedule.planned_date,
                game_schedule.away_team.id,
                game_schedule.home_team.id,
                game_state.away_lineup.batters.clone(),
                game_state.home_lineup.batters.clone(),
            );

            while let GameProgress::Ongoing = game_state.progress() {
                let mut inning = game_state.advance_half_inning();
                let mut inning_state = InningState::new();

                while let InningProgress::Ongoing = inning_state.progress() {
                    let batting_result = game_state.batting_resolve()?;

                    let mut count = inning_state.add_count(&batting_result);

                    game_state.add_point(count.point);
                    game_result
                        .batting_result_histories
                        .push(game_state.add_batting_result_hisrory(count.seq, &batting_result)?);
                    inning.add_count(count);

                    if let GameProgress::WalkOff = game_state.progress() {
                        break;
                    }
                }

                game_result.update_point(&game_state);
                game_result.innings.push(inning);

                if let GameProgress::GameSet = game_state.progress() {
                    break;
                }
            }
            if let Err(e) = self.repo.save_game_result(&game_result) {
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
        Count, GameDetail, GameHeader, GameScheduler, GameType, Inning, TB,
    };
    use crate::domain::shared::game_history::{BattingOrderHistory, BattingResultHistory};
    use crate::domain::shared::player::{DefensiveSkill, Position, RL};
    use crate::domain::shared::team::Team;
    use crate::error::AppError;
    use anyhow::anyhow;
    use chrono::NaiveDate;

    struct RecordingRepo {
        schedules: Vec<GameScheduler>,
        load_error: bool,
        save_error_at: Option<usize>,
        update_error: bool,
        save_calls: usize,
        saved_results: Vec<GameResult>,
        update_calls: usize,
    }

    impl RecordingRepo {
        fn new(schedules: Vec<GameScheduler>) -> Self {
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
        ) -> std::result::Result<Vec<GameScheduler>, AppError> {
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

        fn load_defensive_skills(
            &self,
            _player_id: u32,
        ) -> std::result::Result<Vec<DefensiveSkill>, AppError> {
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

        fn load_batting_order_histories(
            &self,
            _game_id: u32,
        ) -> std::result::Result<Vec<BattingOrderHistory>, AppError> {
            unimplemented!("not used by GameService::process_game_round")
        }

        fn load_batting_result_histories(
            &self,
            _game_id: u32,
        ) -> std::result::Result<Vec<BattingResultHistory>, AppError> {
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
        ];
        let mut player = Player::new(
            id,
            &format!("First{id}"),
            &format!("Last{id}"),
            25,
            RL::Right,
            RL::Right,
            0.0,
            0.0,
        );
        player.defensive_skills = vec![DefensiveSkill {
            position: positions[((id - 1) as usize) % positions.len()].clone(),
            mod_uzr: 0.0,
        }];
        player
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

    fn schedule(id: u32) -> GameScheduler {
        GameScheduler {
            id,
            season: 2026,
            round_seq: 1,
            seq: id as u16,
            planned_date: NaiveDate::from_ymd_opt(2026, 4, id).unwrap(),
            away_team: team(1, "AAA", 1),
            home_team: team(2, "BBB", 101),
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
        assert_eq!(saved.away_points, points_by_inning_type(saved, TB::Top));
        assert_eq!(saved.home_points, points_by_inning_type(saved, TB::Bottom));

        for inning in &saved.innings {
            assert!(inning.seq >= 1);
            assert!(!inning.counts.is_empty());
            for (index, count) in inning.counts.iter().enumerate() {
                assert_eq!(count.seq as usize, index + 1);
                assert!(count.out <= 3);
            }
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
