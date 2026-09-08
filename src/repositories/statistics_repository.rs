use crate::domain::shared::stats::Standing;
use crate::domain::shared::stats::{BattingStats, PitchingStats};
use crate::error::AppError;
use crate::repositories::db::{DbClient, SqlDb};
use anyhow::Result;
use rusqlite::params;

pub trait StatRepository {
    fn load_standings(&self) -> Result<Vec<Standing>, AppError>;
    fn load_batting_stats(&self) -> Result<Vec<BattingStats>, AppError>;
    fn load_pitching_stats(&self) -> Result<Vec<PitchingStats>, AppError>;
}

#[derive(Clone)]
pub struct SqlStatRepository {
    db_client: DbClient,
}
impl SqlStatRepository {
    pub fn new() -> Result<Self> {
        let db_client = DbClient { db: SqlDb::new()? };
        Ok(Self { db_client })
    }
}

impl StatRepository for SqlStatRepository {
    #[tracing::instrument(skip(self), err)]
    fn load_standings(&self) -> Result<Vec<Standing>, AppError> {
        let query = "SELECT 
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
                            ORDER BY pct DESC, wins DESC";
        self.db_client.query_rows::<Standing>(query, params![])
    }

    fn load_batting_stats(&self) -> Result<Vec<BattingStats>, AppError> {
        let query = "SELECT
                            pgb.batter_id AS player_id,
                            pi.first_name AS batter_first_name,
                            pi.last_name AS batter_last_name,
                            SUM(1) AS AB,
                            SUM(CASE WHEN pgb.result = 'Single' THEN 1 ELSE 0 END) AS single,
                            SUM(CASE WHEN pgb.result = 'Double' THEN 1 ELSE 0 END) AS double,
                            SUM(CASE WHEN pgb.result = 'Triple' THEN 1 ELSE 0 END) AS triple,
                            SUM(CASE WHEN pgb.result = 'HomeRun' THEN 1 ELSE 0 END) AS homeRun,
                            COALESCE(ROUND(CAST(SUM(CASE WHEN pgb.result IN ('Single', 'Double', 'Triple', 'HomeRun') THEN 1 ELSE 0 END) AS REAL) / NULLIF(SUM(1), 0), 3), 0.0) AS BA,
                            SUM(c.point) AS rbi
                            FROM player_game_batting pgb
                            LEFT JOIN
                                count c ON pgb.game_id = c.game_id AND pgb.count_seq = c.seq
                            LEFT JOIN
                                player_info pi ON pgb.batter_id = pi.id
                            GROUP BY pgb.batter_id
                            ORDER BY pgb.batter_id";
        self.db_client.query_rows::<BattingStats>(query, params![])
    }

