use crate::domain::shared::player::{FullName, PitchType, PitcherStyle, Player, Position};
use crate::domain::shared::prob::ItemProb;
use crate::domain::shared::prob::{
    BatterSkillProb, DefensiveSkillProb, PitchSkillProb, PitcherAttributeProb, PlayerAttributeProb,
};
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::{DbClient, SqlDb};
use anyhow::Result;
use rusqlite::params;

pub trait PlayerRepository {
    fn save_player(&mut self, team: Team, player: Player) -> Result<(), AppError>;
    fn random_name(&self, language: String) -> Result<FullName, AppError>;
    fn next_player_dist_team(&self, position: Position) -> Result<Team, AppError>;
    fn next_random_team(&self) -> Result<Team, AppError>;
    fn position_probs(&self) -> Result<Vec<ItemProb<Position>>, AppError>;
    fn pitcher_style_probs(&self) -> Result<Vec<ItemProb<PitcherStyle>>, AppError>;
    fn pitch_skill_prob(&self, pitch_type: &PitchType) -> Result<PitchSkillProb, AppError>;
    fn pitch_type_probs(
        &self,
        pitch_style: &PitcherStyle,
    ) -> Result<Vec<ItemProb<PitchType>>, AppError>;
    fn player_attribute_prob(&self) -> Result<PlayerAttributeProb, AppError>;
    fn batter_skill_prob(&self) -> Result<BatterSkillProb, AppError>;
    fn defensive_skill_prob(&self) -> Result<DefensiveSkillProb, AppError>;
    fn pitcher_attribute_prob(&self) -> Result<PitcherAttributeProb, AppError>;
}

#[derive(Clone)]
pub struct SqlPlayerRepository {
    db_client: DbClient,
}
impl SqlPlayerRepository {
    pub fn new() -> Result<Self> {
        let db_client = DbClient { db: SqlDb::new()? };
        Ok(Self { db_client })
    }
}

