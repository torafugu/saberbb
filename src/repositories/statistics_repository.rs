use crate::domain::shared::player::Player;
use crate::domain::shared::statistics::BattingStats;
use crate::domain::shared::team::{Standing, Team};
use crate::domain::statistics_service::StatRepository;
use crate::repositories::db::{DbError, SqlDb, SqliteManager};
use crate::t;
use anyhow::{Result, bail};
use deadpool::managed::Object;

#[derive(Clone)]
pub struct SqlStatRepository {
    db: SqlDb,
}
impl SqlStatRepository {
    pub fn new() -> Result<Self> {
        let db = SqlDb::new()?;
        Ok(Self { db })
    }

    pub fn get_conn(&self) -> Result<Object<SqliteManager>, DbError> {
        self.db.get_conn()
    }
}

impl StatRepository for SqlStatRepository {
    fn load_standings(&self) -> Result<Vec<Standing>> {
        let conn = self.get_conn()?;

        let mut stmt = conn.prepare(
            "SELECT 
                    team_id,
                    team_name,
                    SUM(games) AS games,
                    SUM(CASE WHEN result = 'win' THEN 1 ELSE 0 END) AS wins,
                    SUM(CASE WHEN result = 'loss' THEN 1 ELSE 0 END) AS losses,
                    SUM(CASE WHEN result = 'draw' THEN 1 ELSE 0 END) AS draws,
                    COALESCE(ROUND(CAST(SUM(CASE WHEN result = 'win' THEN 1 ELSE 0 END) AS FLOAT) / NULLIF(SUM(games), 0), 3), 0.0) AS pct
                FROM (
                    SELECT 
                        home_team_id AS team_id,
                        t_home.name AS team_name,
                        CASE 
                            WHEN actual_date IS NULL OR actual_date = '1900-01-01' THEN 0 ELSE 1
                        END AS games,
                        CASE 
                            WHEN actual_date IS NULL OR actual_date = '1900-01-01' THEN NULL
                            WHEN home_points > away_points THEN 'win'
                            WHEN home_points < away_points THEN 'loss'
                            ELSE 'draw'
                        END AS result
                    FROM game
                    LEFT JOIN 
		                Team t_home ON game.home_team_id = t_home.id
    
                 UNION ALL
    
                    SELECT 
                        away_team_id AS team_id,
                        t_away.name AS team_name,
                        CASE 
                            WHEN actual_date IS NULL OR actual_date = '1900-01-01' THEN 0 ELSE 1
                        END AS games,
                        CASE 
                            WHEN actual_date IS NULL OR actual_date = '1900-01-01' THEN NULL
                            WHEN away_points > home_points THEN 'win'
                            WHEN away_points < home_points THEN 'loss'
                            ELSE 'draw'
                        END AS result
                    FROM game
                    LEFT JOIN 
		                Team t_away ON game.away_team_id = t_away.id
                ) AS combined_results
                GROUP BY team_id
                ORDER BY pct DESC, wins DESC;",
        )?;

        let standings_iter = stmt.query_map([], |row| {
            Ok(Standing {
                team: Team {
                    id: row.get("team_id")?,
                    name: row.get("team_name")?,
                    players: Vec::new(),
                },
                games: row.get("games")?,
                wins: row.get("wins")?,
                losses: row.get("losses")?,
                draws: row.get("draws")?,
                pct: row.get("pct")?,
                gb: 0.0,
                r: 0,
                ra: 0,
            })
        });

        if let Err(e) = &standings_iter {
            let error_msg = t!("error", "SQL" => "SELECT standings");
            bail!("{}, {}", error_msg, e);
        }

        let standings: Vec<Standing> = standings_iter?.collect::<Result<Vec<_>, _>>()?;
        Ok(standings)
    }

