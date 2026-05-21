use super::shared::game_state::{GameProgress, GameState, InningProgress, InningState, Lineup};
use crate::domain::shared::{game::GameResult, player::Player};
use crate::repositories::game_repository::GameRepository;
use crate::t;
use anyhow::{Context, Result};

pub struct GameService<R: GameRepository> {
    pub repo: R,
}

impl<R: GameRepository> GameService<R> {
    pub fn process_game_round(&mut self) -> Result<()> {
        // 1. Get game round to process
        let game_schedules = self
            .repo
            .load_game_schedules_to_process()
            .context(t!("error", "function" => "load_game_schedules_to_process"))?;

        // TODO: Check postponement
        // 2. Procees games in the game round
        for mut game_schedule in game_schedules {
            // Initiate the game
            let mut game_state = GameState::new();
            let mut game_result = GameResult {
                id: game_schedule.id,
                actual_date: game_schedule.planned_date,
                innings: Vec::new(),
                away_points: 0,
                home_points: 0,
            };

            // TODO: Implement No DH case
            game_state.away_batters = Lineup::new(game_schedule.away_team.lineup(true));
            game_state.home_batters = Lineup::new(game_schedule.home_team.lineup(true));

            // loop for an innning
            while let GameProgress::Ongoing = game_state.progress() {
                let mut inning = game_state.advance_half_inning();
                let mut inning_state = InningState::new();

                // loop for a count
                while let InningProgress::Ongoing = inning_state.progress() {
                    inning_state.add_count_seq();

                    let count = inning_state.batting_resolve(
                        &game_state.current_batter(),
                        &game_state.current_fielders(),
                    );
                    game_state.add_point(count.point);
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

        if let Err(e) = self.repo.updated_game_result() {
            eprintln!("{}:{}", t!("error", "function" => "updated_game_result"), e);
            return Err(e.into());
        }

        Ok(())
    }
}

mod tests {
    use super::*;
    use crate::domain::shared::game::*;
    use crate::domain::shared::team::*;

    use chrono::NaiveDate;
    struct MockRepo;
    impl GameRepository for MockRepo {
        fn save_game_result(&mut self, _round: &GameResult) -> Result<()> {
            Ok(())
        }

        fn updated_game_result(&mut self) -> Result<()> {
            Ok(())
        }

        fn load_processed_seasons(&self) -> Result<Vec<u16>> {
            let mut seasons: Vec<u16> = Vec::new();
            seasons.push(2026);
            Ok(seasons)
        }

        fn load_processed_game_headers(&self, _season: u16) -> Result<Vec<GameHeader>> {
            let mut game_headers: Vec<GameHeader> = Vec::new();
            let game_header = GameHeader {
                id: 1,
                actual_date: NaiveDate::parse_from_str("20250101", "%Y%m%d")?,
                game_type: GameType::Regular,
                away_team: Team {
                    id: 1,
                    name: "AAA".into(),
                    players: Vec::new(),
                },
                home_team: Team {
                    id: 2,
                    name: "BBB".into(),
                    players: Vec::new(),
                },
                away_points: 2,
                home_points: 3,
            };
            game_headers.push(game_header);
            Ok(game_headers)
        }

        fn load_game_schedules_to_process(&self) -> Result<Vec<GameScheduler>> {
            let mut game_schedules: Vec<GameScheduler> = Vec::new();
            let game_schedule = GameScheduler {
                id: 1,
                season: 2026,
                round_seq: 1,
                seq: 1,
                planned_date: NaiveDate::parse_from_str("20250101", "%Y%m%d")?,
                away_team: Team {
                    id: 1,
                    name: "AAA".into(),
                    players: Vec::new(),
                },
                home_team: Team {
                    id: 2,
                    name: "BBB".into(),
                    players: Vec::new(),
                },
                game_type: GameType::Regular,
            };
            game_schedules.push(game_schedule);
            Ok(game_schedules)
        }

        fn load_game_row(&self, _game_header: &GameHeader) -> Result<GameRow> {
            let game = GameRow {
                id: 1,
                season: 2026,
                round_seq: 1,
                seq: 1,
                planned_date: NaiveDate::parse_from_str("20250101", "%Y%m%d")?,
                actual_date: NaiveDate::parse_from_str("20250101", "%Y%m%d")?,
                away_team: Team {
                    id: 1,
                    name: "AAA".into(),
                    players: Vec::new(),
                },
                home_team: Team {
                    id: 2,
                    name: "BBB".into(),
                    players: Vec::new(),
                },
                game_type: GameType::Regular,
                innings: Vec::new(),
                away_points: 2,
                home_points: 3,
            };
            Ok(game)
        }

        fn load_team_players(&self, _team: &Team) -> Result<Vec<Player>> {
            Ok(Vec::new())
        }

        fn load_innings(&self, _game: &GameRow) -> Result<Vec<Inning>> {
            let innings = Vec::new();
            Ok(innings)
        }

        fn load_counts(&self, _game: &GameRow, _inning: &Inning) -> Result<Vec<Count>> {
            let counts = Vec::new();
            Ok(counts)
        }
    }

    use crate::domain::shared::game::GameRow;
    // use crate::domain::utils::*;
    // use crate::i18n::I18nManager;
    // use crate::repositories::game_repository::SqlGameRepository;
    use crate::repositories::persistence_config::SqliteManager;
    // use crate::repositories::persistence_config::get_sqlite_manager;
    use deadpool::managed::Pool;
    type DbPool = Pool<SqliteManager>;

    // #[test]
    // fn test_team_lineup_has_no_duplicated_player_success() {
    //     let manager = get_sqlite_manager().expect(&t!("dbpool_failed"));
    //     let pool: DbPool = Pool::builder(manager).max_size(16).build().unwrap();
    //     let game_repository = SqlGameRepository { pool };

    //     let result = game_repository.load_games(&game_round);

    //     assert!(result.is_ok(), "Should return Ok");

    //     let games = result.unwrap();

    //     assert!(!games.is_empty(), "Games list should not be empty");

    //     let mut game_state = GameState::new();
    //     game_state.away_batters = Lineup::new(games[0].clone().away_team.lineup(true));
    //     game_state.home_batters = Lineup::new(games[0].clone().home_team.lineup(true));

    //     let away_lineup_full_names: Vec<String> = game_state
    //         .away_batters
    //         .batters
    //         .iter()
    //         .map(|order| I18nManager::global().full_name(&order.first_name, &order.last_name))
    //         .collect();
    //     assert!(
    //         has_unique_elements_sorted(away_lineup_full_names),
    //         "Away line up should not include dupicated player"
    //     );

    //     let home_lineup_full_names: Vec<String> = game_state
    //         .home_batters
    //         .batters
    //         .iter()
    //         .map(|order| I18nManager::global().full_name(&order.first_name, &order.last_name))
    //         .collect();
    //     assert!(
    //         has_unique_elements_sorted(home_lineup_full_names),
    //         "Home line up should not include dupicated player"
    //     );
    // }
}
