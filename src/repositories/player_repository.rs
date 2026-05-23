use super::sql_types::SqlPosition;
use crate::domain::error::AppError;
use crate::domain::shared::player::{Player, PlayerAttributeProb};
use crate::domain::shared::team::Team;
use crate::domain::shared::types::Position;
use crate::domain::utils::ItemProb;
use crate::repositories::persistence_config::SqliteManager;
use crate::t;
use anyhow::Result;
use deadpool::managed::Pool;
use rusqlite::{Error, params};

type DbPool = Pool<SqliteManager>;

pub trait PlayerRepository {
    fn save_player(&mut self, team: Team, player: Player) -> Result<()>;
    fn random_name(&self, language: String) -> Result<[String; 2]>;
    fn next_player_dist_team(&self, position: Position) -> Result<Team>;
    fn next_random_team(&self) -> Result<Team>;
    fn position_probs(&self) -> Result<Vec<ItemProb<Position>>>;
    fn player_attribute_probs(&self) -> Result<PlayerAttributeProb>;
}

#[derive(Clone)]
pub struct SqlPlayerRepository {
    pub pool: DbPool,
}

impl PlayerRepository for SqlPlayerRepository {
    fn save_player(&mut self, team: Team, player: Player) -> Result<()> {
        let mut conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));
        let tx = conn.transaction()?;

        if let Err(e) = tx.execute(
            "INSERT INTO player (
                        team_id, first_name, last_name,
                        age, throw, mod_speed, mod_control, bat, mod_ba, mod_slg
                        ) VALUES (
                         ?1, ?2, ?3,
                         ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                team.id,
                player.first_name,
                player.last_name,
                player.age,
                player.throw,
                player.mod_speed,
                player.mod_control,
                player.bat,
                player.mod_ba,
                player.mod_slg
            ],
        ) {
            let error_msg = t!("error", "SQL" => "INSERT INTO player");
            eprintln!("{}: {}", error_msg, e);
            return Err(e.into());
        };

        let generated_id = tx.last_insert_rowid();

        for defensive_skill in player.defensive_skills.iter() {
            if let Err(e) = tx.execute(
                "INSERT INTO defensive_skill (
                        player_id, position, mod_uzr
                        ) VALUES (
                         ?1, ?2, ?3)",
                params![
                    generated_id,
                    defensive_skill.position,
                    defensive_skill.mod_uzr
                ],
            ) {
                let error_msg = t!("error", "SQL" => "INSERT INTO defensive_skill");
                eprintln!("{}: {}", error_msg, e);
                return Err(e.into());
            };
        }

        if let Err(e) = tx.commit() {
            let error_msg = t!("error", "Function" => "commit of save_player");
            eprintln!("{}: {}", error_msg, e);
            return Err(e.into());
        };

        Ok(())
    }

    fn random_name(&self, language: String) -> Result<[String; 2]> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let mut names: [String; 2] = ["".to_string(), "".to_string()];

        // Retrieve First Name
        let res_count_first_names = conn.query_row(
            "SELECT COUNT(*) as count FROM first_names WHERE country = ?1 AND gender = 'M'",
            params![language],
            |row| Ok(row.get::<_, u32>("count")?),
        );

        let count_first_names = res_count_first_names?;
        if count_first_names == 0 {
            return Ok(names);
        }

        let mut random_offset = rand::random_range(0..count_first_names);
        let first_name = conn.query_row(
            "SELECT name FROM first_names WHERE country = ?1 AND gender = 'M' LIMIT 1 OFFSET ?2",
            params![language, random_offset],
            |row| Ok(row.get("name")?),
        );

        if first_name == Err(Error::QueryReturnedNoRows) {
            return Ok(names);
        } else if let Err(e) = first_name {
            let error_msg = t!("error", "SQL" => "SELECT FROM first_names");
            eprintln!("{}: {}", error_msg, e);
            return Err(e.into());
        }
        names[0] = first_name?;

        // Retrieve Last Name
        let res_count_last_names = conn.query_row(
            "SELECT COUNT(*) as count FROM last_names WHERE country = ?1",
            params![language],
            |row| Ok(row.get::<_, u32>("count")?),
        );

        let count_last_names = res_count_last_names?;
        if count_last_names == 0 {
            return Ok(names);
        }

        random_offset = rand::random_range(0..count_last_names);
        let last_name = conn.query_row(
            "SELECT name FROM last_names WHERE country = ?1 LIMIT 1 OFFSET ?2",
            params![language, random_offset],
            |row| Ok(row.get("name")?),
        );

        if last_name == Err(Error::QueryReturnedNoRows) {
            return Ok(names);
        } else if let Err(e) = last_name {
            let error_msg = t!("error", "SQL" => "SELECT FROM last_names");
            eprintln!("{}: {}", error_msg, e);
            return Err(e.into());
        }
        names[1] = last_name?;

        Ok(names)
    }

    fn next_player_dist_team(&self, position: Position) -> Result<Team> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let team_result = conn.query_row(
            "SELECT
                        t.id AS team_id,
                        t.name AS team_name,
                        COUNT(p.id) AS player_count
                    FROM team t
                    LEFT JOIN player p ON t.id = p.team_id
                    LEFT JOIN defensive_skill ds ON ds.player_id = p.id
        			WHERE ds.position = ?1
                    GROUP BY t.id
                    ORDER BY player_count, t.id
                    LIMIT 1;",
            params![position],
            |row| {
                Ok(Team {
                    id: row.get("team_id")?,
                    name: row.get("team_name")?,
                    players: Vec::new(),
                })
            },
        );

        match team_result {
            Ok(team) => Ok(team),
            Err(rusqlite::Error::QueryReturnedNoRows) => {
                let error_msg = t!("not_found", "property" => "Position");
                return Err(AppError::NotFound(format!("{} {:?}", error_msg, position)).into());
            }
            Err(e) => {
                let error_msg = t!("error", "SQL" => "SELECT FROM team");
                eprintln!("{}: {}", error_msg, e);
                return Err(e.into());
            }
        }
    }

    fn next_random_team(&self) -> Result<Team> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let team = conn.query_row(
            "SELECT id, name
                    FROM team
                    ORDER BY RANDOM() 
                    LIMIT 1;",
            params![],
            |row| {
                Ok(Team {
                    id: row.get("id")?,
                    name: row.get("name")?,
                    players: Vec::new(),
                })
            },
        );
        if let Err(e) = &team {
            let error_msg = t!("error", "SQL" => "SELECT FROM team");
            eprintln!("{}: {}", error_msg, e);
        }
        Ok(team?)
    }

    fn position_probs(&self) -> Result<Vec<ItemProb<Position>>> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let mut position_probs = Vec::new();

        let mut stmt =
            conn.prepare("SELECT name, prob FROM item_prob WHERE category = 'position'")?;
        let position_prob_iter = stmt
            .query_map([], |row| {
                Ok(ItemProb {
                    name: row.get::<_, SqlPosition>("name")?.0,
                    prob: row.get("prob")?,
                })
            })
            .map_err(|err| {
                let error_msg = t!("error", "SQL" => "SELECT FROM item_prob");
                eprintln!("{}:{}", error_msg, err);
                err
            })?;

        for position_prob_result in position_prob_iter {
            let position_prob = position_prob_result?;
            position_probs.push(position_prob);
        }

        Ok(position_probs)
    }

    fn player_attribute_probs(&self) -> Result<PlayerAttributeProb> {
        let conn = futures::executor::block_on(self.pool.get()).expect(&t!("dbpool_failed"));

        let player_attribute_prob = conn.query_row(
            "SELECT 
                    MAX(CASE WHEN name = 'age_shape' THEN prob END) AS age_shape,
                    MAX(CASE WHEN name = 'age_scale' THEN prob END) AS age_scale,
                    MAX(CASE WHEN name = 'age_offset' THEN prob END) AS age_offset,
                    MAX(CASE WHEN name = 'throw_lefty' THEN prob END) AS throw_lefty,
                    MAX(CASE WHEN name = 'bat_lefty' THEN prob END) AS bat_lefty
                    FROM item_prob
                    WHERE category = 'player_attribute';",
            params![],
            |row| {
                Ok(PlayerAttributeProb {
                    age_shape: row.get("age_shape")?,
                    age_scale: row.get("age_scale")?,
                    age_offset: row.get("age_offset")?,
                    throw_lefty: row.get("throw_lefty")?,
                    bat_lefty: row.get("bat_lefty")?,
                })
            },
        );
        if let Err(e) = &player_attribute_prob {
            let error_msg = t!("error", "SQL" => "SELECT FROM item_prob");
            eprintln!("{}: {}", error_msg, e);
        }
        Ok(player_attribute_prob?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::{DefensiveSkill, RL};
    use rusqlite::{Connection, params};
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
            "saberbb-player-repository-{}-{nanos}-{seq}.db",
            std::process::id()
        ))
    }

    fn setup_repo() -> (SqlPlayerRepository, PathBuf) {
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

            CREATE TABLE defensive_skill (
                player_id INTEGER,
                position TEXT,
                mod_uzr REAL NOT NULL,
                PRIMARY KEY (player_id, position)
            );

            CREATE TABLE first_names (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT,
                reading TEXT,
                gender TEXT,
                country TEXT
            );

            CREATE TABLE last_names (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT,
                reading TEXT,
                country TEXT
            );

            CREATE TABLE item_prob (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                category TEXT NOT NULL,
                name TEXT NOT NULL,
                prob REAL NOT NULL
            );
            ",
        )
        .unwrap();
        drop(conn);

        let manager = SqliteManager { path: path.clone() };
        let pool: DbPool = Pool::builder(manager).max_size(16).build().unwrap();
        (SqlPlayerRepository { pool }, path)
    }

    fn setup_repo_without_player_table() -> (SqlPlayerRepository, PathBuf) {
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
            ",
        )
        .unwrap();
        drop(conn);

        let manager = SqliteManager { path: path.clone() };
        let pool: DbPool = Pool::builder(manager).max_size(16).build().unwrap();
        (SqlPlayerRepository { pool }, path)
    }

    fn conn(repo: &SqlPlayerRepository) -> deadpool::managed::Object<SqliteManager> {
        futures::executor::block_on(repo.pool.get()).unwrap()
    }

    fn seed_team(repo: &SqlPlayerRepository, id: u16, name: &str) {
        conn(repo)
            .execute(
                "INSERT INTO team (id, league_id, name) VALUES (?1, 1, ?2)",
                params![id, name],
            )
            .unwrap();
    }

    fn seed_player_row(repo: &SqlPlayerRepository, id: u32, team_id: u16) {
        conn(repo)
            .execute(
                "INSERT INTO player (
                    id, team_id, first_name, last_name, age, throw,
                    mod_speed, mod_control, bat, mod_ba, mod_slg
                ) VALUES (?1, ?2, ?3, ?4, 25, 'Right', 0.0, 0.0, 'Right', 0.0, 0.0)",
                params![id, team_id, format!("First{id}"), format!("Last{id}")],
            )
            .unwrap();
    }

    fn seed_defensive_skill(repo: &SqlPlayerRepository, player_id: u32, position: Position) {
        conn(repo)
            .execute(
                "INSERT INTO defensive_skill (player_id, position, mod_uzr)
                 VALUES (?1, ?2, 0.0)",
                params![player_id, position],
            )
            .unwrap();
    }

    fn seed_first_name(repo: &SqlPlayerRepository, name: &str, gender: &str, country: &str) {
        conn(repo)
            .execute(
                "INSERT INTO first_names (name, reading, gender, country) VALUES (?1, '', ?2, ?3)",
                params![name, gender, country],
            )
            .unwrap();
    }

    fn seed_last_name(repo: &SqlPlayerRepository, name: &str, country: &str) {
        conn(repo)
            .execute(
                "INSERT INTO last_names (name, reading, country) VALUES (?1, '', ?2)",
                params![name, country],
            )
            .unwrap();
    }

    fn seed_position_prob(repo: &SqlPlayerRepository, position: Position, prob: f64) {
        conn(repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob) VALUES ('position', ?1, ?2)",
                params![position, prob],
            )
            .unwrap();
    }

    fn seed_player_attribute_prob(repo: &SqlPlayerRepository, name: &str, prob: f64) {
        conn(repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('player_attribute', ?1, ?2)",
                params![name, prob],
            )
            .unwrap();
    }

    fn player() -> Player {
        Player {
            id: 0,
            first_name: "翔平".into(),
            last_name: "大谷".into(),
            age: 29,
            throw: RL::Left,
            mod_speed: 1.1,
            mod_control: 1.2,
            defensive_skills: Vec::new(),
            bat: RL::Right,
            mod_ba: 1.3,
            mod_slg: 1.4,
        }
    }

    #[test]
    fn save_player_inserts_all_player_fields() {
        let (mut repo, path) = setup_repo();
        seed_team(&repo, 1, "ライオンズ");

        repo.save_player(Team::min(1, "ライオンズ"), player())
            .unwrap();

        let conn = conn(&repo);
        let row: (u16, String, String, u8, String, f64, f64, String, f64, f64) = conn
            .query_row(
                "SELECT team_id, first_name, last_name, age, throw,
                    mod_speed, mod_control, bat, mod_ba, mod_slg
                 FROM player",
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
                        row.get(7)?,
                        row.get(8)?,
                        row.get(9)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, 1);
        assert_eq!(row.1, "翔平");
        assert_eq!(row.2, "大谷");
        assert_eq!(row.3, 29);
        assert_eq!(row.4, "Left");
        assert_eq!(row.5, 1.1);
        assert_eq!(row.6, 1.2);
        assert_eq!(row.7, "Right");
        assert_eq!(row.8, 1.3);
        assert_eq!(row.9, 1.4);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_player_inserts_defensive_skills() {
        let (mut repo, path) = setup_repo();
        seed_team(&repo, 1, "ライオンズ");
        let mut player = player();
        player.defensive_skills = vec![
            DefensiveSkill {
                position: Position::P,
                mod_uzr: 1.5,
            },
            DefensiveSkill {
                position: Position::CF,
                mod_uzr: 2.5,
            },
        ];

        repo.save_player(Team::min(1, "ライオンズ"), player)
            .unwrap();

        let conn = conn(&repo);
        let skills: Vec<(String, f64)> = {
            let mut stmt = conn
                .prepare("SELECT position, mod_uzr FROM defensive_skill ORDER BY position")
                .unwrap();
            stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap()
        };

        assert_eq!(
            skills,
            vec![("CF".to_string(), 2.5), ("P".to_string(), 1.5)]
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_player_returns_error_when_player_table_is_missing() {
        let (mut repo, path) = setup_repo_without_player_table();
        seed_team(&repo, 1, "ライオンズ");

        let result = repo.save_player(Team::min(1, "ライオンズ"), player());

        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_returns_matching_first_and_last_name_for_language() {
        let (repo, path) = setup_repo();
        seed_first_name(&repo, "翔平", "M", "JP");
        seed_last_name(&repo, "大谷", "JP");

        let names = repo.random_name("JP".to_string()).unwrap();

        assert_eq!(names, ["翔平".to_string(), "大谷".to_string()]);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_filters_out_non_male_first_names() {
        let (repo, path) = setup_repo();
        seed_first_name(&repo, "FemaleOnly", "F", "JP");
        seed_first_name(&repo, "MaleOnly", "M", "JP");
        seed_last_name(&repo, "大谷", "JP");

        let names = repo.random_name("JP".to_string()).unwrap();

        assert_eq!(names[0], "MaleOnly");
        assert_eq!(names[1], "大谷");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_filters_by_country() {
        let (repo, path) = setup_repo();
        seed_first_name(&repo, "翔平", "M", "JP");
        seed_first_name(&repo, "Mike", "M", "US");
        seed_last_name(&repo, "大谷", "JP");
        seed_last_name(&repo, "Trout", "US");

        let names = repo.random_name("JP".to_string()).unwrap();

        assert_eq!(names[0], "翔平");
        assert_eq!(names[1], "大谷");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_returns_empty_pair_when_no_first_names_for_language() {
        let (repo, path) = setup_repo();
        seed_last_name(&repo, "大谷", "JP");

        let names = repo.random_name("JP".to_string()).unwrap();

        assert_eq!(names, ["".to_string(), "".to_string()]);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_returns_first_name_and_empty_last_name_when_no_last_names() {
        let (repo, path) = setup_repo();
        seed_first_name(&repo, "翔平", "M", "JP");

        let names = repo.random_name("JP".to_string()).unwrap();

        assert_eq!(names, ["翔平".to_string(), "".to_string()]);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn next_player_dist_team_returns_team_with_fewest_players() {
        let (repo, path) = setup_repo();
        seed_team(&repo, 1, "Full");
        seed_team(&repo, 2, "Fewest Pitchers");
        seed_team(&repo, 3, "Half");
        seed_player_row(&repo, 1, 1);
        seed_player_row(&repo, 2, 1);
        seed_player_row(&repo, 3, 3);
        seed_player_row(&repo, 4, 2);
        seed_defensive_skill(&repo, 1, Position::P);
        seed_defensive_skill(&repo, 2, Position::P);
        seed_defensive_skill(&repo, 3, Position::C);
        seed_defensive_skill(&repo, 4, Position::P);

        let team = repo.next_player_dist_team(Position::P).unwrap();

        assert_eq!(team.id, 2);
        assert_eq!(team.name.as_ref(), "Fewest Pitchers");
        assert!(team.players.is_empty());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn next_player_dist_team_breaks_ties_by_team_id() {
        let (repo, path) = setup_repo();
        seed_team(&repo, 3, "Third");
        seed_team(&repo, 1, "First");
        seed_team(&repo, 2, "Second");
        seed_player_row(&repo, 1, 3);
        seed_player_row(&repo, 2, 1);
        seed_player_row(&repo, 3, 2);
        seed_defensive_skill(&repo, 1, Position::P);
        seed_defensive_skill(&repo, 2, Position::P);
        seed_defensive_skill(&repo, 3, Position::C);

        let team = repo.next_player_dist_team(Position::P).unwrap();

        assert_eq!(team.id, 1);
        assert_eq!(team.name.as_ref(), "First");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn next_player_dist_team_returns_error_when_no_teams_exist() {
        let (repo, path) = setup_repo();

        let result = repo.next_player_dist_team(Position::P);

        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn next_random_team_returns_seeded_team_without_players() {
        let (repo, path) = setup_repo();
        seed_team(&repo, 7, "Randoms");
        seed_player_row(&repo, 1, 7);

        let team = repo.next_random_team().unwrap();

        assert_eq!(team.id, 7);
        assert_eq!(team.name.as_ref(), "Randoms");
        assert!(team.players.is_empty());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn next_random_team_returns_error_when_no_teams_exist() {
        let (repo, path) = setup_repo();

        let result = repo.next_random_team();

        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn position_probs_returns_seeded_position_probabilities() {
        let (repo, path) = setup_repo();
        seed_position_prob(&repo, Position::P, 0.42);
        seed_position_prob(&repo, Position::CF, 0.07);

        let position_probs = repo.position_probs().unwrap();

        assert_eq!(position_probs.len(), 2);
        assert!(position_probs.iter().any(|position_prob| {
            position_prob.name == Position::P && position_prob.prob == 0.42
        }));
        assert!(position_probs.iter().any(|position_prob| {
            position_prob.name == Position::CF && position_prob.prob == 0.07
        }));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn position_probs_ignores_non_position_categories() {
        let (repo, path) = setup_repo();
        seed_position_prob(&repo, Position::P, 0.42);
        conn(&repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob) VALUES ('bat', 'Right', 0.6)",
                [],
            )
            .unwrap();

        let position_probs = repo.position_probs().unwrap();

        assert_eq!(position_probs.len(), 1);
        assert_eq!(position_probs[0].name, Position::P);
        assert_eq!(position_probs[0].prob, 0.42);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn player_attribute_probs_returns_seeded_probabilities() {
        let (repo, path) = setup_repo();
        seed_player_attribute_prob(&repo, "age_shape", 2.5);
        seed_player_attribute_prob(&repo, "age_scale", 2.6);
        seed_player_attribute_prob(&repo, "age_offset", 18.0);
        seed_player_attribute_prob(&repo, "throw_lefty", 0.2);
        seed_player_attribute_prob(&repo, "bat_lefty", 0.4);

        let player_attribute_probs = repo.player_attribute_probs().unwrap();

        assert_eq!(player_attribute_probs.age_shape, 2.5);
        assert_eq!(player_attribute_probs.age_scale, 2.6);
        assert_eq!(player_attribute_probs.age_offset, 18.0);
        assert_eq!(player_attribute_probs.throw_lefty, 0.2);
        assert_eq!(player_attribute_probs.bat_lefty, 0.4);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn player_attribute_probs_ignores_non_player_attribute_categories() {
        let (repo, path) = setup_repo();
        seed_player_attribute_prob(&repo, "age_shape", 2.5);
        seed_player_attribute_prob(&repo, "age_scale", 2.6);
        seed_player_attribute_prob(&repo, "age_offset", 18.0);
        seed_player_attribute_prob(&repo, "throw_lefty", 0.2);
        seed_player_attribute_prob(&repo, "bat_lefty", 0.4);
        conn(&repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('position', 'age_shape', 99.0)",
                [],
            )
            .unwrap();

        let player_attribute_probs = repo.player_attribute_probs().unwrap();

        assert_eq!(player_attribute_probs.age_shape, 2.5);
        std::fs::remove_file(path).ok();
    }
}