    fn load_pitching_stats(&self) -> Result<Vec<PitchingStats>, AppError> {
        let query = "WITH pitcher_ids AS (
                            SELECT pitcher_id FROM player_game_pitching
                            UNION
                            SELECT pitcher_id FROM player_game_batting
                        ),
                        game_counts AS (
                            SELECT
                                pitcher_id,
                                COUNT(DISTINCT game_id) AS games
                            FROM player_game_pitching
                            GROUP BY pitcher_id
                        ),
                        inning_counts AS (
                            SELECT
                                pgp.pitcher_id,
                                COUNT(DISTINCT CASE
                                    WHEN c.inning_seq IS NOT NULL AND c.inning_tb IS NOT NULL
                                        THEN pgp.game_id || '-' || c.inning_seq || '-' || c.inning_tb
                                    ELSE pgp.game_id || '-' || pgp.count_seq
                                END) AS innings
                            FROM player_game_pitching pgp
                            LEFT JOIN
                                count c ON pgp.game_id = c.game_id AND pgp.count_seq = c.seq
                            GROUP BY pgp.pitcher_id
                        ),
                        result_counts AS (
                            SELECT
                                pitcher_id,
                                SUM(CASE WHEN result = 'Strikeout' THEN 1 ELSE 0 END) AS so,
                                SUM(CASE WHEN result IN ('Walk', 'HitByPitch') THEN 1 ELSE 0 END) AS bb
                            FROM player_game_batting
                            GROUP BY pitcher_id
                        ),
                        decision_counts AS (
                            SELECT
                                pitcher_id,
                                SUM(CASE WHEN decision = 'Win' THEN 1 ELSE 0 END) AS wins,
                                SUM(CASE WHEN decision = 'Loss' THEN 1 ELSE 0 END) AS losses,
                                SUM(CASE WHEN decision = 'Save' THEN 1 ELSE 0 END) AS saves,
                                SUM(CASE WHEN decision = 'Hold' THEN 1 ELSE 0 END) AS holds
                            FROM player_game_pitching_decision
                            GROUP BY pitcher_id
                        )
                        SELECT
                            pitcher_ids.pitcher_id AS player_id,
                            pi.first_name AS pitcher_first_name,
                            pi.last_name AS pitcher_last_name,
                            COALESCE(game_counts.games, 0) AS games,
                            COALESCE(inning_counts.innings, 0) AS innings,
                            COALESCE(decision_counts.wins, 0) AS wins,
                            COALESCE(decision_counts.losses, 0) AS losses,
                            COALESCE(decision_counts.saves, 0) AS saves,
                            COALESCE(decision_counts.holds, 0) AS holds,
                            0 AS era,
                            COALESCE(result_counts.so, 0) AS so,
                            COALESCE(result_counts.bb, 0) AS bb
                        FROM pitcher_ids
                        LEFT JOIN
                            player_info pi ON pitcher_ids.pitcher_id = pi.id
                        LEFT JOIN
                            game_counts ON pitcher_ids.pitcher_id = game_counts.pitcher_id
                        LEFT JOIN
                            inning_counts ON pitcher_ids.pitcher_id = inning_counts.pitcher_id
                        LEFT JOIN
                            result_counts ON pitcher_ids.pitcher_id = result_counts.pitcher_id
                        LEFT JOIN
                            decision_counts ON pitcher_ids.pitcher_id = decision_counts.pitcher_id
                        ORDER BY pitcher_ids.pitcher_id";
        self.db_client.query_rows::<PitchingStats>(query, params![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repositories::db::{DbClient, SqliteManager};
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

            CREATE TABLE player_info (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                team_id INTEGER NOT NULL,
                first_name TEXT NOT NULL,
                last_name TEXT NOT NULL,
                age INTEGER NOT NULL,
                uniform_number INTEGER NOT NULL
            );

            CREATE TABLE player_game_pitching (
                game_id INTEGER,
                count_seq INTEGER,
                pitcher_id INTEGER NOT NULL,
                pitch_type TEXT NOT NULL,
                speed REAL NOT NULL,
                spin_rate REAL NOT NULL,
                spin_angle REAL NOT NULL,
                spin_efficiency REAL NOT NULL,
                release_point_x REAL NOT NULL,
                release_point_y REAL NOT NULL,
                release_point_z REAL NOT NULL,
                flight_time REAL NOT NULL,
                aim_zone TEXT NOT NULL,
                aim_location_x REAL NOT NULL,
                aim_location_y REAL NOT NULL,
                actual_location_x REAL NOT NULL,
                actual_location_y REAL NOT NULL,
                ball_movement_x_m REAL NOT NULL,
                ball_movement_z_m REAL NOT NULL,
                timing_bias_sec REAL NOT NULL,
                spatial_bias_x REAL NOT NULL,
                spatial_bias_y REAL NOT NULL,
                crossfire_multiplier REAL NOT NULL,
                release_x_factor REAL NOT NULL,
                horizontal_offset_m REAL NOT NULL,
                vertical_offset_m REAL NOT NULL,
                timing_offset_sec REAL NOT NULL,
                PRIMARY KEY (game_id, count_seq)
            );

            CREATE TABLE player_game_batting (
                game_id INTEGER,
                count_seq INTEGER,
                pitcher_id INTEGER NOT NULL,
                batter_id INTEGER NOT NULL,
                launch_speed REAL NOT NULL,
                launch_angle REAL NOT NULL,
                polar_distance REAL NOT NULL,
                polar_angle REAL NOT NULL,
                total_time REAL NOT NULL,
                first_bounce_distance REAL,
                first_bounce_angle REAL,
                first_bounce_time REAL,
                fence_impact_distance REAL,
                fence_impact_angle REAL,
                fence_impact_time REAL,
                outbound_result TEXT NOT NULL,
                fielder_position TEXT,
                result TEXT NOT NULL,
                PRIMARY KEY (game_id, count_seq)
            );

            CREATE TABLE player_game_pitching_decision (
                game_id INTEGER NOT NULL,
                pitcher_id INTEGER NOT NULL,
                decision TEXT NOT NULL,
                PRIMARY KEY (game_id, pitcher_id, decision)
            );
            ",
        )
        .unwrap();
        drop(conn);

        let manager = SqliteManager::from_path(path.clone());
        let pool: SqlitePool = Pool::builder(manager).max_size(16).build().unwrap();
        (
            SqlStatRepository {
                db_client: DbClient {
                    db: SqlDb::from_pool(pool),
                },
            },
            path,
        )
    }

