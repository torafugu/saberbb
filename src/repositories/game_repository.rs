use crate::domain::shared::game::{
    Count, GameDetail, GameHeader, GameResult, GameSchedule, Inning, TB,
};
use crate::domain::shared::game_stats::{
    PlayerGameBatting, PlayerGameBattingView, PlayerGameEntry, PlayerGameEntryView,
    PlayerGameFielding, PlayerGameRunning, PlayerGameRunningView,
};
use crate::domain::shared::player::{
    BatterInfo, CatcherInfo, DefenseSkills, FielderInfo, FielderType, PitchSkill, PitcherInfo,
    Player, PlayerInfo, Position, RunningSkills,
};
use crate::error::AppError;
use crate::repositories::db::{DbClient, SqlDb};
use anyhow::Result;
use rusqlite::{params, Transaction};
use tracing::info;

pub trait GameRepository {
    fn update_game_result(&mut self, game: &GameResult) -> Result<(), AppError>;
    fn insert_player_entry(
        &self,
        tx: &Transaction,
        game_id: u32,
        player_game_entry: &PlayerGameEntry,
    ) -> Result<usize, AppError>;
    fn insert_player_batting(
        &self,
        tx: &Transaction,
        game_id: u32,
        player_game_batting: &PlayerGameBatting,
    ) -> Result<usize, AppError>;
    fn insert_player_fielding(
        &self,
        tx: &Transaction,
        game_id: u32,
        player_game_fielding: &PlayerGameFielding,
    ) -> Result<usize, AppError>;
    fn insert_player_running(
        &self,
        tx: &Transaction,
        game_id: u32,
        player_game_running: &PlayerGameRunning,
    ) -> Result<usize, AppError>;
    fn update_current_round_seq(&mut self) -> Result<usize, AppError>;
    fn load_processed_seasons(&self) -> Result<Vec<u16>, AppError>;
    fn load_processed_game_headers(&self, season: u16) -> Result<Vec<GameHeader>, AppError>;
    fn load_game_schedules_to_process(&self) -> Result<Vec<GameSchedule>, AppError>;
    fn load_game_detail(&self, game_id: u32) -> Result<GameDetail, AppError>;
    fn load_team_players(&self, team_id: u16) -> Result<Vec<Player>, AppError>;
    fn load_running_skills(&self, player_id: i64) -> Result<RunningSkills, AppError>;
    fn load_batter_info(&self, player_id: i64) -> Result<BatterInfo, AppError>;
    fn load_fielder_info(
        &self,
        player_id: i64,
        fielder_type: FielderType,
    ) -> Result<FielderInfo, AppError>;
    fn load_pitcher_info(&self, player_id: i64) -> Result<PitcherInfo, AppError>;
    fn load_pitch_skill(&self, player_id: i64) -> Result<Vec<PitchSkill>, AppError>;
    fn load_defense_skills(&self, player_id: i64) -> Result<DefenseSkills, AppError>;
    fn load_innings(&self, game_id: u32) -> Result<Vec<Inning>, AppError>;
    fn load_player_game_entry_views(
        &self,
        game_id: u32,
    ) -> Result<Vec<PlayerGameEntryView>, AppError>;
    fn load_player_game_batting_views(
        &self,
        game_id: u32,
    ) -> Result<Vec<PlayerGameBattingView>, AppError>;
    fn load_player_game_running_views(
        &self,
        game_id: u32,
    ) -> Result<Vec<PlayerGameRunningView>, AppError>;
    fn load_counts(
        &self,
        game_id: u32,
        inning_seq: u8,
        inning_tb: TB,
    ) -> Result<Vec<Count>, AppError>;
}