    fn load_batting_stats(&self) -> Result<Vec<BattingStats>> {
        let conn = self.get_conn()?;

        let mut stmt = conn.prepare(
            "SELECT 
                        batter_id,
                        b.first_name AS batter_first_name,
                        b.last_name AS batter_last_name,
                        SUM(1) AS AB,
                        SUM(CASE WHEN result = 'Single' THEN 1 ELSE 0 END) AS single,
                        SUM(CASE WHEN result = 'Double' THEN 1 ELSE 0 END) AS double,
                        SUM(CASE WHEN result = 'Triple' THEN 1 ELSE 0 END) AS triple,
                        SUM(CASE WHEN result = 'HomeRun' THEN 1 ELSE 0 END) AS homeRun,
                        COALESCE(ROUND(CAST(SUM(CASE WHEN result IN ('Single', 'Double', 'Triple', 'HomeRun') THEN 1 ELSE 0 END) AS REAL) / NULLIF(SUM(1), 0), 3), 0.0) AS BA,
                        SUM(point) AS rbi
                    FROM count
                    LEFT JOIN 
                        Player b ON count.batter_id = b.id
                    GROUP BY batter_id
                    ORDER BY batter_id",
        )?;

        let batting_stats_iter = stmt.query_map([], |row| {
            let first_name: String = row.get("batter_first_name")?;
            let last_name: String = row.get("batter_last_name")?;
            Ok(BattingStats {
                batter: Player::min(row.get("batter_id")?, &first_name, &last_name),
                ab: row.get("ab")?,
                single: row.get("single")?,
                double: row.get("double")?,
                triple: row.get("triple")?,
                homerun: row.get("homerun")?,
                ba: row.get("ba")?,
                rbi: row.get("rbi")?,
            })
        });

        if let Err(e) = &batting_stats_iter {
            let error_msg = t!("error", "SQL" => "SELECT batting_stats");
            bail!("{}, {}", error_msg, e);
        }

        let batting_stats: Vec<BattingStats> =
            batting_stats_iter?.collect::<Result<Vec<_>, _>>()?;
        Ok(batting_stats)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use deadpool::managed::Pool;
    use rusqlite::{Connection, params};
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    static TEST_DB_SEQ: AtomicU64 = AtomicU64::new(0);

    type SqlitePool = Pool<SqliteManager>;

    fn test_db_path() -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let seq = TEST_DB_SEQ.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "saberbb-statistics-repository-{}-{nanos}-{seq}.db",
            std::process::id()
        ))
    }

    fn setup_repo() -> (SqlStatRepository, PathBuf) {
        let path = test_db_path();
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(
            "
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
                mod_speed REAL NOT NULL,
                mod_control REAL NOT NULL,
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
            SqlStatRepository {
                db: SqlDb::from_pool(pool),
            },
            path,
        )
    }

    fn conn(repo: &SqlStatRepository) -> deadpool::managed::Object<SqliteManager> {
        repo.get_conn().unwrap()
    }

    fn seed_team(repo: &SqlStatRepository, id: u16, name: &str) {
        conn(repo)
            .execute(
                "INSERT INTO team (id, league_id, name) VALUES (?1, 1, ?2)",
                params![id, name],
            )
            .unwrap();
    }

    fn seed_player(repo: &SqlStatRepository, id: u32, first_name: &str, last_name: &str) {
        conn(repo)
            .execute(
                "INSERT INTO player (
                    id, team_id, first_name, last_name, age, throw,
                    mod_speed, mod_control, bat, mod_ba, mod_slg
                ) VALUES (?1, 1, ?2, ?3, 25, 'Right', 0.0, 0.0, 'Right', 0.0, 0.0)",
                params![id, first_name, last_name],
            )
            .unwrap();
    }

    fn seed_game(
        repo: &SqlStatRepository,
        id: u32,
        away_team_id: u16,
        home_team_id: u16,
        away_points: Option<u8>,
        home_points: Option<u8>,
        actual_date: Option<&str>,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO game (
                    id, season, round_seq, seq, planned_date, actual_date,
                    away_team_id, home_team_id, game_type, away_points, home_points
                ) VALUES (?1, 2026, 1, ?1, '2026-04-01', ?2, ?3, ?4, 'Regular', ?5, ?6)",
                params![
                    id,
                    actual_date,
                    away_team_id,
                    home_team_id,
                    away_points,
                    home_points
                ],
            )
            .unwrap();
    }

    fn seed_count(
        repo: &SqlStatRepository,
        game_id: u32,
        seq: u8,
        batter_id: u32,
        result: &str,
        point: u8,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO count (
                    game_id, inning_seq, inning_tb, seq, batter_id, result, point, out
                ) VALUES (?1, 1, 'Top', ?2, ?3, ?4, ?5, 0)",
                params![game_id, seq, batter_id, result, point],
            )
            .unwrap();
    }

    #[test]
    fn load_standings_returns_team_records_ordered_by_pct_then_wins() {
        let (repo, path) = setup_repo();
        seed_team(&repo, 1, "Aces");
        seed_team(&repo, 2, "Bees");
        seed_team(&repo, 3, "Cats");
        seed_game(&repo, 1, 2, 1, Some(1), Some(3), Some("2026-04-01"));
        seed_game(&repo, 2, 3, 1, Some(2), Some(4), Some("2026-04-02"));
        seed_game(&repo, 3, 2, 3, Some(5), Some(2), Some("2026-04-03"));

        let standings = repo.load_standings().unwrap();

        assert_eq!(standings.len(), 3);
        assert_eq!(standings[0].team.id, 1);
        assert_eq!(standings[0].team.name.as_ref(), "Aces");
        assert_eq!(standings[0].games, 2);
        assert_eq!(standings[0].wins, 2);
        assert_eq!(standings[0].losses, 0);
        assert_eq!(standings[0].draws, 0);
        assert_eq!(standings[0].pct, 1.0);

        assert_eq!(standings[1].team.id, 2);
        assert_eq!(standings[1].games, 2);
        assert_eq!(standings[1].wins, 1);
        assert_eq!(standings[1].losses, 1);
        assert_eq!(standings[1].pct, 0.5);

        assert_eq!(standings[2].team.id, 3);
        assert_eq!(standings[2].games, 2);
        assert_eq!(standings[2].wins, 0);
        assert_eq!(standings[2].losses, 2);
        assert_eq!(standings[2].pct, 0.0);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_standings_counts_draws() {
        let (repo, path) = setup_repo();
        seed_team(&repo, 1, "Away");
        seed_team(&repo, 2, "Home");
        seed_game(&repo, 1, 1, 2, Some(3), Some(3), Some("2026-04-01"));

        let standings = repo.load_standings().unwrap();

        assert_eq!(standings.len(), 2);
        for standing in standings {
            assert_eq!(standing.games, 1);
            assert_eq!(standing.wins, 0);
            assert_eq!(standing.losses, 0);
            assert_eq!(standing.draws, 1);
            assert_eq!(standing.pct, 0.0);
        }
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_standings_ignores_unplayed_games() {
        let (repo, path) = setup_repo();
        seed_team(&repo, 1, "Away");
        seed_team(&repo, 2, "Home");
        seed_game(&repo, 1, 1, 2, None, None, None);

        let standings = repo.load_standings().unwrap();

        assert_eq!(standings.len(), 2);
        for standing in standings {
            assert_eq!(standing.games, 0);
            assert_eq!(standing.wins, 0);
            assert_eq!(standing.losses, 0);
            assert_eq!(standing.draws, 0);
            assert_eq!(standing.pct, 0.0);
        }
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_standings_returns_empty_when_no_games() {
        let (repo, path) = setup_repo();

        let standings = repo.load_standings().unwrap();

        assert!(standings.is_empty());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_batting_stats_aggregates_results_by_batter() {
        let (repo, path) = setup_repo();
        seed_player(&repo, 10, "Shohei", "Ohtani");
        seed_count(&repo, 1, 1, 10, "Single", 1);
        seed_count(&repo, 1, 2, 10, "Double", 2);
        seed_count(&repo, 1, 3, 10, "Triple", 3);
        seed_count(&repo, 1, 4, 10, "HomeRun", 4);
        seed_count(&repo, 1, 5, 10, "Out", 0);

        let stats = repo.load_batting_stats().unwrap();

        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].batter.id, 10);
        assert_eq!(stats[0].batter.first_name.as_ref(), "Shohei");
        assert_eq!(stats[0].batter.last_name.as_ref(), "Ohtani");
        assert_eq!(stats[0].ab, 5);
        assert_eq!(stats[0].single, 1);
        assert_eq!(stats[0].double, 1);
        assert_eq!(stats[0].triple, 1);
        assert_eq!(stats[0].homerun, 1);
        assert_eq!(stats[0].ba, 0.8);
        assert_eq!(stats[0].rbi, 10.0);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_batting_stats_calculates_ba_including_homeruns() {
        let (repo, path) = setup_repo();
        seed_player(&repo, 10, "Shohei", "Ohtani");
        seed_count(&repo, 1, 1, 10, "Single", 0);
        seed_count(&repo, 1, 2, 10, "HomeRun", 1);
        seed_count(&repo, 1, 3, 10, "Out", 0);
        seed_count(&repo, 1, 4, 10, "Out", 0);

        let stats = repo.load_batting_stats().unwrap();

        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].ab, 4);
        assert_eq!(stats[0].homerun, 1);
        assert_eq!(stats[0].ba, 0.5);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_batting_stats_groups_multiple_batters_ordered_by_batter_id() {
        let (repo, path) = setup_repo();
        seed_player(&repo, 10, "First10", "Last10");
        seed_player(&repo, 20, "First20", "Last20");
        seed_count(&repo, 1, 1, 20, "Double", 2);
        seed_count(&repo, 1, 2, 10, "Out", 0);
        seed_count(&repo, 1, 3, 10, "Single", 1);

        let stats = repo.load_batting_stats().unwrap();

        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].batter.id, 10);
        assert_eq!(stats[0].ab, 2);
        assert_eq!(stats[0].single, 1);
        assert_eq!(stats[0].ba, 0.5);
        assert_eq!(stats[1].batter.id, 20);
        assert_eq!(stats[1].ab, 1);
        assert_eq!(stats[1].double, 1);
        assert_eq!(stats[1].ba, 1.0);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_batting_stats_returns_empty_when_no_counts() {
        let (repo, path) = setup_repo();
        seed_player(&repo, 10, "Shohei", "Ohtani");

        let stats = repo.load_batting_stats().unwrap();

        assert!(stats.is_empty());
        std::fs::remove_file(path).ok();
    }
}
