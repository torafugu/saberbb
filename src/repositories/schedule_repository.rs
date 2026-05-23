use crate::domain::shared::game::{GameScheduler, GameSeason};
use crate::domain::shared::team::{League, Team};
use crate::repositories::persistence_config::SqliteManager;
use crate::t;
use anyhow::Result;
use chrono::NaiveDate;
use deadpool::managed::Pool;
use rusqlite::{Error, params};

type DbPool = Pool<SqliteManager>;

pub trait ScheduleRepository {
    fn load_game_season(&self) -> Result<GameSeason>;
    fn load_all_leagues(&self) -> Result<Vec<League>>;
    fn save_game_schedules(&mut self, game_schedules: Vec<GameScheduler>) -> Result<()>;
    fn update_scheduled_season(&self) -> Result<()>;
}

#[derive(Clone)]
pub struct SqlScheduleRepository {
    pub pool: DbPool,
}

impl ScheduleRepository for SqlScheduleRepository {
    fn load_game_season(&self) -> Result<GameSeason> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let start_date = conn.query_row(
            "SELECT season_start_date, scheduled_season + 1 AS scheduled_season FROM game_season LIMIT 1",
            params![],
            |row| {
                Ok(GameSeason {
                    start_date: row.get("season_start_date")?,
                    season: row.get("scheduled_season")?,
                })
            },
        );
        if let Err(e) = &start_date {
            eprintln!("{}:{}", t!("error", "SQL" => "SELECT FROM game_season"), e);
        }
        Ok(start_date?)
    }

    fn load_all_leagues(&self) -> Result<Vec<League>> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let mut stmt_league = conn.prepare("SELECT id, name FROM league ORDER BY id")?;
        let league_iter = stmt_league.query_map([], |row| {
            Ok(League {
                id: row.get("id")?,
                name: row.get("name")?,
                teams: Vec::new(),
            })
        });
        if let Err(e) = &league_iter {
            eprintln!(
                "{}:{}",
                t!("error", "SQL" => "SELECT FROM game_round, game_season"),
                e
            );
        }

        let mut leagues: Vec<League> = Vec::new();

        for league in league_iter? {
            let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

            let mut league = league?;
            let mut stmt_team = conn.prepare("SELECT id, name FROM team WHERE league_id = ?1")?;
            let team_iter = stmt_team.query_map(params![league.id], |row| {
                Ok(Team {
                    id: row.get("id")?,
                    name: row.get("name")?,
                    players: Vec::new(),
                })
            });
            if let Err(e) = &team_iter {
                eprintln!("{}:{}", t!("error", "SQL" => "SELECT FROM team"), e);
            }

            for team in team_iter? {
                league.teams.push(team?);
            }

            leagues.push(league);
        }

        Ok(leagues)
    }

    fn save_game_schedules(&mut self, game_schedules: Vec<GameScheduler>) -> Result<()> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        for game_schedule in game_schedules {
            if let Err(e) = conn.execute(
                "INSERT INTO game (
                season, round_seq, seq, planned_date, away_team_id, home_team_id, game_type
                ) VALUES (
                 ?1, ?2, ?3, ?4, ?5, ?6, ?7
                 )",
                params![
                    game_schedule.season,
                    game_schedule.round_seq,
                    game_schedule.seq,
                    game_schedule.planned_date,
                    game_schedule.away_team.id,
                    game_schedule.home_team.id,
                    game_schedule.game_type,
                ],
            ) {
                eprintln!("{}:{}", t!("error", "SQL" => "INSERT INTO game"), e);
                return Err(e.into());
            };
        }

        Ok(())
    }

    fn update_scheduled_season(&self) -> Result<()> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        if let Err(e) = conn.execute(
            "Update game_season SET scheduled_season = scheduled_season + 1",
            params![],
        ) {
            eprintln!("{}:{}", t!("error", "SQL" => "Update game_season"), e);
            return Err(e.into());
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::game::GameType;
    use rusqlite::Connection;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_DB_SEQ: AtomicU64 = AtomicU64::new(0);

    fn test_db_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEST_DB_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "saberbb-schedule-repository-{}-{nanos}-{seq}.db",
            std::process::id()
        ))
    }

    fn setup_repo() -> (SqlScheduleRepository, PathBuf) {
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

            CREATE TABLE league (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL
            );

            CREATE TABLE team (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                league_id INTEGER NOT NULL,
                name TEXT NOT NULL,
                UNIQUE(league_id, name)
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
            ",
        )
        .unwrap();
        drop(conn);

        let manager = SqliteManager { path: path.clone() };
        let pool: DbPool = Pool::builder(manager).max_size(16).build().unwrap();
        (SqlScheduleRepository { pool }, path)
    }

    fn setup_repo_without_game_table() -> (SqlScheduleRepository, PathBuf) {
        let (repo, path) = setup_repo();
        conn(&repo).execute("DROP TABLE game", []).unwrap();
        (repo, path)
    }

    fn setup_repo_without_game_season_table() -> (SqlScheduleRepository, PathBuf) {
        let (repo, path) = setup_repo();
        conn(&repo).execute("DROP TABLE game_season", []).unwrap();
        (repo, path)
    }

    fn conn(repo: &SqlScheduleRepository) -> deadpool::managed::Object<SqliteManager> {
        futures::executor::block_on(repo.pool.get()).unwrap()
    }

    fn seed_game_season(repo: &SqlScheduleRepository, start_date: &str, scheduled_season: u16) {
        conn(repo)
            .execute(
                "INSERT INTO game_season (
                    season_start_date, scheduled_season, current_season, current_round_seq
                ) VALUES (?1, ?2, ?2, 1)",
                params![start_date, scheduled_season],
            )
            .unwrap();
    }

    fn seed_league(repo: &SqlScheduleRepository, id: u16, name: &str) {
        conn(repo)
            .execute(
                "INSERT INTO league (id, name) VALUES (?1, ?2)",
                params![id, name],
            )
            .unwrap();
    }

    fn seed_team(repo: &SqlScheduleRepository, id: u16, league_id: u16, name: &str) {
        conn(repo)
            .execute(
                "INSERT INTO team (id, league_id, name) VALUES (?1, ?2, ?3)",
                params![id, league_id, name],
            )
            .unwrap();
    }

    fn game_schedule(id: u32, season: u16, round_seq: u16, seq: u16) -> GameScheduler {
        GameScheduler {
            id,
            season,
            round_seq,
            seq,
            planned_date: NaiveDate::from_ymd_opt(2026, 4, seq as u32).unwrap(),
            away_team: Team::min(1, "Away"),
            home_team: Team::min(2, "Home"),
            game_type: GameType::Regular,
        }
    }

    #[test]
    fn load_game_season_returns_next_scheduled_season() {
        let (repo, path) = setup_repo();
        seed_game_season(&repo, "2026-01-01", 2025);

        let game_season = repo.load_game_season().unwrap();

        assert_eq!(
            game_season.start_date,
            NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()
        );
        assert_eq!(game_season.season, 2026);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_game_season_returns_error_when_no_game_season_exists() {
        let (repo, path) = setup_repo();

        let result = repo.load_game_season();

        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_all_leagues_returns_leagues_ordered_by_id() {
        let (repo, path) = setup_repo();
        seed_league(&repo, 2, "Second");
        seed_league(&repo, 1, "First");

        let leagues = repo.load_all_leagues().unwrap();

        assert_eq!(leagues.iter().map(|l| l.id).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(leagues[0].name.as_ref(), "First");
        assert_eq!(leagues[1].name.as_ref(), "Second");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_all_leagues_loads_teams_for_each_league() {
        let (repo, path) = setup_repo();
        seed_league(&repo, 1, "Central");
        seed_league(&repo, 2, "Pacific");
        seed_team(&repo, 1, 1, "Dragons");
        seed_team(&repo, 2, 1, "Tigers");
        seed_team(&repo, 3, 2, "Lions");

        let leagues = repo.load_all_leagues().unwrap();

        assert_eq!(leagues.len(), 2);
        assert_eq!(leagues[0].teams.len(), 2);
        assert_eq!(leagues[0].teams[0].name.as_ref(), "Dragons");
        assert_eq!(leagues[0].teams[1].name.as_ref(), "Tigers");
        assert_eq!(leagues[1].teams.len(), 1);
        assert_eq!(leagues[1].teams[0].name.as_ref(), "Lions");
        assert!(leagues[0].teams[0].players.is_empty());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_all_leagues_returns_empty_when_no_leagues_exist() {
        let (repo, path) = setup_repo();

        let leagues = repo.load_all_leagues().unwrap();

        assert!(leagues.is_empty());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_game_schedules_inserts_all_schedule_fields() {
        let (mut repo, path) = setup_repo();

        repo.save_game_schedules(vec![game_schedule(0, 2026, 2, 3)])
            .unwrap();

        let conn = conn(&repo);
        let row: (u16, u16, u16, String, u16, u16, String) = conn
            .query_row(
                "SELECT season, round_seq, seq, planned_date,
                    away_team_id, home_team_id, game_type
                 FROM game",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, 2026);
        assert_eq!(row.1, 2);
        assert_eq!(row.2, 3);
        assert_eq!(row.3, "2026-04-03");
        assert_eq!(row.4, 1);
        assert_eq!(row.5, 2);
        assert_eq!(row.6, "Regular");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_game_schedules_inserts_multiple_games() {
        let (mut repo, path) = setup_repo();

        repo.save_game_schedules(vec![
            game_schedule(0, 2026, 1, 1),
            game_schedule(0, 2026, 1, 2),
        ])
        .unwrap();

        let count: u16 = conn(&repo)
            .query_row("SELECT COUNT(*) FROM game", [], |row| row.get(0))
            .unwrap();

        assert_eq!(count, 2);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_game_schedules_returns_ok_for_empty_input() {
        let (mut repo, path) = setup_repo();

        let result = repo.save_game_schedules(Vec::new());

        let count: u16 = conn(&repo)
            .query_row("SELECT COUNT(*) FROM game", [], |row| row.get(0))
            .unwrap();
        assert!(result.is_ok());
        assert_eq!(count, 0);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_game_schedules_returns_error_when_game_table_missing() {
        let (mut repo, path) = setup_repo_without_game_table();

        let result = repo.save_game_schedules(vec![game_schedule(0, 2026, 1, 1)]);

        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn update_scheduled_season_increments_by_one() {
        let (repo, path) = setup_repo();
        seed_game_season(&repo, "2026-01-01", 2025);

        repo.update_scheduled_season().unwrap();

        let scheduled_season: u16 = conn(&repo)
            .query_row("SELECT scheduled_season FROM game_season", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(scheduled_season, 2026);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn update_scheduled_season_returns_ok_when_no_rows_exist() {
        let (repo, path) = setup_repo();

        let result = repo.update_scheduled_season();

        assert!(result.is_ok());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn update_scheduled_season_returns_error_when_table_missing() {
        let (repo, path) = setup_repo_without_game_season_table();

        let result = repo.update_scheduled_season();

        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }
}