#[derive(Clone, Debug)]
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
    #[tracing::instrument(skip(self, game), fields(game_id = %game.id))]
    fn update_game_result(&mut self, game: &GameResult) -> Result<(), AppError> {
        info!("save_game_result() started");
        self.db_client.transaction(|tx| {
            let update_game_sql =
                "UPDATE game SET actual_date = ?1, away_points = ?2, home_points = ?3 WHERE id = ?4";
            self.db_client.execute_tx(
                tx,
                update_game_sql,
                params![
                    game.actual_date,
                    game.away_total_point,
                    game.home_total_point,
                    game.id
                ],
            )?;

            let insert_inning_sql = "INSERT INTO inning (game_id, seq, tb) VALUES (?1, ?2, ?3)";
            let insert_count_sql = "INSERT INTO count (
                            game_id, inning_seq, inning_tb, seq, point, ball, strike, out
                            ) VALUES (
                            ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)";

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
                            count.point,
                            count.ball,
                            count.strike,
                            count.out
                        ],
                    )?;
                }
            }

            for player_game_entry in &game.player_entries {
                self.insert_player_entry(tx, game.id, player_game_entry)?;
            }

            for player_game_batting in &game.player_battings {
                self.insert_player_batting(tx, game.id, player_game_batting)?;
            }


            for player_game_fielding in &game.player_fieldings {
                self.insert_player_fielding(tx, game.id, player_game_fielding)?;
            }

            for player_game_running in &game.player_runnings {
                self.insert_player_running(tx, game.id, player_game_running)?;
            }

            Ok(())
        })
    }

    #[tracing::instrument(skip(self, tx), fields(count_seq = %player_game_entry.start_count_seq, player_id = %player_game_entry.player_id))]
    fn insert_player_entry(
        &self,
        tx: &Transaction,
        game_id: u32,
        player_game_entry: &PlayerGameEntry,
    ) -> Result<usize, AppError> {
        info!(
            "insert_player_entry() started for Start Count:{}, Player ID:{}",
            player_game_entry.start_count_seq, player_game_entry.player_id
        );

        let end_count_seq = if let Some(seq) = player_game_entry.end_count_seq {
            seq
        } else {
            0
        };

        let insert_player_game_entry_sql = "INSERT INTO player_game_entry (
                    game_id, start_count_seq, end_count_seq, position, batting_order, player_id
                    ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6)";
        self.db_client.execute_tx(
            tx,
            insert_player_game_entry_sql,
            params![
                game_id,
                player_game_entry.start_count_seq,
                end_count_seq,
                player_game_entry.position,
                player_game_entry.batting_order,
                player_game_entry.player_id
            ],
        )
    }

    #[tracing::instrument(skip(self, tx), fields(count_seq = %player_game_batting.count_seq))]
    fn insert_player_batting(
        &self,
        tx: &Transaction,
        game_id: u32,
        player_game_batting: &PlayerGameBatting,
    ) -> Result<usize, AppError> {
        info!(
            "insert_player_batting() for Started Count:{}",
            player_game_batting.count_seq
        );

        let fielder_position_str: Option<&str> = player_game_batting
            .fielder_position
            .as_ref()
            .map(|p| p.as_ref());

        let insert_player_game_batting_sql =
                "INSERT INTO player_game_batting (
                    game_id, count_seq, pitcher_id, batter_id, launch_speed, launch_angle, polar_distance, polar_angle, 
                    hang_time, trajectory, fielder_position, result
                    ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)";
        self.db_client.execute_tx(
            tx,
            insert_player_game_batting_sql,
            params![
                game_id,
                player_game_batting.count_seq,
                player_game_batting.pitcher_id,
                player_game_batting.batter_id,
                player_game_batting.ball.launch_speed_kmh,
                player_game_batting.ball.launch_angle,
                player_game_batting.ball.polar_position.distance,
                player_game_batting.ball.polar_position.angle,
                player_game_batting.ball.hang_time,
                player_game_batting.ball.trajectory,
                fielder_position_str,
                player_game_batting.result
            ],
        )
    }

    #[tracing::instrument(skip(self, tx), fields(count_seq = %player_game_fielding.count_seq, seq = player_game_fielding.seq))]
    fn insert_player_fielding(
        &self,
        tx: &Transaction,
        game_id: u32,
        player_game_fielding: &PlayerGameFielding,
    ) -> Result<usize, AppError> {
        info!(
            "insert_player_fielding() for Count:{}, Seq:{}",
            player_game_fielding.count_seq, player_game_fielding.seq
        );

        let insert_player_game_fielding_sql =
                "INSERT INTO player_game_fielding (
                    game_id, count_seq, seq, catch_fielder_id, catch_fielder_position, cutoff_fielder_id, cutoff_fielder_position, 
                    final_fielder_id, final_fielder_position, time_to_field, play_type
                    ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)";
        self.db_client.execute_tx(
            tx,
            insert_player_game_fielding_sql,
            params![
                game_id,
                player_game_fielding.count_seq,
                player_game_fielding.seq,
                player_game_fielding.catch_fielder_id,
                player_game_fielding.catch_fielder_position,
                player_game_fielding.cutoff_fielder_id,
                player_game_fielding.cutoff_fielder_position,
                player_game_fielding.final_fielder_id,
                player_game_fielding.final_fielder_position,
                player_game_fielding.time_to_field,
                player_game_fielding.play_type
            ],
        )
    }

    #[tracing::instrument(skip(self, tx), fields(count_seq = %player_game_running.count_seq, seq = player_game_running.seq))]
    fn insert_player_running(
        &self,
        tx: &Transaction,
        game_id: u32,
        player_game_running: &PlayerGameRunning,
    ) -> Result<usize, AppError> {
        info!(
            "insert_player_fielding() for Count:{}, Seq:{}",
            player_game_running.count_seq, player_game_running.seq
        );

        let insert_player_game_running_sql =
                "INSERT INTO player_game_running (
                    game_id, count_seq, seq, defense_time, runner_time, throw_target_base, event,
                    play_type, 
                    ruling, runs_scored, target_runner_id, runner_1st_id, runner_2nd_id, runner_3rd_id
                    ) VALUES (
                    ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)";
        self.db_client.execute_tx(
            tx,
            insert_player_game_running_sql,
            params![
                game_id,
                player_game_running.count_seq,
                player_game_running.seq,
                player_game_running.defense_time,
                player_game_running.runner_time,
                player_game_running.throw_target_base,
                player_game_running.event.as_ref(),
                player_game_running.play_type,
                player_game_running.ruling,
                player_game_running.runs_scored,
                player_game_running.target_runner_id,
                player_game_running.runner_1st_id,
                player_game_running.runner_2nd_id,
                player_game_running.runner_3rd_id
            ],
        )
    }

    fn update_current_round_seq(&mut self) -> Result<usize, AppError> {
        info!("update_current_round_seq() started");
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

    fn load_game_schedules_to_process(&self) -> Result<Vec<GameSchedule>, AppError> {
        info!("load_game_schedules_to_process() started");
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
                            g.stadium_id AS stadium_id,
                            st.name AS stadium_name,
                            st.foul_pole_distance AS stadium_foul_pole_distance,
                            st.center_fence_distance AS stadium_center_fence_distance,
                            st.fence_height AS stadium_fence_height,
                            g.game_type
                            FROM game g
                            INNER JOIN game_season s
                    	        ON s.current_season = g.season
						        AND s.current_round_seq = g.round_seq
					        LEFT JOIN 
                		        team t_away ON g.away_team_id = t_away.id
            		        LEFT JOIN 
                		        team t_home ON g.home_team_id = t_home.id
                            LEFT JOIN
                                stadium st ON g.stadium_id = st.id
                            ORDER BY round_seq, seq DESC";
        let mut game_schedules = self
            .db_client
            .query_rows::<GameSchedule>(query, params![])?;

        for game_schedule in &mut game_schedules {
            game_schedule.away_team.players = self.load_team_players(game_schedule.away_team.id)?;
            game_schedule.home_team.players = self.load_team_players(game_schedule.home_team.id)?;
        }
        Ok(game_schedules)
    }

    #[tracing::instrument(skip(self), fields(game_id = %game_id))]
    fn load_game_detail(&self, game_id: u32) -> Result<GameDetail, AppError> {
        info!("load_game_detail() started");
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
                LEFT JOIN 
                    team t_away ON g.away_team_id = t_away.id
                LEFT JOIN 
                    team t_home ON g.home_team_id = t_home.id
                WHERE g.id = ?1";
        let mut game = self
            .db_client
            .query_row::<GameDetail>(query, params![game_id])?;

        game.innings = self.load_innings(game.id)?;
        for inning in &mut game.innings {
            inning.counts = self.load_counts(game.id, inning.seq, inning.tb)?;
        }

        game.away_team.players = self.load_team_players(game.away_team.id)?;
        game.home_team.players = self.load_team_players(game.home_team.id)?;

        game.player_entries = self.load_player_game_entry_views(game.id)?;
        game.player_battings = self.load_player_game_batting_views(game.id)?;
        game.player_runnings = self.load_player_game_running_views(game.id)?;

        Ok(game)
    }

    // CONSTRAINT: Player does not use multiple fielder info in a game.
    #[tracing::instrument(skip(self), fields(team_id = %team_id))]
    fn load_team_players(&self, team_id: u16) -> Result<Vec<Player>, AppError> {
        info!("load_team_players() started");

        let query = "SELECT 
                id,
                first_name,
                last_name,
                age,
                uniform_number
            FROM player_info
            WHERE team_id = ?1";

        let player_infos = self
            .db_client
            .query_rows::<PlayerInfo>(query, params![team_id])?;

        let mut players = Vec::new();

        for player_info in player_infos {
            let mut player = Player::from_player_info(player_info.clone());
            player.offense_skills.running = self.load_running_skills(player_info.id)?;
            player.defense_skills = self.load_defense_skills(player_info.id)?;

            if player.defense_skills.position == Position::P {
                player.defense_skills.pitcher = Some(self.load_pitcher_info(player_info.id)?);
            } else {
                player.offense_skills.batter = Some(self.load_batter_info(player_info.id)?);

                if player.defense_skills.position == Position::C {
                    player.defense_skills.catcher = Some(CatcherInfo::from_fielder_info(
                        self.load_fielder_info(player_info.id, FielderType::Catcher)?,
                    ));
                } else if player.defense_skills.position.is_corner_infielder() {
                    player.defense_skills.corner_infielder =
                        Some(self.load_fielder_info(player_info.id, FielderType::CornerInfielder)?);
                } else if player.defense_skills.position.is_middle_infielder() {
                    player.defense_skills.middle_infielder =
                        Some(self.load_fielder_info(player_info.id, FielderType::MiddleInfielder)?);
                } else if player.defense_skills.position.is_outfielder() {
                    player.defense_skills.outfielder =
                        Some(self.load_fielder_info(player_info.id, FielderType::Outfielder)?);
                }
            }

            players.push(player);
        }
        Ok(players)
    }

    fn load_running_skills(&self, player_id: i64) -> Result<RunningSkills, AppError> {
        info!("load_running_skills() started for {}", player_id);

        let query =
            "SELECT speed, lead_distance, start_reaction FROM running_skills WHERE player_id = ?1";
        self.db_client
            .query_row::<RunningSkills>(query, params![player_id])
    }

    fn load_batter_info(&self, player_id: i64) -> Result<BatterInfo, AppError> {
        info!("load_batter_info() started for {}", player_id);

        let query = "SELECT batting_side, swing_speed, base_launch_angle, consistency_sigma,
                weight_pull, weight_center, weight_opposite, weight_foul_pull, weight_foul_opposite
                FROM batter_info WHERE player_id = ?1";
        self.db_client
            .query_row::<BatterInfo>(query, params![player_id])
    }

    fn load_fielder_info(
        &self,
        player_id: i64,
        fielder_type: FielderType,
    ) -> Result<FielderInfo, AppError> {
        info!("load_fielder_info() started");
        let query =
                "SELECT fielder_type, throw_speed, running_speed, reaction, prep_time FROM fielder_info 
                WHERE player_id = ?1 AND fielder_type = ?2";
        self.db_client
            .query_row::<FielderInfo>(query, params![player_id, fielder_type.as_ref()])
    }

    fn load_pitcher_info(&self, player_id: i64) -> Result<PitcherInfo, AppError> {
        info!("load_pitcher_info() started");
        let query =
                "SELECT throw_side, arm_slot, pitcher_style, velocity, control, stamina, injury_proneness, clutch, hpp, platoon_splitting, delivery_motion_time 
                FROM pitcher_info WHERE player_id = ?1";
        let mut pitcher_info = self
            .db_client
            .query_row::<PitcherInfo>(query, params![player_id])?;

        pitcher_info.pitch_skills = self.load_pitch_skill(player_id)?;
        pitcher_info.fielder_info = self.load_fielder_info(player_id, FielderType::Pitcher)?;
        Ok(pitcher_info)
    }

    fn load_pitch_skill(&self, player_id: i64) -> Result<Vec<PitchSkill>, AppError> {
        info!("load_pitch_skill() started");
        let query =
                "SELECT pitch_type, velocity, control, stamina, injury_proneness, spin_rate, spin_angle, spin_efficiency, usage 
                FROM pitch_skill WHERE player_id = ?1";
        self.db_client
            .query_rows::<PitchSkill>(query, params![player_id])
    }

    #[tracing::instrument(skip(self), fields(player_id = %player_id))]
    fn load_defense_skills(&self, player_id: i64) -> Result<DefenseSkills, AppError> {
        info!("load_defense_skills() started");
        let query = "SELECT position FROM defense_skills WHERE player_id = ?1";
        self.db_client
            .query_row::<DefenseSkills>(query, params![player_id])
    }

    #[tracing::instrument(skip(self), fields(game_id = %game_id))]
    fn load_innings(&self, game_id: u32) -> Result<Vec<Inning>, AppError> {
        info!("load_innings() started");
        let query =
            "SELECT seq, tb FROM inning WHERE game_id = ?1 ORDER BY game_id ASC, seq ASC, tb DESC";
        self.db_client.query_rows::<Inning>(query, params![game_id])
    }

    #[tracing::instrument(skip(self), fields(game_id = %game_id))]
    fn load_counts(
        &self,
        game_id: u32,
        inning_seq: u8,
        inning_tb: TB,
    ) -> Result<Vec<Count>, AppError> {
        info!("load_counts() started");
        let query = "SELECT seq, point, ball, strike, out 
                                FROM count
                                WHERE game_id = ?1 AND inning_seq = ?2 AND inning_tb = ?3";
        self.db_client
            .query_rows::<Count>(query, params![game_id, inning_seq, inning_tb.as_ref()])
    }

    #[tracing::instrument(skip(self), fields(game_id = %game_id))]
    fn load_player_game_entry_views(
        &self,
        game_id: u32,
    ) -> Result<Vec<PlayerGameEntryView>, AppError> {
        info!("load_batting_order_histories() started");
        let query = "SELECT 
                pge.start_count_seq,
                pge.end_count_seq,
                pi.team_id AS team_id,
                pge.position,
                pge.batting_order,
                pge.player_id,
                pi.first_name AS first_name,
                pi.last_name AS last_name,
                pi.age AS age,
                pi.uniform_number AS uniform_number
            FROM player_game_entry pge 
            LEFT JOIN 
                player_info pi ON pge.player_id = pi.id
            WHERE pge.game_id = ?1";
        self.db_client
            .query_rows::<PlayerGameEntryView>(query, params![game_id])
    }

    #[tracing::instrument(skip(self), fields(game_id = %game_id))]
    fn load_player_game_batting_views(
        &self,
        game_id: u32,
    ) -> Result<Vec<PlayerGameBattingView>, AppError> {
        info!("load_batting_result_histories() started");
        let query = "SELECT 
                pgb.count_seq,
                pgb.pitcher_id,
                pi.first_name AS pitcher_first_name,
                pi.last_name AS pitcher_last_name,
                pi.age AS pitcher_age,
                pi.uniform_number AS pitcher_uniform_number,
                pgb.batter_id,
                bi.first_name AS batter_first_name,
                bi.last_name AS batter_last_name,
                bi.age AS batter_age,
                bi.uniform_number AS batter_uniform_number,
                pgb.launch_speed,
                pgb.launch_angle,
                pgb.polar_distance,
                pgb.polar_angle,
                pgb.hang_time,
                pgb.trajectory,
                pgb.fielder_position,
                pgb.result
            FROM player_game_batting pgb
            LEFT JOIN 
                player_info pi ON pgb.pitcher_id = pi.id
            LEFT JOIN 
                player_info bi ON pgb.batter_id = bi.id
            WHERE pgb.game_id = ?1";
        self.db_client
            .query_rows::<PlayerGameBattingView>(query, params![game_id])
    }

    #[tracing::instrument(skip(self), fields(game_id = %game_id))]
    fn load_player_game_running_views(
        &self,
        game_id: u32,
    ) -> Result<Vec<PlayerGameRunningView>, AppError> {
        info!("load_player_game_running_views() started");
        let table_count = self.db_client.query_row::<i64>(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'player_game_running'",
            params![],
        )?;
        if table_count == 0 {
            return Ok(Vec::new());
        }

        let query = "SELECT
                pgr.count_seq,
                pgr.seq,
                pgr.defense_time,
                pgr.runner_time,
                pgr.throw_target_base,
                pgr.play_type,
                pgr.event,
                pgr.ruling,
                pgr.runs_scored,
                pgr.target_runner_id,
                tr.first_name AS target_runner_first_name,
                tr.last_name AS target_runner_last_name,
                tr.age AS target_runner_age,
                tr.uniform_number AS target_runner_uniform_number,
                pgr.runner_1st_id,
                r1.first_name AS runner_1st_first_name,
                r1.last_name AS runner_1st_last_name,
                r1.age AS runner_1st_age,
                r1.uniform_number AS runner_1st_uniform_number,
                pgr.runner_2nd_id,
                r2.first_name AS runner_2nd_first_name,
                r2.last_name AS runner_2nd_last_name,
                r2.age AS runner_2nd_age,
                r2.uniform_number AS runner_2nd_uniform_number,
                pgr.runner_3rd_id,
                r3.first_name AS runner_3rd_first_name,
                r3.last_name AS runner_3rd_last_name,
                r3.age AS runner_3rd_age,
                r3.uniform_number AS runner_3rd_uniform_number
            FROM player_game_running pgr
            LEFT JOIN player_info tr ON pgr.target_runner_id = tr.id
            LEFT JOIN player_info r1 ON pgr.runner_1st_id = r1.id
            LEFT JOIN player_info r2 ON pgr.runner_2nd_id = r2.id
            LEFT JOIN player_info r3 ON pgr.runner_3rd_id = r3.id
            WHERE pgr.game_id = ?1
            ORDER BY pgr.count_seq, pgr.seq";
        self.db_client
            .query_rows::<PlayerGameRunningView>(query, params![game_id])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::game::{BattingResult, GameType, TB};
    use crate::domain::shared::game_stats::PlayerGameEntry;
    use crate::domain::shared::player::{FielderType, Position};
    use crate::repositories::db::{DbClient, SqliteManager};
    use deadpool::managed::Pool;
    use rusqlite::Connection;
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

            CREATE TABLE player_info (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                team_id INTEGER NOT NULL,
                first_name TEXT NOT NULL,
                last_name TEXT NOT NULL,
                age INTEGER NOT NULL,
                uniform_number INTEGER NOT NULL
            );

            CREATE TABLE defense_skills (
                player_id INTEGER PRIMARY KEY,
                position TEXT NOT NULL
            );

            CREATE TABLE running_skills (
                player_id INTEGER PRIMARY KEY,
                speed REAL NOT NULL,
                lead_distance REAL NOT NULL,
                start_reaction REAL NOT NULL
            );

            CREATE TABLE batter_info (
                player_id INTEGER PRIMARY KEY,
                batting_side TEXT NOT NULL,
                swing_speed REAL NOT NULL,
                base_launch_angle REAL NOT NULL,
                consistency_sigma REAL NOT NULL,
                weight_pull REAL NOT NULL,
                weight_center REAL NOT NULL,
                weight_opposite REAL NOT NULL,
                weight_foul_pull REAL NOT NULL,
                weight_foul_opposite REAL NOT NULL
            );

            CREATE TABLE fielder_info (
                player_id INTEGER NOT NULL,
                fielder_type TEXT NOT NULL,
                throw_speed REAL NOT NULL,
                running_speed REAL NOT NULL,
                reaction REAL NOT NULL,
                prep_time REAL NOT NULL,
                PRIMARY KEY (player_id, fielder_type)
            );

            CREATE TABLE pitcher_info (
                player_id INTEGER PRIMARY KEY,
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
                player_id INTEGER NOT NULL,
                pitch_type TEXT NOT NULL,
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

            CREATE TABLE game (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                season INTEGER,
                round_seq INTEGER,
                seq INTEGER,
                planned_date TEXT NOT NULL,
                actual_date TEXT,
                away_team_id INTEGER NOT NULL,
                home_team_id INTEGER NOT NULL,
                stadium_id INTEGER NOT NULL DEFAULT 1,
                game_type TEXT NOT NULL,
                away_points INTEGER,
                home_points INTEGER
            );

            CREATE TABLE stadium (
                id INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                foul_pole_distance REAL NOT NULL,
                center_fence_distance REAL NOT NULL,
                fence_line TEXT,
                fence_height REAL NOT NULL
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
                point INTEGER NOT NULL,
                ball INTEGER NOT NULL,
                strike INTEGER NOT NULL,
                out INTEGER NOT NULL,
                PRIMARY KEY (game_id, inning_seq, inning_tb, seq)
            );

            CREATE TABLE player_game_entry (
                game_id INTEGER,
                start_count_seq INTEGER,
                end_count_seq INTEGER,
                position TEXT,
                batting_order INTEGER,
                player_id INTEGER,
                PRIMARY KEY (
                    game_id,
                    start_count_seq,
                    player_id
                )
            );

            CREATE TABLE player_game_batting (
                game_id INTEGER,
                count_seq INTEGER,
                team_id INTEGER,
                pitcher_id INTEGER,
                batter_id INTEGER,
                launch_speed REAL NOT NULL DEFAULT 0.0,
                launch_angle REAL NOT NULL DEFAULT 0.0,
                polar_distance REAL NOT NULL DEFAULT 0.0,
                polar_angle REAL NOT NULL DEFAULT 0.0,
                hang_time REAL NOT NULL DEFAULT 0.0,
                trajectory TEXT NOT NULL DEFAULT 'Grounder',
                fielder_position TEXT,
                result TEXT NOT NULL,
                PRIMARY KEY (
                    game_id,
                    count_seq,
                    batter_id
                )
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
                "INSERT INTO player_info (
                    id, team_id, first_name, last_name, age, uniform_number
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    team_id,
                    format!("First{id}"),
                    format!("Last{id}"),
                    20 + id,
                    id,
                ],
            )
            .unwrap();
        }
    }

    fn seed_defensive_skills(repo: &SqlGameRepository) {
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
        let conn = conn(repo);
        for id in 1_u32..=18 {
            conn.execute(
                "INSERT INTO defense_skills (player_id, position)
                 VALUES (?1, ?2)",
                params![id, positions[((id - 1) as usize) % positions.len()]],
            )
            .unwrap();
        }
    }

    fn seed_player_skills(repo: &SqlGameRepository) {
        seed_defensive_skills(repo);

        let conn = conn(repo);
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

        for id in 1_i64..=18 {
            conn.execute(
                "INSERT INTO running_skills (
                    player_id, speed, lead_distance, start_reaction
                ) VALUES (?1, 7.5, 2.0, 0.3)",
                params![id],
            )
            .unwrap();

            let position = positions[((id - 1) as usize) % positions.len()];
            if position == Position::P {
                conn.execute(
                    "INSERT INTO pitcher_info (
                        player_id, throw_side, arm_slot, pitcher_style, velocity, control, stamina,
                        injury_proneness, clutch, hpp, platoon_splitting,
                        delivery_motion_time
                    ) VALUES (?1, 'Right', 'ThreeQuarter', 'BalancedPitcher', 145.0, 0.7, 90.0, 0.1, 0.6, 0.5, 0.2, 1.4)",
                    params![id],
                )
                .unwrap();
                conn.execute(
                    "INSERT INTO pitch_skill (
                        player_id, pitch_type, velocity, control, stamina,
                        injury_proneness, spin_rate, spin_angle, spin_efficiency, usage
                    ) VALUES (?1, 'FourSeamFastball', 145.0, 0.7, 90.0, 0.1, 2200.0, 0.0, 0.9, 1.0)",
                    params![id],
                )
                .unwrap();
                seed_fielder_info(&conn, id, FielderType::Pitcher);
            } else {
                conn.execute(
                    "INSERT INTO batter_info (
                        player_id, batting_side, swing_speed, base_launch_angle,
                        consistency_sigma, weight_pull, weight_center, weight_opposite,
                        weight_foul_pull, weight_foul_opposite
                    ) VALUES (?1, 'Right', 30.0, 28.0, 0.03, 0.3, 0.3, 0.2, 0.1, 0.1)",
                    params![id],
                )
                .unwrap();

                if position == Position::C {
                    seed_fielder_info(&conn, id, FielderType::Catcher);
                } else if position.is_corner_infielder() {
                    seed_fielder_info(&conn, id, FielderType::CornerInfielder);
                } else if position.is_middle_infielder() {
                    seed_fielder_info(&conn, id, FielderType::MiddleInfielder);
                } else if position.is_outfielder() {
                    seed_fielder_info(&conn, id, FielderType::Outfielder);
                }
            }
        }
    }

    fn seed_fielder_info(
        conn: &deadpool::managed::Object<SqliteManager>,
        player_id: i64,
        fielder_type: FielderType,
    ) {
        conn.execute(
            "INSERT INTO fielder_info (
                player_id, fielder_type, throw_speed, running_speed, reaction, prep_time
            ) VALUES (?1, ?2, 38.0, 7.0, 0.5, 0.6)",
            params![player_id, fielder_type],
        )
        .unwrap();
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
                    game_id, inning_seq, inning_tb, seq,
                    point, ball, strike, out
                ) VALUES (?1, ?2, ?3, 1, 2, 1, 2, 1)",
                params![game_id, inning_seq, inning_tb],
            )
            .unwrap();
    }

    fn seed_player_game_entry(
        repo: &SqlGameRepository,
        game_id: u32,
        position: Position,
        player_id: u32,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO player_game_entry (
                    game_id, start_count_seq, end_count_seq, position, batting_order, player_id
                ) VALUES (?1, 1, 3, ?2, 0, ?3)",
                params![game_id, position, player_id],
            )
            .unwrap();
    }

    fn seed_player_game_batting(
        repo: &SqlGameRepository,
        game_id: u32,
        _inning_seq: u8,
        _inning_tb: &str,
        count_seq: u8,
        team_id: u16,
        pitcher_id: u32,
        batter_id: u32,
        result: &str,
    ) {
        conn(repo)
            .execute(
                "INSERT INTO player_game_batting (
                    game_id, count_seq, team_id, pitcher_id, batter_id,
                    launch_speed, launch_angle, polar_distance, polar_angle,
                    hang_time, trajectory, fielder_position, result
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                params![
                    game_id,
                    count_seq,
                    team_id,
                    pitcher_id,
                    batter_id,
                    0.0,                  // launch_speed
                    0.0,                  // launch_angle
                    0.0,                  // polar_distance
                    0.0,                  // polar_angle
                    0.0,                  // hang_time
                    "Grounder",           // trajectory
                    Option::<&str>::None, // fielder_position
                    result,
                ],
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
        seed_player_skills(&repo);
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
        assert_eq!(schedules[0].away_team.players[0].info.id, 1);
        assert_eq!(schedules[0].home_team.players[0].info.id, 10);
        assert!(matches!(
            schedules[0].away_team.players[0].defense_skills.position,
            Position::P
        ));
        assert!(matches!(
            schedules[0].home_team.players[0].defense_skills.position,
            Position::P
        ));
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_game_detail_loads_game_innings_and_active_fielder_histories() {
        let (repo, path) = setup_repo();
        seed_teams(&repo);
        seed_players(&repo);
        seed_player_skills(&repo);
        seed_game(&repo, 1, 2026, 1, 1, Some("2026-04-01"));
        seed_inning(&repo, 1, 1, "Top");
        seed_count(&repo, 1, 1, "Top");
        seed_player_game_entry(&repo, 1, Position::P, 1);

        let game = repo.load_game_detail(1).unwrap();

        assert_eq!(game.id, 1);
        assert_eq!(game.actual_date.to_string(), "2026-04-01");
        assert_eq!(game.away_team.id, 1);
        assert_eq!(game.away_team.name.as_ref(), "Away");
        assert_eq!(game.home_team.id, 2);
        assert_eq!(game.home_team.name.as_ref(), "Home");
        assert!(matches!(game.game_type, GameType::Regular));
        assert_eq!(game.away_points, 3);
        assert_eq!(game.home_points, 2);
        assert_eq!(game.innings.len(), 1);
        assert_eq!(game.innings[0].seq, 1);
        assert!(matches!(game.innings[0].tb, TB::Top));
        assert_eq!(game.innings[0].counts.len(), 1);
        assert_eq!(game.player_entries.len(), 1);
        assert_eq!(game.player_entries[0].team_id, 1);
        assert!(matches!(game.player_entries[0].position, Position::P));
        assert_eq!(game.player_entries[0].player.id, 1);
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
    fn load_batting_order_histories_returns_active_fielder_histories_for_game() {
        let (repo, path) = setup_repo();
        seed_teams(&repo);
        seed_players(&repo);
        seed_game(&repo, 1, 2026, 1, 1, Some("2026-04-01"));
        seed_game(&repo, 2, 2026, 1, 2, Some("2026-04-02"));
        seed_player_game_entry(&repo, 1, Position::P, 1);
        seed_player_game_entry(&repo, 2, Position::C, 10);

        let histories = repo.load_player_game_entry_views(1).unwrap();

        assert_eq!(histories.len(), 1);
        let history = &histories[0];
        assert_eq!(history.start_count_seq, 1);
        assert_eq!(history.end_count_seq, 3);
        assert_eq!(history.team_id, 1);
        assert!(matches!(history.position, Position::P));
        assert_eq!(history.player.id, 1);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_batting_result_histories_returns_histories_for_game() {
        let (repo, path) = setup_repo();
        seed_teams(&repo);
        seed_players(&repo);
        seed_game(&repo, 1, 2026, 1, 1, Some("2026-04-01"));
        seed_game(&repo, 2, 2026, 1, 2, Some("2026-04-02"));
        seed_player_game_batting(&repo, 1, 1, "Top", 1, 1, 10, 1, "Double");
        seed_player_game_batting(&repo, 2, 1, "Bottom", 1, 2, 1, 10, "Single");

        let histories = repo.load_player_game_batting_views(1).unwrap();

        assert_eq!(histories.len(), 1);
        let history = &histories[0];
        assert_eq!(history.count_seq, 1);
        assert_eq!(history.pitcher.id, 10);
        assert_eq!(history.batter.id, 1);
        assert!(matches!(history.result, BattingResult::Double));
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
            away_total_point: 4,
            home_total_point: 3,
            innings: vec![Inning {
                seq: 1,
                tb: TB::Top,
                counts: vec![Count {
                    seq: 1,
                    ball: 1,
                    strike: 1,
                    point: 1,
                    out: 0,
                }],
            }],
            player_entries: Vec::new(),
            player_pitchings: Vec::new(),
            player_battings: Vec::new(),
            player_fieldings: Vec::new(),
            player_runnings: Vec::new(),
        };

        repo.update_game_result(&game).unwrap();

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
    fn save_game_result_inserts_active_fielder_histories() {
        let (mut repo, path) = setup_repo();
        seed_teams(&repo);
        seed_game(&repo, 1, 2026, 1, 1, None);
        let ended_history = PlayerGameEntry::new(1, Some(3), Position::P, 0, 1);
        let game = GameResult {
            id: 1,
            actual_date: "2026-04-01".parse().unwrap(),
            away_total_point: 0,
            home_total_point: 0,
            innings: Vec::new(),
            player_entries: vec![
                ended_history,
                PlayerGameEntry::new(1, Some(1), Position::C, 1, 10),
            ],
            player_pitchings: Vec::new(),
            player_battings: Vec::new(),
            player_fieldings: Vec::new(),
            player_runnings: Vec::new(),
        };

        repo.update_game_result(&game).unwrap();

        let conn = conn(&repo);
        let mut stmt = conn
            .prepare(
                "SELECT
                    game_id, start_count_seq, end_count_seq, position, batting_order, player_id
                FROM player_game_entry
                ORDER BY player_id",
            )
            .unwrap();
        let histories = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, u32>(0)?,
                    row.get::<_, u8>(1)?,
                    row.get::<_, u8>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, u8>(4)?,
                    row.get::<_, u32>(5)?,
                ))
            })
            .unwrap()
            .collect::<rusqlite::Result<Vec<_>>>()
            .unwrap();

        assert_eq!(
            histories,
            vec![
                (1, 1, 3, "P".to_string(), 0, 1),
                (1, 1, 1, "C".to_string(), 1, 10),
            ]
        );
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
            ball: 1,
            strike: 1,
            point: 1,
            out: 0,
        };
        let game = GameResult {
            id: 1,
            actual_date: "2026-04-01".parse().unwrap(),
            away_total_point: 4,
            home_total_point: 3,
            innings: vec![Inning {
                seq: 1,
                tb: TB::Top,
                counts: vec![count.clone(), count],
            }],
            player_entries: Vec::new(),
            player_pitchings: Vec::new(),
            player_battings: Vec::new(),
            player_fieldings: Vec::new(),
            player_runnings: Vec::new(),
        };

        assert!(repo.update_game_result(&game).is_err());

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
    fn update_current_round_seq_increments_current_round_seq() {
        let (mut repo, path) = setup_repo();
        seed_game_season(&repo, 2026, 7);

        repo.update_current_round_seq().unwrap();

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
        seed_player_skills(&repo);

        let players = repo.load_team_players(2).unwrap();

        assert_eq!(players.len(), 9);
        assert_eq!(players[0].info.id, 10);
        assert_eq!(players[0].info.first_name, "First10");
        assert_eq!(players[0].info.last_name, "Last10");
        assert_eq!(players[0].info.age, 30);
        std::fs::remove_file(path).ok();
    }

    #[test]
    fn load_defensive_skills_returns_skills_for_player() {
        let (repo, path) = setup_repo();
        seed_teams(&repo);
        seed_players(&repo);
        seed_defensive_skills(&repo);
        let skills = repo.load_defense_skills(1).unwrap();

        assert!(matches!(skills.position, Position::P));
        std::fs::remove_file(path).ok();
    }
}
