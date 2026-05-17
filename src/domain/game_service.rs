use super::shared::game_state::{GameProgress, GameState, InningProgress, InningState, Lineup};
use crate::domain::shared::player::Player;
use crate::repositories::game_repository::GameRepository;
use crate::t;
use anyhow::{Context, Result};

pub struct GameService<R: GameRepository> {
    pub repo: R,
}

impl<R: GameRepository> GameService<R> {
    pub fn process_game_round(&mut self) -> Result<()> {
        // 1. Get game round to process
        let mut game_round = self
            .repo
            .load_game_round_to_process()
            .context(t!("error", "function" => "load_game_round_to_process"))?;

        // 2. Procees games in the game round
        for game in game_round.games.iter_mut() {
            // Initiate the game
            let mut game_state = GameState::new();
            // TODO: Implement No DH case
            game_state.away_batting_lineup = Lineup::new(game.away_team.lineup(true));
            game_state.home_batting_lineup = Lineup::new(game.home_team.lineup(true));

            // TODO: Check postponement
            game.actual_date = game.planned_date;

            // loop for an innning
            while let GameProgress::Ongoing = game_state.progress() {
                let mut inning = game_state.advance_half_inning();
                let mut inning_state = InningState::new();

                // loop for a count
                while let InningProgress::Ongoing = inning_state.progress() {
                    inning_state.add_count_seq();

                    let count = inning_state.batting_resolve(&game_state.current_batter());
                    game_state.add_point(count.point);
                    inning.add_count(count);

                    if let GameProgress::WalkOff = game_state.progress() {
                        break;
                    }
                }

                game.update_point(&game_state);
                game.innings.push(inning);

                if let GameProgress::GameSet = game_state.progress() {
                    break;
                }
            }
        }

        if let Err(e) = self.repo.save_game_round(&game_round) {
            eprintln!("{}:{}", t!("error", "function" => "save_game_round"), e);
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
        fn save_game_round(&mut self, _round: &GameRound) -> Result<()> {
            Ok(())
        }

        fn load_game_round_to_process(&self) -> Result<GameRound> {
            let game_round = GameRound {
                id: 1,
                season: 2026,
                seq: 1,
                date: NaiveDate::parse_from_str("20260101", "%Y%m%d")?,
                games: Vec::new(),
            };
            Ok(game_round)
        }

        fn load_processed_games(&self, _season: u16) -> Result<Vec<Game>> {
            let mut games: Vec<Game> = Vec::new();
            let game = Game {
                id: 1,
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
                away_point: 2,
                home_point: 3,
            };
            games.push(game);
            Ok(games)
        }

        fn load_processed_seasons(&self) -> Result<Vec<u16>> {
            let mut seasons: Vec<u16> = Vec::new();
            seasons.push(2026);
            Ok(seasons)
        }

        fn load_games(&self, _game_round: &GameRound) -> Result<Vec<Game>> {
            let mut games: Vec<Game> = Vec::new();
            let game = Game {
                id: 1,
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
                away_point: 2,
                home_point: 3,
            };
            games.push(game);
            Ok(games)
        }

        fn load_team_players(&self, _team_id: u16) -> Result<Vec<Player>> {
            Ok(Vec::new())
        }

        fn load_innings(&self, _game_id: u32) -> Result<Vec<Inning>> {
            let mut innings = Vec::new();
            Ok(innings)
        }

        fn load_counts(&self, _game_id: u32, _inning: &Inning) -> Result<Vec<Count>> {
            let counts = Vec::new();
            Ok(counts)
        }
    }

    use crate::domain::utils::*;
    use crate::i18n::I18nManager;
    use crate::repositories::game_repository::SqlGameRepository;
    use crate::repositories::persistence_config::SqliteManager;
    use crate::repositories::persistence_config::get_sqlite_manager;
    use deadpool::managed::Pool;
    type DbPool = Pool<SqliteManager>;

    #[test]
    fn test_team_lineup_has_no_duplicated_player_success() {
        let game_round = GameRound {
            id: 1,
            season: 2026,
            seq: 1,
            date: NaiveDate::parse_from_str("20260101", "%Y%m%d").expect(&t!("date_parse_error")),
            games: Vec::new(),
        };

        let manager = get_sqlite_manager().expect(&t!("dbpool_failed"));
        let pool: DbPool = Pool::builder(manager).max_size(16).build().unwrap();
        let game_repository = SqlGameRepository { pool };

        let result = game_repository.load_games(&game_round);

        assert!(result.is_ok(), "Should return Ok");

        let games = result.unwrap();

        assert!(!games.is_empty(), "Games list should not be empty");

        let mut game_state = GameState::new();
        game_state.away_batting_lineup = Lineup::new(games[0].clone().away_team.lineup(true));
        game_state.home_batting_lineup = Lineup::new(games[0].clone().home_team.lineup(true));

        let away_lineup_full_names: Vec<String> = game_state
            .away_batting_lineup
            .batting_orders
            .iter()
            .map(|order| {
                I18nManager::global().full_name(&order.player.first_name, &order.player.last_name)
            })
            .collect();
        assert!(
            has_unique_elements_sorted(away_lineup_full_names),
            "Away line up should not include dupicated player"
        );

        let home_lineup_full_names: Vec<String> = game_state
            .home_batting_lineup
            .batting_orders
            .iter()
            .map(|order| {
                I18nManager::global().full_name(&order.player.first_name, &order.player.last_name)
            })
            .collect();
        assert!(
            has_unique_elements_sorted(home_lineup_full_names),
            "Home line up should not include dupicated player"
        );
    }
}
