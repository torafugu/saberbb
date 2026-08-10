use crate::domain::shared::player::{
    BatterInfo, DefenseSkills, FielderInfo, FullName, OffenseSkills, PitchSkill, PitcherInfo,
    Player, Position, RunningSkills,
};
use crate::domain::shared::prob::ItemWeighted;
use crate::domain::shared::prob::{GammaParam, NormalParam};
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::db::FromRow;
use crate::repositories::db::{DbClient, SqlDb};
use anyhow::Result;
use rusqlite::{Transaction, params};
use tracing::info;

pub trait PlayerRepository {
    fn insert_player(&mut self, team_id: u16, player: &Player) -> Result<(), AppError>;
    fn insert_offense_skills(
        &self,
        tx: &Transaction,
        player_id: i64,
        offense_skills: &OffenseSkills,
    ) -> Result<(), AppError>;
    fn insert_batter_info(
        &self,
        tx: &Transaction,
        player_id: i64,
        batter_info: &BatterInfo,
    ) -> Result<usize, AppError>;
    fn insert_running_skills(
        &self,
        tx: &Transaction,
        player_id: i64,
        running_skills: &RunningSkills,
    ) -> Result<usize, AppError>;
    fn insert_defense_skills(
        &self,
        tx: &Transaction,
        player_id: i64,
        defense_skills: &DefenseSkills,
    ) -> Result<(), AppError>;
    fn insert_fielder_info(
        &self,
        tx: &Transaction,
        player_id: i64,
        fielder_info: &FielderInfo,
    ) -> Result<usize, AppError>;
    fn insert_pitcher_info(
        &self,
        tx: &Transaction,
        player_id: i64,
        pitcher_attribute: &PitcherInfo,
    ) -> Result<(), AppError>;
    fn insert_pitch_skill(
        &self,
        tx: &Transaction,
        player_id: i64,
        pitch_skill: &PitchSkill,
    ) -> Result<usize, AppError>;
    fn random_name(&self, language: String) -> Result<FullName, AppError>;
    fn next_player_dist_team(&self, position: Position) -> Result<Team, AppError>;
    fn next_random_team(&self) -> Result<Team, AppError>;
    fn normal_params(
        &self,
        category1: &str,
        category2: &str,
        name: &str,
    ) -> Result<NormalParam, AppError>;
    fn gamma_params(
        &self,
        category1: &str,
        category2: &str,
        name: &str,
    ) -> Result<GammaParam, AppError>;
    fn item_probs<T>(
        &self,
        category1: &str,
        category2: &str,
    ) -> Result<Vec<ItemWeighted<T>>, AppError>
    where
        ItemWeighted<T>: FromRow<Error = AppError>;
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
    #[tracing::instrument(skip(self, player), fields(team_id = %team_id), err)]
    fn insert_player(&mut self, team_id: u16, player: &Player) -> Result<(), AppError> {
        info!("insert_player() started");

        self.db_client.transaction(|tx| {
            let insert_player_sql = "INSERT INTO player_info (
                                        team_id, first_name, last_name, age, uniform_number
                                        ) VALUES (
                                        ?1, ?2, ?3, ?4, ?5)";
            let generated_id = self.db_client.execute_insert_tx(
                tx,
                insert_player_sql,
                params![
                    team_id,
                    player.info.first_name,
                    player.info.last_name,
                    player.info.age,
                    player.info.uniform_number
                ],
            )?;

            self.insert_offense_skills(tx, generated_id as i64, &player.offense_skills)?;
            self.insert_defense_skills(tx, generated_id as i64, &player.defense_skills)?;

            Ok(())
        })
    }

    #[tracing::instrument(skip(self, tx, offense_skills), fields(player_id = %player_id), err)]
    fn insert_offense_skills(
        &self,
        tx: &Transaction,
        player_id: i64,
        offense_skills: &OffenseSkills,
    ) -> Result<(), AppError> {
        info!("insert_offense_skills() started");

        if let Some(batter) = &offense_skills.batter {
            self.insert_batter_info(tx, player_id, &batter)?;
        }

        self.insert_running_skills(tx, player_id, &offense_skills.running)?;

        Ok(())
    }

    #[tracing::instrument(skip(self, tx, batter_info), fields(player_id = %player_id), err)]
    fn insert_batter_info(
        &self,
        tx: &Transaction,
        player_id: i64,
        batter_info: &BatterInfo,
    ) -> Result<usize, AppError> {
        info!("insert_batter_info() started");

        let insert_batter_info_sql = "INSERT INTO batter_info (
                                                player_id, batting_side, batting_eye, swing_speed, swing_power,
                                                attack_angle, bat_contact, timing_bias, consistency_sigma
                                                ) VALUES (
                                                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)";
        self.db_client.execute_tx(
            tx,
            insert_batter_info_sql,
            params![
                player_id,
                batter_info.batting_side,
                batter_info.batting_eye,
                batter_info.swing_speed,
                batter_info.swing_power,
                batter_info.attack_angle,
                batter_info.bat_contact,
                batter_info.timing_bias,
                batter_info.consistency_sigma,
            ],
        )
    }

    #[tracing::instrument(skip(self, tx, running_skills), fields(player_id = %player_id), err)]
    fn insert_running_skills(
        &self,
        tx: &Transaction,
        player_id: i64,
        running_skills: &RunningSkills,
    ) -> Result<usize, AppError> {
        info!("insert_running_skills() started");

        let insert_running_skills_sql = "INSERT INTO running_skills (
                                                player_id, speed, lead_distance, start_reaction
                                                ) VALUES (
                                                ?1, ?2, ?3, ?4)";
        self.db_client.execute_tx(
            tx,
            insert_running_skills_sql,
            params![
                player_id,
                running_skills.speed,
                running_skills.lead_distance,
                running_skills.start_reaction,
            ],
        )
    }

    #[tracing::instrument(skip(self, tx, defense_skills), fields(player_id = %player_id), err)]
    fn insert_defense_skills(
        &self,
        tx: &Transaction,
        player_id: i64,
        defense_skills: &DefenseSkills,
    ) -> Result<(), AppError> {
        info!("insert_defense_skills() started");

        let insert_defensive_skills_sql = "INSERT INTO defense_skills (
                                                player_id, position
                                                ) VALUES (
                                                ?1, ?2)";
        self.db_client.execute_tx(
            tx,
            insert_defensive_skills_sql,
            params![player_id, defense_skills.position,],
        )?;

        if let Some(pitcher) = &defense_skills.pitcher {
            self.insert_fielder_info(tx, player_id, &pitcher.fielder_info)?;
            self.insert_pitcher_info(tx, player_id, &pitcher)?;
        }

        if let Some(catcher) = &defense_skills.catcher {
            self.insert_fielder_info(tx, player_id, &catcher.fielder_info)?;
        }

        if let Some(middle_infielder) = &defense_skills.middle_infielder {
            self.insert_fielder_info(tx, player_id, &middle_infielder)?;
        }

        if let Some(corner_infielder) = &defense_skills.corner_infielder {
            self.insert_fielder_info(tx, player_id, &corner_infielder)?;
        }

        if let Some(outfielder) = &defense_skills.outfielder {
            self.insert_fielder_info(tx, player_id, &outfielder)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self, tx, fielder_info), fields(player_id = %player_id, fielder_type = %fielder_info.fielder_type), err)]
    fn insert_fielder_info(
        &self,
        tx: &Transaction,
        player_id: i64,
        fielder_info: &FielderInfo,
    ) -> Result<usize, AppError> {
        info!("insert_fielder_info() started");

        let insert_fielder_info_sql = "INSERT INTO fielder_info (
                                                player_id, fielder_type, throw_speed, running_speed, reaction, prep_time
                                                ) VALUES (
                                                ?1, ?2, ?3, ?4, ?5, ?6)";
        self.db_client.execute_tx(
            tx,
            insert_fielder_info_sql,
            params![
                player_id,
                fielder_info.fielder_type,
                fielder_info.throw_speed,
                fielder_info.running_speed,
                fielder_info.reaction,
                fielder_info.prep_time
            ],
        )
    }

    #[tracing::instrument(skip(self, tx, pitcher_info), fields(player_id = %player_id), err)]
    fn insert_pitcher_info(
        &self,
        tx: &Transaction,
        player_id: i64,
        pitcher_info: &PitcherInfo,
    ) -> Result<(), AppError> {
        info!("insert_pitcher_info() started");

        let insert_pitcher_info_sql = "INSERT INTO pitcher_info (
                                                            player_id,
                                                            height,
                                                            extension,
                                                            throw_side,
                                                            arm_slot,
                                                            pitcher_style,
                                                            velocity,
                                                            control,
                                                            stamina,
                                                            injury_proneness,
                                                            clutch,
                                                            hpp,
                                                            platoon_splitting,
                                                            delivery_motion_time
                                                            ) VALUES (
                                                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)";
        self.db_client.execute_tx(
            tx,
            insert_pitcher_info_sql,
            params![
                player_id,
                pitcher_info.height,
                pitcher_info.extension,
                pitcher_info.throw_side,
                pitcher_info.arm_slot,
                pitcher_info.pitcher_style,
                pitcher_info.velocity,
                pitcher_info.control,
                pitcher_info.stamina,
                pitcher_info.injury_proneness,
                pitcher_info.clutch,
                pitcher_info.hpp,
                pitcher_info.platoon_splitting,
                pitcher_info.delivery_motion_time
            ],
        )?;

        for pitch_skill in &pitcher_info.pitch_skills {
            self.insert_pitch_skill(tx, player_id, &pitch_skill)?;
        }

        Ok(())
    }

    #[tracing::instrument(skip(self, tx, pitch_skill), fields(player_id = %player_id, pitch_type = %pitch_skill.pitch_type), err)]
    fn insert_pitch_skill(
        &self,
        tx: &Transaction,
        player_id: i64,
        pitch_skill: &PitchSkill,
    ) -> Result<usize, AppError> {
        info!("insert_pitch_skill() started");

        let insert_pitch_skill_sql = "INSERT INTO pitch_skill (
                                                            player_id,
                                                            pitch_type,
                                                            velocity,
                                                            control,
                                                            stamina,
                                                            injury_proneness,
                                                            spin_rate,
                                                            spin_angle,
                                                            spin_efficiency,
                                                            usage
                                                        ) VALUES (
                                                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)";
        self.db_client.execute_tx(
            tx,
            insert_pitch_skill_sql,
            params![
                player_id,
                pitch_skill.pitch_type,
                pitch_skill.velocity,
                pitch_skill.control,
                pitch_skill.stamina,
                pitch_skill.injury_proneness,
                pitch_skill.spin_rate,
                pitch_skill.spin_angle,
                pitch_skill.spin_efficiency,
                pitch_skill.usage,
            ],
        )
    }

    #[tracing::instrument(skip(self), fields(language = %language), err)]
    fn random_name(&self, language: String) -> Result<FullName, AppError> {
        info!("random_name() started");

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

    #[tracing::instrument(skip(self, position), fields(position = %position), err)]
    fn next_player_dist_team(&self, position: Position) -> Result<Team, AppError> {
        info!("next_player_dist_team() started for {}", position);

        let query = "SELECT
                            t.id AS id,
                            t.name AS name,
                            COUNT(ds.player_id) AS player_count
                            FROM team t
                            LEFT JOIN player_info p ON t.id = p.team_id
                            LEFT JOIN defense_skills ds ON ds.player_id = p.id
                                AND ds.position = ?1
                            GROUP BY t.id, t.name
                            ORDER BY player_count, t.id
                            LIMIT 1";
        self.db_client.query_row::<Team>(query, params![position])
    }

    #[tracing::instrument(skip(self), err)]
    fn next_random_team(&self) -> Result<Team, AppError> {
        info!("next_random_team() started");

        let query = "SELECT id, name
                    FROM team
                    ORDER BY RANDOM() 
                    LIMIT 1";
        self.db_client.query_row::<Team>(query, params![])
    }

    #[tracing::instrument(skip(self), fields(category1 = %category1, category2 = %category2, name = %name), err)]
    fn normal_params(
        &self,
        category1: &str,
        category2: &str,
        name: &str,
    ) -> Result<NormalParam, AppError> {
        info!("normal_params() started");

        let query = "SELECT 
                        mean, std_dev, skew, coefficient, offset
                        FROM normal_param
                        WHERE category1 = ?1 AND category2 = ?2 AND name = ?3";
        self.db_client
            .query_row::<NormalParam>(query, params![category1, category2, name])
    }

    #[tracing::instrument(skip(self), fields(category1 = %category1, category2 = %category2, name = %name), err)]
    fn gamma_params(
        &self,
        category1: &str,
        category2: &str,
        name: &str,
    ) -> Result<GammaParam, AppError> {
        info!("gamma_params() started");

        let query = "SELECT 
                        shape, scale, offset 
                        FROM gamma_param
                        WHERE category1 = ?1 AND category2 = ?2 AND name = ?3";
        self.db_client
            .query_row::<GammaParam>(query, params![category1, category2, name])
    }

    #[tracing::instrument(skip(self), fields(category1 = %category1, category2 = %category2), err)]
    fn item_probs<T>(
        &self,
        category1: &str,
        category2: &str,
    ) -> Result<Vec<ItemWeighted<T>>, AppError>
    where
        ItemWeighted<T>: FromRow<Error = AppError>,
    {
        info!(
            "item_probs() started for category1:{} and category2:{}",
            category1, category2
        );

        let query = "SELECT name, weight AS prob FROM item_weighted WHERE category1 = ?1 AND category2 = ?2";
        self.db_client
            .query_rows::<ItemWeighted<T>>(query, params![category1, category2])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::{
        ArmSlot, BatterInfo, DefenseSkills, FielderInfo, FielderType, OffenseSkills, PitchSkill,
        PitchType, PitcherInfo, PitcherStyle, PlayerInfo, Position, RL, RunningSkills,
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

            CREATE TABLE player_info (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                team_id INTEGER NOT NULL,
                first_name TEXT NOT NULL,
                last_name TEXT NOT NULL,
                age INTEGER NOT NULL,
                uniform_number INTEGER NOT NULL
            );

            CREATE TABLE batter_info (
                player_id INTEGER PRIMARY KEY,
                batting_side TEXT NOT NULL,
                batting_eye REAL NOT NULL,
                swing_speed REAL NOT NULL,
                swing_power REAL NOT NULL,
                attack_angle REAL NOT NULL,
                bat_contact REAL NOT NULL,
                timing_bias REAL NOT NULL,
                consistency_sigma REAL NOT NULL
            );

            CREATE TABLE running_skills (
                player_id INTEGER PRIMARY KEY,
                speed REAL NOT NULL,
                lead_distance REAL NOT NULL,
                start_reaction REAL NOT NULL
            );

            CREATE TABLE defense_skills (
                player_id INTEGER PRIMARY KEY,
                position TEXT NOT NULL
            );

            CREATE TABLE fielder_info (
                player_id INTEGER,
                fielder_type TEXT,
                throw_speed REAL NOT NULL,
                running_speed REAL NOT NULL,
                reaction REAL NOT NULL,
                prep_time REAL NOT NULL,
                PRIMARY KEY (player_id, fielder_type)
            );

            CREATE TABLE pitcher_info (
                player_id INTEGER PRIMARY KEY,
                height REAL NOT NULL,
                extension REAL NOT NULL,
                throw_side TEXT NOT NULL,
                arm_slot TEXT NOT NULL,
                pitcher_style TEXT NOT NULL,
                velocity REAL NOT NULL,
                control REAL NOT NULL,
                stamina REAL NOT NULL,
                injury_proneness REAL NOT NULL,
                clutch REAL NOT NULL,
                hpp REAL NOT NULL,
                platoon_splitting REAL NOT NULL,
                delivery_motion_time REAL NOT NULL
            );

            CREATE TABLE pitch_skill (
                player_id INTEGER,
                pitch_type TEXT,
                velocity REAL NOT NULL,
                control REAL NOT NULL,
                stamina REAL NOT NULL,
                injury_proneness REAL NOT NULL,
                spin_rate REAL NOT NULL,
                spin_angle REAL NOT NULL,
                spin_efficiency REAL NOT NULL,
                usage REAL NOT NULL,
                PRIMARY KEY (player_id, pitch_type)
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

            CREATE TABLE normal_param (
                category1 TEXT,
                category2 TEXT,
                name TEXT,
                mean REAL NOT NULL,
                std_dev REAL NOT NULL,
                skew REAL NOT NULL,
                coefficient REAL NOT NULL,
                offset REAL NOT NULL,
                PRIMARY KEY (category1, category2, name)
            );

            CREATE TABLE gamma_param (
                category1 TEXT,
                category2 TEXT,
                name TEXT,
                shape REAL NOT NULL,
                scale REAL NOT NULL,
                offset REAL NOT NULL,
                PRIMARY KEY (category1, category2, name)
            );

            CREATE TABLE item_weighted (
                category1 TEXT,
                category2 TEXT,
                name TEXT NOT NULL,
                weight REAL NOT NULL
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
                "INSERT INTO player_info (
                    id, team_id, first_name, last_name, age, uniform_number
                ) VALUES (?1, ?2, ?3, ?4, 25, 17)",
                params![id, team_id, format!("First{id}"), format!("Last{id}")],
            )
            .unwrap();
    }

    fn seed_defensive_skill(repo: &SqlPlayerRepository, player_id: u32, position: Position) {
        conn(repo)
            .execute(
                "INSERT INTO defense_skills (player_id, position)
                 VALUES (?1, ?2)",
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
                "INSERT INTO item_weighted (category1, category2, name, weight)
                 VALUES ('player', 'position', ?1, ?2)",
                params![position, prob],
            )
            .unwrap();
    }

    fn seed_pitcher_style_prob(repo: &SqlPlayerRepository, pitcher_style: PitcherStyle, prob: f64) {
        conn(repo)
            .execute(
                "INSERT INTO item_weighted (category1, category2, name, weight)
                 VALUES ('player', 'pitcher_info', ?1, ?2)",
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
                "INSERT INTO item_weighted (category1, category2, name, weight)
                 VALUES (?1, 'pitch_type', ?2, ?3)",
                params![pitcher_style.as_ref(), pitch_type, prob],
            )
            .unwrap();
    }

    fn seed_normal_param(
        repo: &SqlPlayerRepository,
        category1: &str,
        category2: &str,
        name: &str,
        mean: f64,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO normal_param (
                    category1, category2, name, mean, std_dev, skew, coefficient, offset
                ) VALUES (?1, ?2, ?3, ?4, 1.2, 1.3, 1.4, 1.5)",
                params![category1, category2, name, mean],
            )
            .unwrap();
    }

    fn seed_gamma_param(
        repo: &SqlPlayerRepository,
        category1: &str,
        category2: &str,
        name: &str,
        shape: f64,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO gamma_param (
                    category1, category2, name, shape, scale, offset
                ) VALUES (?1, ?2, ?3, ?4, 0.5, 18.0)",
                params![category1, category2, name, shape],
            )
            .unwrap();
    }

    fn player() -> Player {
        Player {
            info: PlayerInfo::new_unsaved("翔平".into(), "大谷".into(), 29, 17),
            offense_skills: OffenseSkills {
                batter: Some(BatterInfo {
                    batting_side: RL::Right,
                    batting_eye: 0.5,
                    swing_speed: 1.0,
                    swing_power: 1.1,
                    attack_angle: 28.0,
                    bat_contact: 0.8,
                    timing_bias: 0.0,
                    consistency_sigma: 0.03,
                }),
                running: RunningSkills {
                    speed: 7.1,
                    lead_distance: 2.2,
                    start_reaction: 0.3,
                },
            },
            defense_skills: DefenseSkills::new(Position::CF),
        }
    }

    fn fielder_info(fielder_type: FielderType) -> FielderInfo {
        FielderInfo {
            fielder_type,
            throw_speed: 35.0,
            running_speed: 7.0,
            reaction: 0.4,
            prep_time: 0.6,
        }
    }

    #[test]
    fn save_player_inserts_all_player_fields() {
        let (mut repo, path) = setup_repo();
        seed_team(&repo, 1, "ライオンズ");

        repo.insert_player(1, &player()).unwrap();

        let conn = conn(&repo);
        let row: (u16, String, String, u8, u8) = conn
            .query_row(
                "SELECT team_id, first_name, last_name, age, uniform_number
                 FROM player_info",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, 1);
        assert_eq!(row.1, "翔平");
        assert_eq!(row.2, "大谷");
        assert_eq!(row.3, 29);
        assert_eq!(row.4, 17);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_player_inserts_defensive_skills() {
        let (mut repo, path) = setup_repo();
        seed_team(&repo, 1, "ライオンズ");
        let mut player = player();
        player.defense_skills.position = Position::P;

        repo.insert_player(1, &player).unwrap();

        let conn = conn(&repo);
        let position: String = conn
            .query_row("SELECT position FROM defense_skills", [], |row| row.get(0))
            .unwrap();

        assert_eq!(position, "P");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_player_inserts_pitcher_info_when_present() {
        let (mut repo, path) = setup_repo();
        seed_team(&repo, 1, "ライオンズ");
        let mut player = player();
        player.defense_skills.position = Position::P;
        player.defense_skills.pitcher = Some(PitcherInfo {
            height: 1.85,
            extension: 1.8,
            throw_side: RL::Left,
            arm_slot: ArmSlot::Sidearm,
            pitcher_style: PitcherStyle::BalancedPitcher,
            velocity: 1.1,
            control: 1.2,
            stamina: 1.3,
            injury_proneness: 1.4,
            clutch: 1.5,
            hpp: 1.6,
            platoon_splitting: 1.7,
            delivery_motion_time: 1.8,
            pitch_skills: Vec::new(),
            fielder_info: fielder_info(FielderType::Pitcher),
        });

        repo.insert_player(1, &player).unwrap();

        let conn = conn(&repo);
        let row: (
            u32,
            String,
            String,
            String,
            [f64; 10],
        ) = conn
            .query_row(
                "SELECT player_id, throw_side, arm_slot, pitcher_style, velocity, control, stamina,
                    height, extension, injury_proneness, clutch, hpp, platoon_splitting, delivery_motion_time
                 FROM pitcher_info",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        [
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                            row.get(8)?,
                            row.get(9)?,
                            row.get(10)?,
                            row.get(11)?,
                            row.get(12)?,
                            row.get(13)?,
                        ],
                    ))
                },
            )
            .unwrap();

        assert_eq!(
            row,
            (
                1,
                "Left".to_string(),
                "Sidearm".to_string(),
                "BalancedPitcher".to_string(),
                [1.1, 1.2, 1.3, 1.85, 1.8, 1.4, 1.5, 1.6, 1.7, 1.8]
            )
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_player_inserts_pitch_skills_when_pitcher_attribute_is_present() {
        let (mut repo, path) = setup_repo();
        seed_team(&repo, 1, "ライオンズ");
        let mut player = player();
        player.defense_skills.position = Position::P;
        player.defense_skills.pitcher = Some(PitcherInfo {
            height: 1.85,
            extension: 1.8,
            throw_side: RL::Right,
            arm_slot: ArmSlot::ThreeQuarter,
            pitcher_style: PitcherStyle::PowerPitcher,
            velocity: 1.1,
            control: 1.2,
            stamina: 1.3,
            injury_proneness: 1.4,
            clutch: 1.5,
            hpp: 1.6,
            platoon_splitting: 1.7,
            delivery_motion_time: 1.8,
            pitch_skills: vec![PitchSkill {
                pitch_type: PitchType::FourSeamFastball,
                velocity: 2.1,
                control: 2.2,
                stamina: 2.3,
                injury_proneness: 2.4,
                spin_rate: 3.0,
                spin_angle: 3.1,
                spin_efficiency: 3.2,
                usage: 3.3,
            }],
            fielder_info: fielder_info(FielderType::Pitcher),
        });

        repo.insert_player(1, &player).unwrap();

        let conn = conn(&repo);
        let row: (u32, String, f64, f64, f64, f64, f64, f64, f64, f64) = conn
            .query_row(
                "SELECT player_id, pitch_type, velocity, control, stamina,
                    injury_proneness, spin_rate, spin_angle, spin_efficiency, usage
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
                    ))
                },
            )
            .unwrap();

        assert_eq!(row.0, 1);
        assert_eq!(row.1, "FourSeamFastball");
        assert_eq!(
            [row.2, row.3, row.4, row.5, row.6, row.7, row.8, row.9],
            [2.1, 2.2, 2.3, 2.4, 3.0, 3.1, 3.2, 3.3]
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn save_player_returns_error_when_player_table_is_missing() {
        let (mut repo, path) = setup_repo_without_player_table();
        seed_team(&repo, 1, "ライオンズ");

        let result = repo.insert_player(1, &player());

        assert!(result.is_err());
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_returns_matching_first_and_last_name_for_language() {
        let (repo, path) = setup_repo();
        seed_first_name(&repo, "翔平", "M", "JP");
        seed_last_name(&repo, "大谷", "JP");

        let names = repo.random_name("JP".to_string()).unwrap();

        assert_eq!(names.first, "翔平");
        assert_eq!(names.last, "大谷");
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn random_name_filters_out_non_male_first_names() {
        let (repo, path) = setup_repo();
        seed_first_name(&repo, "FemaleOnly", "F", "JP");
        seed_first_name(&repo, "MaleOnly", "M", "JP");
        seed_last_name(&repo, "大谷", "JP");

        let names = repo.random_name("JP".to_string()).unwrap();

        assert_eq!(names.first, "MaleOnly");
        assert_eq!(names.last, "大谷");
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

        assert_eq!(names.first, "翔平");
        assert_eq!(names.last, "大谷");
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

        let position_probs: Vec<ItemWeighted<Position>> =
            repo.item_probs("player", "position").unwrap();

        assert_eq!(position_probs.len(), 2);
        assert!(
            position_probs
                .iter()
                .any(|prob| prob.name == Position::P && prob.weight == 0.42)
        );
        assert!(
            position_probs
                .iter()
                .any(|prob| prob.name == Position::CF && prob.weight == 0.07)
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn item_probs_filters_by_requested_category() {
        let (repo, path) = setup_repo();
        seed_position_prob(&repo, Position::P, 0.42);
        conn(&repo)
            .execute(
                "INSERT INTO item_weighted (category1, category2, name, weight)
                 VALUES ('player', 'batting_side', 'Right', 0.6)",
                [],
            )
            .unwrap();

        let position_probs: Vec<ItemWeighted<Position>> =
            repo.item_probs("player", "position").unwrap();

        assert_eq!(position_probs.len(), 1);
        assert_eq!(position_probs[0].name, Position::P);
        assert_eq!(position_probs[0].weight, 0.42);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn item_probs_returns_seeded_pitcher_style_probabilities() {
        let (repo, path) = setup_repo();
        seed_pitcher_style_prob(&repo, PitcherStyle::PowerPitcher, 0.31);
        seed_pitcher_style_prob(&repo, PitcherStyle::FinessePitcher, 0.22);

        let pitcher_style_probs: Vec<ItemWeighted<PitcherStyle>> =
            repo.item_probs("player", "pitcher_info").unwrap();

        assert_eq!(pitcher_style_probs.len(), 2);
        assert!(pitcher_style_probs.iter().any(|prob| {
            matches!(prob.name, PitcherStyle::PowerPitcher) && prob.weight == 0.31
        }));
        assert!(pitcher_style_probs.iter().any(|prob| {
            matches!(prob.name, PitcherStyle::FinessePitcher) && prob.weight == 0.22
        }));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn item_probs_returns_seeded_pitch_type_probabilities_for_pitcher_style() {
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

        let pitch_type_probs: Vec<ItemWeighted<PitchType>> = repo
            .item_probs(PitcherStyle::PowerPitcher.as_ref(), "pitch_type")
            .unwrap();

        assert_eq!(pitch_type_probs.len(), 2);
        assert!(pitch_type_probs.iter().any(|prob| {
            matches!(prob.name, PitchType::FourSeamFastball) && prob.weight == 1.0
        }));
        assert!(
            pitch_type_probs
                .iter()
                .any(|prob| matches!(prob.name, PitchType::Slider) && prob.weight == 0.5)
        );
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn normal_params_returns_seeded_values() {
        let (repo, path) = setup_repo();
        seed_normal_param(&repo, "player", "running_skills", "speed", 7.2);

        let params = repo
            .normal_params("player", "running_skills", "speed")
            .unwrap();

        assert_eq!(params.mean, 7.2);
        assert_eq!(params.std_dev, 1.2);
        assert_eq!(params.skew, 1.3);
        assert_eq!(params.coefficient, 1.4);
        assert_eq!(params.offset, 1.5);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn normal_params_filters_by_categories_and_name() {
        let (repo, path) = setup_repo();
        seed_normal_param(&repo, "player", "running_skills", "speed", 7.2);
        seed_normal_param(&repo, "player", "fielder_info", "speed", 99.0);

        let params = repo
            .normal_params("player", "running_skills", "speed")
            .unwrap();

        assert_eq!(params.mean, 7.2);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn gamma_params_returns_seeded_values() {
        let (repo, path) = setup_repo();
        seed_gamma_param(&repo, "player", "player_info", "age", 2.5);

        let params = repo.gamma_params("player", "player_info", "age").unwrap();

        assert_eq!(params.shape, 2.5);
        assert_eq!(params.scale, 0.5);
        assert_eq!(params.offset, 18.0);
        std::fs::remove_file(path).ok();
    }
}