    fn conn(repo: &SqlStatRepository) -> deadpool::managed::Object<SqliteManager> {
        repo.db_client.get_conn().unwrap()
    }

    fn seed_team(repo: &SqlStatRepository, id: u16, name: &str) {
        conn(repo)
            .execute(
                "INSERT INTO team (id, league_id, name) VALUES (?1, 1, ?2)",
                params![id, name],
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
        inning_seq: u8,
        inning_tb: &str,
        count_seq: u16,
        point: u8,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO count (
                    game_id, inning_seq, inning_tb, seq, result, point, out
                ) VALUES (?1, ?2, ?3, ?4, 'Out', ?5, 1)",
                params![game_id, inning_seq, inning_tb, count_seq, point],
            )
            .unwrap();
    }

    fn seed_player_info(repo: &SqlStatRepository, id: i64, first_name: &str, last_name: &str) {
        conn(repo)
            .execute(
                "INSERT INTO player_info (
                    id, team_id, first_name, last_name, age, uniform_number
                ) VALUES (?1, 1, ?2, ?3, 25, 11)",
                params![id, first_name, last_name],
            )
            .unwrap();
    }

    fn seed_player_game_pitching(
        repo: &SqlStatRepository,
        game_id: u32,
        count_seq: u16,
        pitcher_id: i64,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO player_game_pitching (
                    game_id, count_seq, pitcher_id, pitch_type, speed, spin_rate, spin_angle,
                    spin_efficiency, release_point_x, release_point_y, release_point_z,
                    flight_time, aim_zone, aim_location_x, aim_location_y, actual_location_x,
                    actual_location_y, ball_movement_x_m, ball_movement_z_m, timing_bias_sec,
                    spatial_bias_x, spatial_bias_y, crossfire_multiplier, release_x_factor,
                    horizontal_offset_m, vertical_offset_m, timing_offset_sec
                ) VALUES (
                    ?1, ?2, ?3, 'FourSeamFastball', 150.0, 2200.0, 0.0,
                    0.9, 0.0, 18.0, 6.0, 0.4, 'Center', 0.0, 0.0, 0.0,
                    0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0,
                    0.0, 0.0, 0.0
                )",
                params![game_id, count_seq, pitcher_id],
            )
            .unwrap();
    }

    fn seed_player_game_batting(
        repo: &SqlStatRepository,
        game_id: u32,
        count_seq: u16,
        pitcher_id: i64,
        batter_id: i64,
        result: &str,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO player_game_batting (
                    game_id, count_seq, pitcher_id, batter_id, launch_speed, launch_angle,
                    polar_distance, polar_angle, total_time, first_bounce_distance,
                    first_bounce_angle, first_bounce_time, fence_impact_distance,
                    fence_impact_angle, fence_impact_time, outbound_result, fielder_position,
                    result
                ) VALUES (
                    ?1, ?2, ?3, ?4, 0.0, 0.0, 0.0, 0.0, 0.0, NULL,
                    NULL, NULL, NULL, NULL, NULL, 'InField', NULL, ?5
                )",
                params![game_id, count_seq, pitcher_id, batter_id, result],
            )
            .unwrap();
    }

    fn seed_player_game_pitching_decision(
        repo: &SqlStatRepository,
        game_id: u32,
        pitcher_id: i64,
        decision: &str,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO player_game_pitching_decision (
                    game_id, pitcher_id, decision
                ) VALUES (?1, ?2, ?3)",
                params![game_id, pitcher_id, decision],
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
    fn load_pitching_stats_returns_games_strikeouts_and_walks() {
        let (repo, path) = setup_repo();
        seed_player_info(&repo, 10, "Shohei", "Ohtani");
        seed_player_info(&repo, 20, "Mike", "Trout");
        seed_count(&repo, 1, 1, "Top", 1, 0);
        seed_count(&repo, 1, 1, "Top", 2, 0);
        seed_count(&repo, 2, 1, "Bottom", 1, 0);
        seed_player_game_pitching(&repo, 1, 1, 10);
        seed_player_game_pitching(&repo, 1, 2, 10);
        seed_player_game_pitching(&repo, 2, 1, 10);
        seed_player_game_batting(&repo, 1, 1, 10, 20, "Strikeout");
        seed_player_game_batting(&repo, 1, 2, 10, 20, "Walk");
        seed_player_game_batting(&repo, 2, 1, 10, 20, "HitByPitch");

        let stats = repo.load_pitching_stats().unwrap();

        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].batter.info.id, 10);
        assert_eq!(stats[0].batter.info.first_name.as_str(), "Shohei");
        assert_eq!(stats[0].batter.info.last_name.as_str(), "Ohtani");
        assert_eq!(stats[0].games, 2);
        assert_eq!(stats[0].innings, 2);
        assert_eq!(stats[0].wins, 0);
        assert_eq!(stats[0].losses, 0);
        assert_eq!(stats[0].saves, 0);
        assert_eq!(stats[0].holds, 0);
        assert_eq!(stats[0].era, 0);
        assert_eq!(stats[0].so, 1);
        assert_eq!(stats[0].bb, 2);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_pitching_stats_counts_pitcher_decisions() {
        let (repo, path) = setup_repo();
        seed_player_info(&repo, 10, "Shohei", "Ohtani");
        seed_player_info(&repo, 20, "Mike", "Trout");
        seed_count(&repo, 1, 1, "Top", 1, 0);
        seed_count(&repo, 2, 1, "Top", 1, 0);
        seed_player_game_pitching(&repo, 1, 1, 10);
        seed_player_game_pitching(&repo, 2, 1, 10);
        seed_player_game_batting(&repo, 1, 1, 10, 20, "Strikeout");
        seed_player_game_batting(&repo, 2, 1, 10, 20, "Out");
        seed_player_game_pitching_decision(&repo, 1, 10, "Win");
        seed_player_game_pitching_decision(&repo, 2, 10, "Save");
        seed_player_game_pitching_decision(&repo, 3, 10, "Hold");
        seed_player_game_pitching_decision(&repo, 4, 10, "Loss");

        let stats = repo.load_pitching_stats().unwrap();

        assert_eq!(stats.len(), 1);
        assert_eq!(stats[0].wins, 1);
        assert_eq!(stats[0].losses, 1);
        assert_eq!(stats[0].saves, 1);
        assert_eq!(stats[0].holds, 1);
        std::fs::remove_file(path).ok();
    }
}