impl PlayerRepository for SqlPlayerRepository {
    fn save_player(&mut self, team: Team, mut player: Player) -> Result<(), AppError> {
        self.db_client.transaction(|tx| {
            let insert_player_sql = "INSERT INTO player (
                                        team_id, first_name, last_name,
                                        age, throw, bat, mod_ba, mod_slg
                                        ) VALUES (
                                        ?1, ?2, ?3,
                                        ?4, ?5, ?6, ?7, ?8)";
            let generated_id = self.db_client.execute_insert_tx(
                tx,
                insert_player_sql,
                params![
                    team.id,
                    player.first_name,
                    player.last_name,
                    player.age,
                    player.throw,
                    player.bat,
                    player.mod_ba,
                    player.mod_slg
                ],
            )?;

            for defensive_skill in player.defensive_skills.iter() {
                let insert_defensive_skill_sql = "INSERT INTO defensive_skill (
                                                player_id, position, mod_uzr
                                                ) VALUES (
                                                ?1, ?2, ?3)";
                self.db_client.execute_tx(
                    tx,
                    insert_defensive_skill_sql,
                    params![
                        generated_id,
                        defensive_skill.position,
                        defensive_skill.mod_uzr
                    ],
                )?;
            }

            if let Some(pitcher_attribute) = player.pitcher_attribute.take() {
                let insert_pitcher_attribute_sql = "INSERT INTO pitcher_attribute (
                                                            player_id,
                                                            pitcher_style,
                                                            mod_velocity,
                                                            mod_control,
                                                            mod_stamina,
                                                            mod_injury_proneness,
                                                            mod_clutch,
                                                            mod_hpp,
                                                            mod_platoon_splitting
                                                            ) VALUES (
                                                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)";
                self.db_client.execute_tx(
                    tx,
                    insert_pitcher_attribute_sql,
                    params![
                        generated_id,
                        pitcher_attribute.pitcher_style,
                        pitcher_attribute.mod_velocity,
                        pitcher_attribute.mod_control,
                        pitcher_attribute.mod_stamina,
                        pitcher_attribute.mod_injury_proneness,
                        pitcher_attribute.mod_clutch,
                        pitcher_attribute.mod_hpp,
                        pitcher_attribute.mod_platoon_splitting,
                    ],
                )?;

                for pitch_skill in pitcher_attribute.pitch_skills {
                    let insert_pitch_skill_sql = "INSERT INTO pitch_skill (
                                                            player_id,
                                                            pitch_type,
                                                            mod_velocity,
                                                            mod_control,
                                                            mod_stamina,
                                                            mod_injury_proneness,
                                                            mod_stuff,
                                                            mod_fb,
                                                            mod_gp,
                                                            mod_horizontal_movement,
                                                            mod_vertical_movement,
                                                            mod_spin_rate,
                                                            mod_usage
                                                        ) VALUES (
                                                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)";
                    self.db_client.execute_tx(
                        tx,
                        insert_pitch_skill_sql,
                        params![
                            generated_id,
                            pitch_skill.pitch_type,
                            pitch_skill.mod_velocity,
                            pitch_skill.mod_control,
                            pitch_skill.mod_stamina,
                            pitch_skill.mod_injury_proneness,
                            pitch_skill.mod_stuff,
                            pitch_skill.mod_fb,
                            pitch_skill.mod_gp,
                            pitch_skill.mod_horizontal_movement,
                            pitch_skill.mod_vertical_movement,
                            pitch_skill.mod_spin_rate,
                            pitch_skill.mod_usage,
                        ],
                    )?;
                }
            }
            Ok(())
        })
    }

    fn random_name(&self, language: String) -> Result<FullName, AppError> {
        let query = "SELECT 
	                        fn.name AS first_name,
	                        ln.name AS last_name
                            FROM 
                            (
                                SELECT name 
                                FROM last_names 
                                WHERE country = ?1
                                LIMIT 1 
                                OFFSET ABS(RANDOM()) % (SELECT COUNT(*) FROM last_names WHERE country = ?1)
                            ) AS ln
                            CROSS JOIN 
                            (
                                SELECT name 
                                FROM first_names 
                                WHERE country = ?1 AND gender = 'M'
                                LIMIT 1 
                                OFFSET ABS(RANDOM()) % (SELECT COUNT(*) FROM first_names WHERE country = ?1 AND gender = 'M')
                            ) AS fn";

        self.db_client
            .query_row::<FullName>(query, params![language])
    }

    fn next_player_dist_team(&self, position: Position) -> Result<Team, AppError> {
        let query = "SELECT
                            t.id AS id,
                            t.name AS name,
                            COUNT(ds.player_id) AS player_count
                            FROM team t
                            LEFT JOIN player p ON t.id = p.team_id
                            LEFT JOIN defensive_skill ds ON ds.player_id = p.id
                                AND ds.position = ?1
                            GROUP BY t.id, t.name
                            ORDER BY player_count, t.id
                            LIMIT 1";
        self.db_client.query_row::<Team>(query, params![position])
    }

    fn next_random_team(&self) -> Result<Team, AppError> {
        let query = "SELECT id, name
                    FROM team
                    ORDER BY RANDOM() 
                    LIMIT 1";
        self.db_client.query_row::<Team>(query, params![])
    }

    fn position_probs(&self) -> Result<Vec<ItemProb<Position>>, AppError> {
        let query = "SELECT name, prob FROM item_prob WHERE category = ?1";
        self.db_client
            .query_rows::<ItemProb<Position>>(query, params!["position"])
    }

    fn pitcher_style_probs(&self) -> Result<Vec<ItemProb<PitcherStyle>>, AppError> {
        let query = "SELECT name, prob FROM item_prob WHERE category = ?1";
        self.db_client
            .query_rows::<ItemProb<PitcherStyle>>(query, params!["pitcher_style"])
    }

    fn pitch_type_probs(
        &self,
        pitch_style: &PitcherStyle,
    ) -> Result<Vec<ItemProb<PitchType>>, AppError> {
        let query = "SELECT name, prob FROM item_prob WHERE category = ?1";
        self.db_client
            .query_rows::<ItemProb<PitchType>>(query, params![pitch_style.as_ref()])
    }

    fn player_attribute_prob(&self) -> Result<PlayerAttributeProb, AppError> {
        let query = "SELECT 
                        MAX(CASE WHEN name = 'age_shape' THEN prob END) AS age_shape,
                        MAX(CASE WHEN name = 'age_scale' THEN prob END) AS age_scale,
                        MAX(CASE WHEN name = 'age_offset' THEN prob END) AS age_offset,
                        MAX(CASE WHEN name = 'throw_lefty' THEN prob END) AS throw_lefty,
                        MAX(CASE WHEN name = 'bat_lefty' THEN prob END) AS bat_lefty
                        FROM item_prob
                        WHERE category = 'player_attribute'";
        self.db_client
            .query_row::<PlayerAttributeProb>(query, params![])
    }

    fn batter_skill_prob(&self) -> Result<BatterSkillProb, AppError> {
        let query = "SELECT 
                        MAX(CASE WHEN name = 'ba_skew' THEN prob END) AS ba_skew,
                        MAX(CASE WHEN name = 'slg_skew' THEN prob END) AS slg_skew
                        FROM item_prob
                        WHERE category = 'batter_skill'";
        self.db_client
            .query_row::<BatterSkillProb>(query, params![])
    }

    fn defensive_skill_prob(&self) -> Result<DefensiveSkillProb, AppError> {
        let query = "SELECT 
                     MAX(CASE WHEN name = 'uzr_skew' THEN prob END) AS uzr_skew
                     FROM item_prob
                     WHERE category = 'defensive_skill'";
        self.db_client
            .query_row::<DefensiveSkillProb>(query, params![])
    }

    fn pitcher_attribute_prob(&self) -> Result<PitcherAttributeProb, AppError> {
        let query = "SELECT 
                        MAX(CASE WHEN name = 'velocity_skew' THEN prob END) AS velocity_skew,
    				    MAX(CASE WHEN name = 'control_skew' THEN prob END) AS control_skew,
    				    MAX(CASE WHEN name = 'stamina_skew' THEN prob END) AS stamina_skew,
    				    MAX(CASE WHEN name = 'injury_proneness_skew' THEN prob END) AS injury_proneness_skew,
    				    MAX(CASE WHEN name = 'clutch_skew' THEN prob END) AS clutch_skew,
    				    MAX(CASE WHEN name = 'hpp_skew' THEN prob END) AS hpp_skew,
    				    MAX(CASE WHEN name = 'platoon_splitting_skew' THEN prob END) AS platoon_splitting_skew
                        FROM item_prob
                        WHERE category = 'pitcher_attribute'";
        self.db_client
            .query_row::<PitcherAttributeProb>(query, params![])
    }

    fn pitch_skill_prob(&self, pitch_type: &PitchType) -> Result<PitchSkillProb, AppError> {
        let query = "SELECT 
                            MAX(CASE WHEN name = 'velocity_skew' THEN prob END) AS velocity_skew,
    				        MAX(CASE WHEN name = 'control_skew' THEN prob END) AS control_skew,
    				        MAX(CASE WHEN name = 'stamina_skew' THEN prob END) AS stamina_skew,
    				        MAX(CASE WHEN name = 'injury_proneness_skew' THEN prob END) AS injury_proneness_skew,
    				        MAX(CASE WHEN name = 'stuff_skew' THEN prob END) AS stuff_skew,
    				        MAX(CASE WHEN name = 'fb_skew' THEN prob END) AS fb_skew,
    				        MAX(CASE WHEN name = 'gp_skew' THEN prob END) AS gp_skew,
    				        MAX(CASE WHEN name = 'horizontal_movement_skew' THEN prob END) AS horizontal_movement_skew,
    				        MAX(CASE WHEN name = 'vertical_movement_skew' THEN prob END) AS vertical_movement_skew,
    				        MAX(CASE WHEN name = 'spin_rate_skew' THEN prob END) AS spin_rate_skew,
    				        MAX(CASE WHEN name = 'usage_skew' THEN prob END) AS usage_skew
                            FROM item_prob
                            WHERE category = ?1";
        self.db_client
            .query_row_with_ctx::<PitchSkillProb, PitchType>(query, params![pitch_type], pitch_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::{
        DefensiveSkill, PitchSkill, PitcherAttribute, PitcherStyle, Position, RL,
    };
    use crate::repositories::db::SqliteManager;
    use deadpool::managed::Pool;
    use rusqlite::{Connection, params};
    use std::path::PathBuf;
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
                bat TEXT NOT NULL,
                mod_ba REAL NOT NULL,
                mod_slg REAL NOT NULL
            );

            CREATE TABLE pitcher_attribute (
                player_id INTEGER PRIMARY KEY,
                pitcher_style TEXT NOT NULL,
                mod_velocity REAL NOT NULL,
                mod_control REAL NOT NULL,
                mod_stamina REAL NOT NULL,
                mod_injury_proneness REAL NOT NULL,
                mod_clutch REAL NOT NULL,
                mod_hpp REAL NOT NULL,
                mod_platoon_splitting REAL NOT NULL
            );

            CREATE TABLE pitch_skill (
                player_id INTEGER,
                pitch_type TEXT,
                mod_velocity REAL NOT NULL,
                mod_control REAL NOT NULL,
                mod_stamina REAL NOT NULL,
                mod_injury_proneness REAL NOT NULL,
                mod_stuff REAL NOT NULL,
                mod_fb REAL NOT NULL,
                mod_gp REAL NOT NULL,
                mod_horizontal_movement REAL NOT NULL,
                mod_vertical_movement REAL NOT NULL,
                mod_spin_rate REAL NOT NULL,
                mod_usage REAL NOT NULL,
                PRIMARY KEY (player_id, pitch_type)
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

        let manager = SqliteManager::from_path(path.clone());
        let pool: SqlitePool = Pool::builder(manager).max_size(16).build().unwrap();
        let db = SqlDb::from_pool(pool);
        let db_client = DbClient { db };
        (SqlPlayerRepository { db_client }, path)
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

        let manager = SqliteManager::from_path(path.clone());
        let pool: SqlitePool = Pool::builder(manager).max_size(16).build().unwrap();
        let db = SqlDb::from_pool(pool);
        let db_client = DbClient { db };
        (SqlPlayerRepository { db_client }, path)
    }

    fn conn(repo: &SqlPlayerRepository) -> deadpool::managed::Object<SqliteManager> {
        repo.db_client.get_conn().unwrap()
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
                    bat, mod_ba, mod_slg
                ) VALUES (?1, ?2, ?3, ?4, 25, 'Right', 'Right', 0.0, 0.0)",
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

    fn seed_pitcher_style_prob(repo: &SqlPlayerRepository, pitcher_style: PitcherStyle, prob: f64) {
        conn(repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('pitcher_style', ?1, ?2)",
                params![pitcher_style, prob],
            )
            .unwrap();
    }

    fn seed_pitch_type_prob(
        repo: &SqlPlayerRepository,
        pitcher_style: PitcherStyle,
        pitch_type: PitchType,
        prob: f64,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES (?1, ?2, ?3)",
                params![pitcher_style.as_ref(), pitch_type, prob],
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

    fn seed_batter_skill_prob(repo: &SqlPlayerRepository, name: &str, prob: f64) {
        conn(repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('batter_skill', ?1, ?2)",
                params![name, prob],
            )
            .unwrap();
    }

    fn seed_defensive_skill_prob(repo: &SqlPlayerRepository, name: &str, prob: f64) {
        conn(repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('defensive_skill', ?1, ?2)",
                params![name, prob],
            )
            .unwrap();
    }

    fn seed_pitcher_attribute_prob(repo: &SqlPlayerRepository, name: &str, prob: f64) {
        conn(repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('pitcher_attribute', ?1, ?2)",
                params![name, prob],
            )
            .unwrap();
    }

    fn seed_pitch_skill_prob(
        repo: &SqlPlayerRepository,
        pitch_type: PitchType,
        name: &str,
        prob: f64,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES (?1, ?2, ?3)",
                params![pitch_type, name, prob],
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
            defensive_skills: Vec::new(),
            pitcher_attribute: None,
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
        let row: (u16, String, String, u8, String, String, f64, f64) = conn
            .query_row(
                "SELECT team_id, first_name, last_name, age, throw,
                    bat, mod_ba, mod_slg
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
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, 1);
        assert_eq!(row.1, "翔平");
        assert_eq!(row.2, "大谷");
        assert_eq!(row.3, 29);
        assert_eq!(row.4, "Left");
        assert_eq!(row.5, "Right");
        assert_eq!(row.6, 1.3);
        assert_eq!(row.7, 1.4);
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
    fn save_player_inserts_pitcher_attribute_when_present() {
        let (mut repo, path) = setup_repo();
        seed_team(&repo, 1, "ライオンズ");
        let mut player = player();
        player.pitcher_attribute = Some(PitcherAttribute {
            pitcher_style: PitcherStyle::BalancedPitcher,
            mod_velocity: 1.1,
            mod_control: 1.2,
            mod_stamina: 1.3,
            mod_injury_proneness: 1.4,
            mod_clutch: 1.5,
            mod_hpp: 1.6,
            mod_platoon_splitting: 1.7,
            pitch_skills: Vec::new(),
        });

        repo.save_player(Team::min(1, "ライオンズ"), player)
            .unwrap();

        let conn = conn(&repo);
        let row: (u32, String, f64, f64, f64, f64, f64, f64, f64) = conn
            .query_row(
                "SELECT player_id, pitcher_style, mod_velocity, mod_control, mod_stamina,
                    mod_injury_proneness, mod_clutch, mod_hpp, mod_platoon_splitting
                 FROM pitcher_attribute",
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
                    ))
                },
            )
            .unwrap();

        assert_eq!(
            row,
            (
                1,
                "BalancedPitcher".to_string(),
                1.1,
                1.2,
                1.3,
                1.4,
                1.5,
                1.6,
                1.7
            )
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_player_inserts_pitch_skills_when_pitcher_attribute_is_present() {
        let (mut repo, path) = setup_repo();
        seed_team(&repo, 1, "ライオンズ");
        let mut player = player();
        player.pitcher_attribute = Some(PitcherAttribute {
            pitcher_style: PitcherStyle::PowerPitcher,
            mod_velocity: 1.1,
            mod_control: 1.2,
            mod_stamina: 1.3,
            mod_injury_proneness: 1.4,
            mod_clutch: 1.5,
            mod_hpp: 1.6,
            mod_platoon_splitting: 1.7,
            pitch_skills: vec![PitchSkill {
                pitch_type: PitchType::FourSeamFastball,
                mod_velocity: 2.1,
                mod_control: 2.2,
                mod_stamina: 2.3,
                mod_injury_proneness: 2.4,
                mod_stuff: 2.5,
                mod_fb: 2.6,
                mod_gp: 2.7,
                mod_horizontal_movement: 2.8,
                mod_vertical_movement: 2.9,
                mod_spin_rate: 3.0,
                mod_usage: 3.1,
            }],
        });

        repo.save_player(Team::min(1, "ライオンズ"), player)
            .unwrap();

        let conn = conn(&repo);
        let row: (
            u32,
            String,
            f64,
            f64,
            f64,
            f64,
            f64,
            f64,
            f64,
            f64,
            f64,
            f64,
            f64,
        ) = conn
            .query_row(
                "SELECT player_id, pitch_type, mod_velocity, mod_control, mod_stamina,
                    mod_injury_proneness, mod_stuff, mod_fb, mod_gp,
                    mod_horizontal_movement, mod_vertical_movement, mod_spin_rate, mod_usage
                 FROM pitch_skill",
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
                        row.get(10)?,
                        row.get(11)?,
                        row.get(12)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, 1);
        assert_eq!(row.1, "FourSeamFastball");
        assert_eq!(
            [
                row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9, row.10, row.11, row.12,
            ],
            [2.1, 2.2, 2.3, 2.4, 2.5, 2.6, 2.7, 2.8, 2.9, 3.0, 3.1]
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

        assert_eq!(names.first.as_ref(), "翔平");
        assert_eq!(names.last.as_ref(), "大谷");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_filters_out_non_male_first_names() {
        let (repo, path) = setup_repo();
        seed_first_name(&repo, "FemaleOnly", "F", "JP");
        seed_first_name(&repo, "MaleOnly", "M", "JP");
        seed_last_name(&repo, "大谷", "JP");

        let names = repo.random_name("JP".to_string()).unwrap();

        assert_eq!(names.first.as_ref(), "MaleOnly");
        assert_eq!(names.last.as_ref(), "大谷");
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

        assert_eq!(names.first.as_ref(), "翔平");
        assert_eq!(names.last.as_ref(), "大谷");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_returns_error_when_no_first_names_for_language() {
        let (repo, path) = setup_repo();
        seed_last_name(&repo, "大谷", "JP");

        let result = repo.random_name("JP".to_string());

        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_returns_error_when_no_last_names() {
        let (repo, path) = setup_repo();
        seed_first_name(&repo, "翔平", "M", "JP");

        let result = repo.random_name("JP".to_string());

        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn next_player_dist_team_returns_team_with_fewest_players() {
        let (repo, path) = setup_repo();
        seed_team(&repo, 1, "Full");
        seed_team(&repo, 2, "No Pitchers");
        seed_team(&repo, 3, "Half");
        seed_player_row(&repo, 1, 1);
        seed_player_row(&repo, 2, 1);
        seed_player_row(&repo, 3, 3);
        seed_defensive_skill(&repo, 1, Position::P);
        seed_defensive_skill(&repo, 2, Position::P);
        seed_defensive_skill(&repo, 3, Position::P);

        let team = repo.next_player_dist_team(Position::P).unwrap();

        assert_eq!(team.id, 2);
        assert_eq!(team.name.as_ref(), "No Pitchers");
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
        seed_defensive_skill(&repo, 3, Position::P);

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
    fn item_probs_returns_seeded_position_probabilities_for_category() {
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
    fn item_probs_filters_by_requested_category() {
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
    fn pitcher_style_probs_returns_seeded_pitcher_style_probabilities() {
        let (repo, path) = setup_repo();
        seed_pitcher_style_prob(&repo, PitcherStyle::PowerPitcher, 0.31);
        seed_pitcher_style_prob(&repo, PitcherStyle::FinessePitcher, 0.22);
        conn(&repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('position', 'P', 0.99)",
                [],
            )
            .unwrap();

        let pitcher_style_probs = repo.pitcher_style_probs().unwrap();

        assert_eq!(pitcher_style_probs.len(), 2);
        assert!(pitcher_style_probs.iter().any(|pitcher_style_prob| {
            matches!(pitcher_style_prob.name, PitcherStyle::PowerPitcher)
                && pitcher_style_prob.prob == 0.31
        }));
        assert!(pitcher_style_probs.iter().any(|pitcher_style_prob| {
            matches!(pitcher_style_prob.name, PitcherStyle::FinessePitcher)
                && pitcher_style_prob.prob == 0.22
        }));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn pitch_type_probs_returns_seeded_probabilities_for_pitcher_style() {
        let (repo, path) = setup_repo();
        seed_pitch_type_prob(
            &repo,
            PitcherStyle::PowerPitcher,
            PitchType::FourSeamFastball,
            1.0,
        );
        seed_pitch_type_prob(&repo, PitcherStyle::PowerPitcher, PitchType::Slider, 0.5);
        seed_pitch_type_prob(
            &repo,
            PitcherStyle::FinessePitcher,
            PitchType::Changeup,
            0.8,
        );

        let pitch_type_probs = repo.pitch_type_probs(&PitcherStyle::PowerPitcher).unwrap();

        assert_eq!(pitch_type_probs.len(), 2);
        assert!(pitch_type_probs.iter().any(|pitch_type_prob| {
            matches!(pitch_type_prob.name, PitchType::FourSeamFastball)
                && pitch_type_prob.prob == 1.0
        }));
        assert!(pitch_type_probs.iter().any(|pitch_type_prob| {
            matches!(pitch_type_prob.name, PitchType::Slider) && pitch_type_prob.prob == 0.5
        }));
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

        let player_attribute_probs = repo.player_attribute_prob().unwrap();

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

        let player_attribute_probs = repo.player_attribute_prob().unwrap();

        assert_eq!(player_attribute_probs.age_shape, 2.5);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn batter_skill_prob_returns_seeded_probabilities() {
        let (repo, path) = setup_repo();
        seed_batter_skill_prob(&repo, "ba_skew", 0.25);
        seed_batter_skill_prob(&repo, "slg_skew", 0.35);

        let batter_skill_prob = repo.batter_skill_prob().unwrap();

        assert_eq!(batter_skill_prob.ba_skew, 0.25);
        assert_eq!(batter_skill_prob.slg_skew, 0.35);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn batter_skill_prob_ignores_non_batter_skill_categories() {
        let (repo, path) = setup_repo();
        seed_batter_skill_prob(&repo, "ba_skew", 0.25);
        seed_batter_skill_prob(&repo, "slg_skew", 0.35);
        conn(&repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('player_attribute', 'ba_skew', 99.0)",
                [],
            )
            .unwrap();

        let batter_skill_prob = repo.batter_skill_prob().unwrap();

        assert_eq!(batter_skill_prob.ba_skew, 0.25);
        assert_eq!(batter_skill_prob.slg_skew, 0.35);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn defensive_skill_prob_returns_seeded_probabilities() {
        let (repo, path) = setup_repo();
        seed_defensive_skill_prob(&repo, "uzr_skew", 0.45);

        let defensive_skill_prob = repo.defensive_skill_prob().unwrap();

        assert_eq!(defensive_skill_prob.uzr_skew, 0.45);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn defensive_skill_prob_ignores_non_defensive_skill_categories() {
        let (repo, path) = setup_repo();
        seed_defensive_skill_prob(&repo, "uzr_skew", 0.45);
        conn(&repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('batter_skill', 'uzr_skew', 99.0)",
                [],
            )
            .unwrap();

        let defensive_skill_prob = repo.defensive_skill_prob().unwrap();

        assert_eq!(defensive_skill_prob.uzr_skew, 0.45);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn pitcher_attribute_prob_returns_seeded_probabilities() {
        let (repo, path) = setup_repo();
        seed_pitcher_attribute_prob(&repo, "velocity_skew", 0.11);
        seed_pitcher_attribute_prob(&repo, "control_skew", 0.12);
        seed_pitcher_attribute_prob(&repo, "stamina_skew", 0.13);
        seed_pitcher_attribute_prob(&repo, "injury_proneness_skew", 0.14);
        seed_pitcher_attribute_prob(&repo, "clutch_skew", 0.15);
        seed_pitcher_attribute_prob(&repo, "hpp_skew", 0.16);
        seed_pitcher_attribute_prob(&repo, "platoon_splitting_skew", 0.17);

        let pitcher_attribute_prob = repo.pitcher_attribute_prob().unwrap();

        assert_eq!(pitcher_attribute_prob.velocity_skew, 0.11);
        assert_eq!(pitcher_attribute_prob.control_skew, 0.12);
        assert_eq!(pitcher_attribute_prob.stamina_skew, 0.13);
        assert_eq!(pitcher_attribute_prob.injury_proneness_skew, 0.14);
        assert_eq!(pitcher_attribute_prob.clutch_skew, 0.15);
        assert_eq!(pitcher_attribute_prob.hpp_skew, 0.16);
        assert_eq!(pitcher_attribute_prob.platoon_splitting_skew, 0.17);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn pitcher_attribute_prob_ignores_non_pitcher_attribute_categories() {
        let (repo, path) = setup_repo();
        seed_pitcher_attribute_prob(&repo, "velocity_skew", 0.11);
        seed_pitcher_attribute_prob(&repo, "control_skew", 0.12);
        seed_pitcher_attribute_prob(&repo, "stamina_skew", 0.13);
        seed_pitcher_attribute_prob(&repo, "injury_proneness_skew", 0.14);
        seed_pitcher_attribute_prob(&repo, "clutch_skew", 0.15);
        seed_pitcher_attribute_prob(&repo, "hpp_skew", 0.16);
        seed_pitcher_attribute_prob(&repo, "platoon_splitting_skew", 0.17);
        conn(&repo)
            .execute(
                "INSERT INTO item_prob (category, name, prob)
                 VALUES ('batter_skill', 'velocity_skew', 99.0)",
                [],
            )
            .unwrap();

        let pitcher_attribute_prob = repo.pitcher_attribute_prob().unwrap();

        assert_eq!(pitcher_attribute_prob.velocity_skew, 0.11);
        assert_eq!(pitcher_attribute_prob.control_skew, 0.12);
        assert_eq!(pitcher_attribute_prob.stamina_skew, 0.13);
        assert_eq!(pitcher_attribute_prob.injury_proneness_skew, 0.14);
        assert_eq!(pitcher_attribute_prob.clutch_skew, 0.15);
        assert_eq!(pitcher_attribute_prob.hpp_skew, 0.16);
        assert_eq!(pitcher_attribute_prob.platoon_splitting_skew, 0.17);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn pitch_skill_prob_returns_seeded_probabilities_for_pitch_type() {
        let (repo, path) = setup_repo();
        seed_pitch_skill_prob(&repo, PitchType::Slider, "velocity_skew", 0.11);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "control_skew", 0.12);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "stamina_skew", 0.13);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "injury_proneness_skew", 0.14);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "stuff_skew", 0.15);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "fb_skew", 0.16);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "gp_skew", 0.17);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "horizontal_movement_skew", 0.18);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "vertical_movement_skew", 0.19);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "spin_rate_skew", 0.20);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "usage_skew", 0.21);

        let pitch_skill_prob = repo.pitch_skill_prob(&PitchType::Slider).unwrap();

        assert!(matches!(pitch_skill_prob.pitch_type, PitchType::Slider));
        assert_eq!(pitch_skill_prob.velocity_skew, 0.11);
        assert_eq!(pitch_skill_prob.control_skew, 0.12);
        assert_eq!(pitch_skill_prob.stamina_skew, 0.13);
        assert_eq!(pitch_skill_prob.injury_proneness_skew, 0.14);
        assert_eq!(pitch_skill_prob.stuff_skew, 0.15);
        assert_eq!(pitch_skill_prob.fb_skew, 0.16);
        assert_eq!(pitch_skill_prob.gp_skew, 0.17);
        assert_eq!(pitch_skill_prob.horizontal_movement_skew, 0.18);
        assert_eq!(pitch_skill_prob.vertical_movement_skew, 0.19);
        assert_eq!(pitch_skill_prob.spin_rate_skew, 0.20);
        assert_eq!(pitch_skill_prob.usage_skew, 0.21);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn pitch_skill_prob_ignores_other_pitch_type_categories() {
        let (repo, path) = setup_repo();
        seed_pitch_skill_prob(&repo, PitchType::Slider, "velocity_skew", 0.11);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "control_skew", 0.12);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "stamina_skew", 0.13);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "injury_proneness_skew", 0.14);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "stuff_skew", 0.15);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "fb_skew", 0.16);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "gp_skew", 0.17);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "horizontal_movement_skew", 0.18);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "vertical_movement_skew", 0.19);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "spin_rate_skew", 0.20);
        seed_pitch_skill_prob(&repo, PitchType::Slider, "usage_skew", 0.21);
        seed_pitch_skill_prob(&repo, PitchType::Changeup, "velocity_skew", 0.99);

        let pitch_skill_prob = repo.pitch_skill_prob(&PitchType::Slider).unwrap();

        assert_eq!(pitch_skill_prob.velocity_skew, 0.11);
        std::fs::remove_file(path).ok();
    }
}
