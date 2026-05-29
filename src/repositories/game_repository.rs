use crate::domain::shared::game::{
    Count, GameHeader, GameResult, GameRow, GameScheduler, Inning, TB,
};
use crate::domain::shared::player::Player;
use crate::error::AppError;
use crate::repositories::db::{DbClient, SqlDb};
use anyhow::Result;
use rusqlite::params;

pub trait GameRepository {
    fn save_game_result(&mut self, game: &GameResult) -> Result<(), AppError>;
    fn updated_game_result(&mut self) -> Result<usize, AppError>;
    fn load_processed_seasons(&self) -> Result<Vec<u16>, AppError>;
    fn load_processed_game_headers(&self, season: u16) -> Result<Vec<GameHeader>, AppError>;
    fn load_game_schedules_to_process(&self) -> Result<Vec<GameScheduler>, AppError>;
    fn load_game_row(&self, game: &GameHeader) -> Result<GameRow, AppError>;
    fn load_team_players(&self, team_id: u16) -> Result<Vec<Player>, AppError>;
    fn load_innings(&self, game_id: u32) -> Result<Vec<Inning>, AppError>;
    fn load_counts(
        &self,
        game_id: u32,
        inning_seq: u8,
        inning_tb: TB,
    ) -> Result<Vec<Count>, AppError>;
}

#[derive(Clone)]
pub struct SqlGameRepository {
    db_client: DbClient,
}

impl SqlGameRepository {
    pub fn new() -> Result<Self> {
        let db_client = DbClient { db: SqlDb::new()? };
        Ok(Self { db_client })
    }
}
impl GameRepository for SqlGameRepository {
    fn save_game_result(&mut self, game: &GameResult) -> Result<(), AppError> {
        self.db_client.transaction(|tx| {
            let update_game_sql =
                "UPDATE game SET actual_date = ?1, away_points = ?2, home_points = ?3 WHERE id = ?4";
            self.db_client.execute_tx(
                tx,
                update_game_sql,
                params![
                    game.actual_date,
                    game.away_points,
                    game.home_points,
                    game.id
                ],
            )?;

            let insert_inning_sql = "INSERT INTO inning (game_id, seq, tb) VALUES (?1, ?2, ?3)";
            let insert_count_sql = "INSERT INTO count (
                            game_id, inning_seq, inning_tb, seq, bases_occupied, 
                            pitcher_id, catcher_id, 
                            first_baseman_id, second_baseman_id, third_baseman_id, shortstop_id, 
                            left_fielder_id, center_fielder_id, right_fielder_id, 
                            batter_id, 
                            result, point, out
                            ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, 
                            ?6, ?7, 
                            ?8, ?9, ?10, ?11, 
                            ?12, ?13, ?14, 
                            ?15,
                            ?16, ?17, ?18)";

            for inning in &game.innings {
                self.db_client.execute_tx(
                    tx,
                    insert_inning_sql,
                    params![game.id, inning.seq, inning.tb],
                )?;

                for count in &inning.counts {
                    self.db_client.execute_tx(
                        tx,
                        insert_count_sql,
                        params![
                            game.id,
                            inning.seq,
                            inning.tb,
                            count.seq,
                            count.bases_occupied,
                            count.pitcher.id,
                            count.catcher.id,
                            count.first_baseman.id,
                            count.second_baseman.id,
                            count.third_baseman.id,
                            count.shortstop.id,
                            count.left_fielder.id,
                            count.center_fielder.id,
                            count.right_fielder.id,
                            count.batter.id,
                            count.result,
                            count.point,
                            count.out
                        ],
                    )?;
                }
            }
            Ok(())
        })
    }

    fn updated_game_result(&mut self) -> Result<usize, AppError> {
        let update_game_season_sql =
            "UPDATE game_season SET current_round_seq = current_round_seq + 1";
        self.db_client.execute(update_game_season_sql, params![])
    }

    fn load_processed_seasons(&self) -> Result<Vec<u16>, AppError> {
        let query = "SELECT season 
                    FROM game
                    INNER JOIN 
                    game_season ON current_season >= season 
                    GROUP BY season
                    ORDER BY season";
        self.db_client.query_rows::<u16>(query, params![])
    }

    fn load_processed_game_headers(&self, season: u16) -> Result<Vec<GameHeader>, AppError> {
        let query = "SELECT 
                            g.id,
                            g.actual_date,
                            g.away_team_id, 
                            t_away.name AS away_team_name,
                            g.home_team_id, 
                            t_home.name AS home_team_name,
                            g.game_type,
                            g.away_points,
                            g.home_points
                            FROM game g
                            INNER JOIN game_season
                    	        ON current_round_seq >= round_seq
					        LEFT JOIN 
                		        team t_away ON g.away_team_id = t_away.id
            		        LEFT JOIN 
                		        team t_home ON g.home_team_id = t_home.id
                            WHERE season = ?1 AND actual_date IS NOT NULL
                            ORDER BY round_seq, seq DESC";
        self.db_client
            .query_rows::<GameHeader>(query, params![season])
    }

    fn load_game_schedules_to_process(&self) -> Result<Vec<GameScheduler>, AppError> {
        let query = "SELECT 
                            g.id,
                            g.season,
                            g.round_seq,
                            g.seq,
                            planned_date,
                            g.away_team_id, 
                            t_away.name AS away_team_name,
                            g.home_team_id, 
                            t_home.name AS home_team_name,
                            g.game_type
                            FROM game g
                            INNER JOIN game_season s
                    	        ON s.current_season = g.season
						        AND s.current_round_seq = g.round_seq
					        LEFT JOIN 
                		        team t_away ON g.away_team_id = t_away.id
            		        LEFT JOIN 
                		        team t_home ON g.home_team_id = t_home.id
                            ORDER BY round_seq, seq DESC";
        let mut game_schedules = self
            .db_client
            .query_rows::<GameScheduler>(query, params![])?;

        for game_schedule in &mut game_schedules {
            game_schedule.away_team.players = self.load_team_players(game_schedule.away_team.id)?;
            game_schedule.home_team.players = self.load_team_players(game_schedule.home_team.id)?;
        }
        Ok(game_schedules)
    }

    fn load_game_row(&self, game_header: &GameHeader) -> Result<GameRow, AppError> {
        let query = "SELECT 
                g.id,
                g.season,
    			g.round_seq,
    			g.seq,
                g.planned_date,
                g.actual_date,
                g.away_team_id, 
                t_away.name AS away_team_name,
                g.home_team_id, 
                t_home.name AS home_team_name,
                g.game_type,
                g.away_points,
                g.home_points
                FROM game g
                LEFT JOIN 
                    team t_away ON g.away_team_id = t_away.id
                LEFT JOIN 
                    team t_home ON g.home_team_id = t_home.id
                WHERE g.id = ?1 
                ORDER BY g.id";
        let mut game = self
            .db_client
            .query_row::<GameRow>(query, params![game_header.id])?;

        game.away_team.players = self.load_team_players(game.away_team.id)?;
        game.home_team.players = self.load_team_players(game.home_team.id)?;

        game.innings = self.load_innings(game.id)?;
        for inning in &mut game.innings {
            inning.counts = self.load_counts(game.id, inning.seq, inning.tb)?;
        }

        Ok(game)
    }

    fn load_team_players(&self, team_id: u16) -> Result<Vec<Player>, AppError> {
        let query =
            "SELECT id, first_name, last_name, mod_ba, mod_slg FROM player WHERE team_id = ?1";
        self.db_client.query_rows::<Player>(query, params![team_id])
    }

    fn load_innings(&self, game_id: u32) -> Result<Vec<Inning>, AppError> {
        let query =
            "SELECT seq, tb FROM inning WHERE game_id = ?1 ORDER BY game_id ASC, seq ASC, tb DESC";
        self.db_client.query_rows::<Inning>(query, params![game_id])
    }

    fn load_counts(
        &self,
        game_id: u32,
        inning_seq: u8,
        inning_tb: TB,
    ) -> Result<Vec<Count>, AppError> {
        let query = "SELECT seq, bases_occupied, result, point, out, 
                                b.id as b_id, b.first_name as b_first_name, b.last_name as b_last_name, b.mod_ba as ba, b.mod_slg as slg, 
                                p.id as p_id, p.first_name as p_first_name, p.last_name as p_last_name,
                                c.id as c_id, c.first_name as c_first_name, c.last_name as c_last_name,
                                fb.id as fb_id, fb.first_name as fb_first_name, fb.last_name as fb_last_name,
                                sb.id as sb_id, sb.first_name as sb_first_name, sb.last_name as sb_last_name,
                                tb.id as tb_id, tb.first_name as tb_first_name, tb.last_name as tb_last_name,
                                ss.id as ss_id, ss.first_name as ss_first_name, ss.last_name as ss_last_name,
                                lf.id as lf_id, lf.first_name as lf_first_name, lf.last_name as lf_last_name,
                                cf.id as cf_id, cf.first_name as cf_first_name, cf.last_name as cf_last_name,
                                rf.id as rf_id, rf.first_name as rf_first_name, rf.last_name as rf_last_name
                                FROM count
                                INNER JOIN player AS b
                                    ON count.batter_id = b.id
                                INNER JOIN player AS p
                                    ON count.pitcher_id = p.id
                                INNER JOIN player AS c
                                    ON count.catcher_id = c.id
                                INNER JOIN player AS fb
                                    ON count.first_baseman_id = fb.id
                                INNER JOIN player AS sb
                                    ON count.second_baseman_id = sb.id
                                INNER JOIN player AS tb
                                    ON count.third_baseman_id = tb.id
                                INNER JOIN player AS ss
                                    ON count.shortstop_id = ss.id
                                INNER JOIN player AS lf
                                    ON count.left_fielder_id = lf.id
                                INNER JOIN player AS cf
                                    ON count.center_fielder_id = cf.id
                                INNER JOIN player AS rf
                                    ON count.right_fielder_id = rf.id 
                                WHERE game_id = ?1 AND inning_seq = ?2 AND inning_tb = ?3";
        self.db_client
            .query_rows::<Count>(query, params![game_id, inning_seq, inning_tb])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::game::{BattingResult, GameHeader, GameType, TB};
    use crate::domain::shared::team::Team;
    use crate::repositories::db::{DbClient, SqliteManager};
    use deadpool::managed::Pool;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_DB_SEQ: AtomicU64 = AtomicU64::new(0);

    pub type SqlitePool = Pool<SqliteManager>;

    fn test_db_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEST_DB_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "saberbb-game-repository-{}-{nanos}-{seq}.db",
            std::process::id()
        ))
    }

    fn setup_repo() -> (SqlGameRepository, PathBuf) {
        let path = test_db_path();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "
            CREATE TABLE game_season (
                season_start_date TEXT NOT NULL,
                scheduled_season INTEGER NOT NULL,
                current_season INTEGER NOT NULL,
                current_round_seq INTEGER NOT NULL
            );

            CREATE TABLE team (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                league_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                UNIQUE(league_id, name)
            );

            CREATE TABLE player (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                team_id INTEGER NOT NULL,
                first_name TEXT NOT NULL,
                last_name TEXT NOT NULL,
                age INTEGER NOT NULL,
                throw TEXT NOT NULL,
                bat TEXT NOT NULL,
                mod_ba REAL NOT NULL,
                mod_slg REAL NOT NULL
            );

            CREATE TABLE game (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                season INTEGER,
                round_seq INTEGER,
                seq INTEGER,
                planned_date TEXT NOT NULL,
                actual_date TEXT,
                away_team_id INTEGER NOT NULL,
                home_team_id INTEGER NOT NULL,
                game_type TEXT NOT NULL,
                away_points INTEGER,
                home_points INTEGER
            );

            CREATE TABLE inning (
                game_id INTEGER,
                seq INTEGER,
                tb TEXT,
                PRIMARY KEY (game_id, seq, tb)
            );

            CREATE TABLE count (
                game_id INTEGER,
                inning_seq INTEGER,
                inning_tb TEXT,
                seq INTEGER,
                bases_occupied INTEGER NOT NULL DEFAULT 0,
                pitcher_id INTEGER,
                catcher_id INTEGER,
                first_baseman_id INTEGER,
                second_baseman_id INTEGER,
                third_baseman_id INTEGER,
                shortstop_id INTEGER,
                left_fielder_id INTEGER,
                center_fielder_id INTEGER,
                right_fielder_id INTEGER,
                batter_id INTEGER,
                result TEXT NOT NULL,
                point INTEGER NOT NULL,
                out INTEGER NOT NULL,
                PRIMARY KEY (game_id, inning_seq, inning_tb, seq)
            );
            ",
        )
        .unwrap();
        drop(conn);

        let manager = SqliteManager::from_path(path.clone());
        let pool: SqlitePool = Pool::builder(manager).max_size(16).build().unwrap();
        (
            SqlGameRepository {
                db_client: DbClient {
                    db: SqlDb::from_pool(pool),
                },
            },
            path,
        )
    }

    fn conn(repo: &SqlGameRepository) -> deadpool::managed::Object<SqliteManager> {
        repo.db_client.get_conn().unwrap()
    }

    fn seed_game_season(repo: &SqlGameRepository, current_season: u16, current_round_seq: u16) {
        conn(repo)
            .execute(
                "INSERT INTO game_season (
                    season_start_date, scheduled_season, current_season, current_round_seq
                ) VALUES ('2026-01-01', ?1, ?2, ?3)",
                params![current_season, current_season, current_round_seq],
            )
            .unwrap();
    }

    fn seed_teams(repo: &SqlGameRepository) {
        let conn = conn(repo);
        conn.execute(
            "INSERT INTO team (id, league_id, name) VALUES (1, 1, 'Away')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO team (id, league_id, name) VALUES (2, 1, 'Home')",
            [],
        )
        .unwrap();
    }

    fn seed_players(repo: &SqlGameRepository) {
        let conn = conn(repo);
        for id in 1..=18 {
            let team_id = if id <= 9 { 1 } else { 2 };
            conn.execute(
                "INSERT INTO player (
                    id, team_id, first_name, last_name, age, throw,
                    bat, mod_ba, mod_slg
                ) VALUES (?1, ?2, ?3, ?4, 25, 'Right', 'Right', ?5, ?6)",
                params![
                    id,
                    team_id,
                    format!("First{id}"),
                    format!("Last{id}"),
                    id as f64 / 100.0,
                    id as f64 / 50.0,
                ],
            )
            .unwrap();
        }
    }

    fn seed_game(
        repo: &SqlGameRepository,
        id: u32,
        season: u16,
        round_seq: u16,
        seq: u16,
        actual_date: Option<&str>,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO game (
                    id, season, round_seq, seq, planned_date, actual_date,
                    away_team_id, home_team_id, game_type, away_points, home_points
                ) VALUES (?1, ?2, ?3, ?4, '2026-04-01', ?5, 1, 2, 'Regular', 3, 2)",
                params![id, season, round_seq, seq, actual_date],
            )
            .unwrap();
    }

    fn seed_inning(repo: &SqlGameRepository, game_id: u32, seq: u8, tb: &str) {
        conn(repo)
            .execute(
                "INSERT INTO inning (game_id, seq, tb) VALUES (?1, ?2, ?3)",
                params![game_id, seq, tb],
            )
            .unwrap();
    }

    fn seed_count(repo: &SqlGameRepository, game_id: u32, inning_seq: u8, inning_tb: &str) {
        conn(repo)
            .execute(
                "INSERT INTO count (
                    game_id, inning_seq, inning_tb, seq, bases_occupied,
                    pitcher_id, catcher_id, first_baseman_id, second_baseman_id,
                    third_baseman_id, shortstop_id, left_fielder_id, center_fielder_id,
                    right_fielder_id, batter_id, result, point, out
                ) VALUES (?1, ?2, ?3, 1, 5, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 'Double', 2, 1)",
                params![game_id, inning_seq, inning_tb],
            )
            .unwrap();
    }

    #[test]
    fn load_processed_seasons_returns_only_processed_seasons() {
        let (repo, path) = setup_repo();
        seed_game_season(&repo, 2026, 1);
        seed_teams(&repo);
        seed_game(&repo, 1, 2025, 1, 1, Some("2025-04-01"));
        seed_game(&repo, 2, 2026, 1, 1, Some("2026-04-01"));
        seed_game(&repo, 3, 2027, 1, 1, Some("2027-04-01"));

        let seasons = repo.load_processed_seasons().unwrap();

        assert_eq!(seasons, vec![2025, 2026]);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_processed_game_headers_returns_completed_games_for_season() {
        let (repo, path) = setup_repo();
        seed_game_season(&repo, 2026, 1);
        seed_teams(&repo);
        seed_game(&repo, 1, 2026, 1, 2, Some("2026-04-01"));
        seed_game(&repo, 2, 2026, 1, 1, None);
        seed_game(&repo, 3, 2025, 1, 1, Some("2025-04-01"));

        let games = repo.load_processed_game_headers(2026).unwrap();

        assert_eq!(games.len(), 1);
        assert_eq!(games[0].id, 1);
        assert_eq!(games[0].actual_date.to_string(), "2026-04-01");
        assert_eq!(games[0].away_team.id, 1);
        assert_eq!(games[0].away_team.name.as_ref(), "Away");
        assert_eq!(games[0].home_team.id, 2);
        assert_eq!(games[0].home_team.name.as_ref(), "Home");
        assert!(matches!(games[0].game_type, GameType::Regular));
        assert_eq!(games[0].away_points, 3);
        assert_eq!(games[0].home_points, 2);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_processed_game_headers_returns_empty_for_unknown_season() {
        let (repo, path) = setup_repo();
        seed_game_season(&repo, 2026, 1);
        seed_teams(&repo);
        seed_game(&repo, 1, 2026, 1, 1, Some("2026-04-01"));

        let games = repo.load_processed_game_headers(9999).unwrap();

        assert!(games.is_empty());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_game_schedules_to_process_returns_current_round_with_players() {
        let (repo, path) = setup_repo();
        seed_game_season(&repo, 2026, 2);
        seed_teams(&repo);
        seed_players(&repo);
        seed_game(&repo, 1, 2026, 1, 1, None);
        seed_game(&repo, 2, 2026, 2, 1, None);
        seed_game(&repo, 3, 2027, 2, 1, None);

        let schedules = repo.load_game_schedules_to_process().unwrap();

        assert_eq!(schedules.len(), 1);
        assert_eq!(schedules[0].id, 2);
        assert_eq!(schedules[0].season, 2026);
        assert_eq!(schedules[0].round_seq, 2);
        assert_eq!(schedules[0].away_team.players.len(), 9);
        assert_eq!(schedules[0].home_team.players.len(), 9);
        assert_eq!(schedules[0].away_team.players[0].id, 1);
        assert_eq!(schedules[0].home_team.players[0].id, 10);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_game_row_loads_game_teams_innings_and_counts() {
        let (repo, path) = setup_repo();
        seed_teams(&repo);
        seed_players(&repo);
        seed_game(&repo, 1, 2026, 1, 1, Some("2026-04-01"));
        seed_inning(&repo, 1, 1, "Top");
        seed_count(&repo, 1, 1, "Top");
        let header = GameHeader {
            id: 1,
            actual_date: "2026-04-01".parse().unwrap(),
            away_team: Team::min(1, "Away"),
            home_team: Team::min(2, "Home"),
            game_type: GameType::Regular,
            away_points: 3,
            home_points: 2,
        };

        let game = repo.load_game_row(&header).unwrap();

        assert_eq!(game.id, 1);
        assert_eq!(game.away_team.players.len(), 9);
        assert_eq!(game.home_team.players.len(), 9);
        assert_eq!(game.innings.len(), 1);
        assert_eq!(game.innings[0].seq, 1);
        assert!(matches!(game.innings[0].tb, TB::Top));
        assert_eq!(game.innings[0].counts.len(), 1);
        let count = &game.innings[0].counts[0];
        assert_eq!(count.seq, 1);
        assert_eq!(count.bases_occupied, 5);
        assert!(matches!(count.result, BattingResult::Double));
        assert_eq!(count.pitcher.id, 1);
        assert_eq!(count.catcher.id, 2);
        assert_eq!(count.first_baseman.id, 3);
        assert_eq!(count.second_baseman.id, 4);
        assert_eq!(count.third_baseman.id, 5);
        assert_eq!(count.shortstop.id, 6);
        assert_eq!(count.left_fielder.id, 7);
        assert_eq!(count.center_fielder.id, 8);
        assert_eq!(count.right_fielder.id, 9);
        assert_eq!(count.batter.id, 10);
        assert_eq!(count.point, 2);
        assert_eq!(count.out, 1);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_innings_orders_by_sequence_and_half() {
        let (repo, path) = setup_repo();
        seed_teams(&repo);
        seed_game(&repo, 1, 2026, 1, 1, Some("2026-04-01"));
        seed_inning(&repo, 1, 2, "Top");
        seed_inning(&repo, 1, 1, "Bottom");
        seed_inning(&repo, 1, 1, "Top");

        let innings = repo.load_innings(1).unwrap();

        assert_eq!(innings.len(), 3);
        assert_eq!(innings[0].seq, 1);
        assert!(matches!(innings[0].tb, TB::Top));
        assert_eq!(innings[1].seq, 1);
        assert!(matches!(innings[1].tb, TB::Bottom));
        assert_eq!(innings[2].seq, 2);
        assert!(matches!(innings[2].tb, TB::Top));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_game_result_updates_game_and_inserts_innings_counts() {
        let (mut repo, path) = setup_repo();
        seed_teams(&repo);
        seed_players(&repo);
        seed_game(&repo, 1, 2026, 1, 1, None);
        let game = GameResult {
            id: 1,
            actual_date: "2026-04-01".parse().unwrap(),
            away_points: 4,
            home_points: 3,
            innings: vec![Inning {
                seq: 1,
                tb: TB::Top,
                counts: vec![Count {
                    seq: 1,
                    bases_occupied: 3,
                    pitcher: Arc::new(Player::min(1, "First1", "Last1")),
                    catcher: Arc::new(Player::min(2, "First2", "Last2")),
                    first_baseman: Arc::new(Player::min(3, "First3", "Last3")),
                    second_baseman: Arc::new(Player::min(4, "First4", "Last4")),
                    third_baseman: Arc::new(Player::min(5, "First5", "Last5")),
                    shortstop: Arc::new(Player::min(6, "First6", "Last6")),
                    left_fielder: Arc::new(Player::min(7, "First7", "Last7")),
                    center_fielder: Arc::new(Player::min(8, "First8", "Last8")),
                    right_fielder: Arc::new(Player::min(9, "First9", "Last9")),
                    batter: Arc::new(Player::min(10, "First10", "Last10")),
                    result: BattingResult::Single,
                    point: 1,
                    out: 0,
                }],
            }],
        };

        repo.save_game_result(&game).unwrap();

        let conn = conn(&repo);
        let (actual_date, away_points, home_points): (String, u8, u8) = conn
            .query_row(
                "SELECT actual_date, away_points, home_points FROM game WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(actual_date, "2026-04-01");
        assert_eq!(away_points, 4);
        assert_eq!(home_points, 3);

        let innings: u8 = conn
            .query_row("SELECT COUNT(*) FROM inning WHERE game_id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let counts: u8 = conn
            .query_row("SELECT COUNT(*) FROM count WHERE game_id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(innings, 1);
        assert_eq!(counts, 1);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_game_result_rolls_back_when_count_insert_fails() {
        let (mut repo, path) = setup_repo();
        seed_teams(&repo);
        seed_players(&repo);
        seed_game(&repo, 1, 2026, 1, 1, None);
        let count = Count {
            seq: 1,
            bases_occupied: 0,
            pitcher: Arc::new(Player::min(1, "First1", "Last1")),
            catcher: Arc::new(Player::min(2, "First2", "Last2")),
            first_baseman: Arc::new(Player::min(3, "First3", "Last3")),
            second_baseman: Arc::new(Player::min(4, "First4", "Last4")),
            third_baseman: Arc::new(Player::min(5, "First5", "Last5")),
            shortstop: Arc::new(Player::min(6, "First6", "Last6")),
            left_fielder: Arc::new(Player::min(7, "First7", "Last7")),
            center_fielder: Arc::new(Player::min(8, "First8", "Last8")),
            right_fielder: Arc::new(Player::min(9, "First9", "Last9")),
            batter: Arc::new(Player::min(10, "First10", "Last10")),
            result: BattingResult::Single,
            point: 1,
            out: 0,
        };
        let game = GameResult {
            id: 1,
            actual_date: "2026-04-01".parse().unwrap(),
            away_points: 4,
            home_points: 3,
            innings: vec![Inning {
                seq: 1,
                tb: TB::Top,
                counts: vec![count.clone(), count],
            }],
        };

        assert!(repo.save_game_result(&game).is_err());

        let conn = conn(&repo);
        let (actual_date, away_points, home_points): (Option<String>, u8, u8) = conn
            .query_row(
                "SELECT actual_date, away_points, home_points FROM game WHERE id = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(actual_date, None);
        assert_eq!(away_points, 3);
        assert_eq!(home_points, 2);

        let innings: u8 = conn
            .query_row("SELECT COUNT(*) FROM inning WHERE game_id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        let counts: u8 = conn
            .query_row("SELECT COUNT(*) FROM count WHERE game_id = 1", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(innings, 0);
        assert_eq!(counts, 0);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn updated_game_result_increments_current_round_seq() {
        let (mut repo, path) = setup_repo();
        seed_game_season(&repo, 2026, 7);

        repo.updated_game_result().unwrap();

        let current_round_seq: u16 = conn(&repo)
            .query_row("SELECT current_round_seq FROM game_season", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(current_round_seq, 8);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_team_players_returns_batter_players_for_team() {
        let (repo, path) = setup_repo();
        seed_teams(&repo);
        seed_players(&repo);

        let players = repo.load_team_players(2).unwrap();

        assert_eq!(players.len(), 9);
        assert_eq!(players[0].id, 10);
        assert_eq!(players[0].first_name.as_ref(), "First10");
        assert_eq!(players[0].last_name.as_ref(), "Last10");
        assert_eq!(players[0].mod_ba, 0.10);
        assert_eq!(players[0].mod_slg, 0.20);
        std::fs::remove_file(path).ok();
    }
}
