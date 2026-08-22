use super::game::{BattingResult, Count, Inning, PitchResult, TB};
use super::player::{
    BatterInfo, CatcherInfo, FielderInfo, PitcherInfo, Position, RL, RunningSkills,
};
use super::team::Lineup;
use crate::domain::random_provider::RandomProvider;
use crate::domain::resolver::batting_resolver::{
    CountStatus, SwingContactType, adapt_to_pitch, calculate_batted_ball, calculate_batting_factor,
    calculate_swing_execution_error, calculate_swing_factor, evaluate_swing_contact,
    select_swing_execution,
};
use crate::domain::resolver::fielding_physics::FielderRiskTolerance;
use crate::domain::resolver::fielding_resolver::{
    DefensePlayResult, PlayContext, PlayType, evaluate_base_stealing, evaluate_defense_play,
    evaluate_double_play, process_fielding,
};
use crate::domain::resolver::pitching_resolver::{
    MatchupContext, calculate_ball_movement, calculate_hanging_pitch_effect,
    calculate_location_bias, calculate_pitch_offset, create_pitch,
};
use crate::domain::resolver::running_resolver::{
    RunnerAdvanceResult, RunnersOnBase, RunnersUnsaved, RunningEvent,
};
use crate::domain::shared::ball::{BattedBall, OutboundResult};
use crate::domain::shared::game::{GameResult, GameSchedule};
use crate::domain::shared::stadium::{Base, Stadium};
use crate::domain::strategy::batting_strategy::SwingExecution;
use crate::domain::util::PolarPosition;
use crate::error::AppError;
use crate::t;
use serde::{Deserialize, Serialize};
use std::fmt;
use strum_macros::{AsRefStr, EnumString};
use tracing::info;

// TODO: Move MAX_INNING to league
pub const MAX_INNING: u8 = 9;
pub const MAX_OUT: u8 = 3;
pub const MAX_BALL: u8 = 4;
pub const MAX_STRIKE: u8 = 3;

// TODO: Move to running strategy
const TEMP_ATTEMPT_STEAL_BASE_WEIGHT: f64 = 0.3;

#[derive(thiserror::Error, Debug)]
pub enum GameError {
    #[error("No players for position: {0}")]
    NoPlayerFor(String),

    #[error("Failed to retrieve BatterInfo")]
    BatterInfo,

    #[error("Failed to retrieve PitcherInfo")]
    PitcherInfo,

    #[error("Failed to retrieve FielderInfo")]
    FielderInfo,

    #[error("Failed to initialize lineup at {0}")]
    Lineup(String),

    #[error("Failed to retrieve current batter")]
    CurrentBatter,

    #[error("Failed to retrieve batter runner")]
    BatterRunner,

    #[error("Failed to retrieve first runner")]
    Runner1st,

    #[error("Failed to retrieve second runner")]
    Runner2nd,

    #[error("Failed to retrieve third runner")]
    Runner3rd,

    #[error("Batter runner target base should not be home base")]
    BatterRunnerTargetBase,

    #[error("Steal target base must be second or third base")]
    StealTargetBase,

    #[error("Double play target base must not be home base")]
    DoublePlayTargetBase,

    #[error("Same target bases are passed")]
    SameTargetBase,

    #[error("Path of from base and to base is not supported")]
    UnsupportedPath,

    #[error("Stadium has no fence intersection")]
    StadiumHasNoFenceIntersection,

    #[error("Too long process time of calculate_trajectory")]
    TimeOut,

    #[error("No fieldes to pick up the batted ball")]
    NoFieldersToPickUp,

    #[error("Failed to create pitch: {0}")]
    PitchCreation(#[from] AppError),
}

pub struct WindCondition {
    pub speed_m_per_s: f64, // NOTE: Wind speed (m/s)
    pub dir_deg: f64, // NOTE: Wind direction (0°: tailwind, 180°: headwind, 90°: crosswind toward first base)
}

#[derive(Debug, Copy, Clone, PartialEq, Eq, Deserialize, Serialize, EnumString, AsRefStr)]
pub enum Ruling {
    Safe,
    Out,
}
impl fmt::Display for Ruling {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Ruling::Safe => write!(f, "{}", t!("safe")),
            Ruling::Out => write!(f, "{}", t!("out")),
        }
    }
}

#[derive(Clone, Debug, Copy, PartialEq, Eq, EnumString, Serialize, Deserialize, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum PitchOutcome {
    InPlay,
    Foul,
    StrikeSwung,
    StrikeLooking,
    Ball,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivePlayer {
    pub id: i64,
    pub batting_order: Option<u8>,
    pub batter: Option<BatterInfo>,
    pub runner: RunningSkills,
    pub fielding_position: Option<Position>,
    pub fielder: Option<FielderInfo>,
    pub polar_position: Option<PolarPosition>,
    pub pitcher: Option<PitcherInfo>,
    pub catcher: Option<CatcherInfo>,
}
impl ActivePlayer {
    pub fn is_batter(&self) -> bool {
        self.batting_order.is_some()
    }

    pub fn is_fielder(&self) -> bool {
        self.fielding_position.is_some()
    }

    pub fn active_batter(&self) -> Option<ActiveBatter> {
        Some(ActiveBatter {
            id: self.id,
            index: self.batting_order?,
            batter: self.batter.clone()?,
            runner: self.runner,
        })
    }

    pub fn active_fielder(&self, risk_tolerance: FielderRiskTolerance) -> Option<ActiveFielder> {
        Some(ActiveFielder {
            position: self.fielding_position?,
            id: self.id,
            info: self.fielder?,
            polar_position: self.polar_position?,
            risk_tolerance,
        })
    }

    pub fn active_pitcher(&self) -> Option<ActivePitcher> {
        Some(ActivePitcher {
            id: self.id,
            pitcher: self.pitcher.clone()?,
        })
    }

    pub fn active_catcher(&self) -> Option<ActiveCatcher> {
        Some(ActiveCatcher {
            id: self.id,
            catcher: self.catcher?,
        })
    }

    pub fn runner(&self) -> ActiveRunner {
        ActiveRunner {
            id: self.id,
            skills: self.runner,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActiveBatter {
    pub id: i64,
    pub index: u8,
    pub batter: BatterInfo,
    pub runner: RunningSkills,
}
impl ActiveBatter {
    pub fn new(player_id: i64, batter: BatterInfo, runner: RunningSkills) -> Self {
        Self {
            id: player_id,
            index: 0,
            batter: batter,
            runner: runner,
        }
    }

    pub fn runner(&self) -> ActiveRunner {
        ActiveRunner {
            id: self.id,
            skills: self.runner,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveRunner {
    pub id: i64,
    pub skills: RunningSkills,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivePitcher {
    pub id: i64,
    pub pitcher: PitcherInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActiveCatcher {
    pub id: i64,
    pub catcher: CatcherInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActiveFielder {
    pub position: Position,
    pub id: i64,
    pub info: FielderInfo,
    pub polar_position: PolarPosition,
    pub risk_tolerance: FielderRiskTolerance,
}
impl ActiveFielder {
    pub fn new(position: Position, player_id: i64, info: FielderInfo) -> Self {
        Self {
            position: position,
            id: player_id,
            info: info,
            polar_position: PolarPosition::default(),
            risk_tolerance: FielderRiskTolerance::Balanced,
        }
    }

    pub fn is(&self, position: Position) -> bool {
        self.position == position
    }

    pub fn distance(&self) -> f64 {
        self.polar_position.distance
    }

    pub fn angle(&self) -> f64 {
        self.polar_position.angle
    }

    pub fn x(&self) -> f64 {
        self.polar_position.x
    }

    pub fn y(&self) -> f64 {
        self.polar_position.y
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum GameProgress {
    Ongoing,
    WalkOff,
    GameSet,
    Postponed,
}

#[derive(Debug)]
pub struct GameState {
    rng: Box<dyn RandomProvider>,
    pub inning_seq: u8,
    pub inning_tb: TB,
    pub count_seq: u16,
    pub away_total_point: u8,
    pub home_total_point: u8,
    pub away_lineup: Lineup,
    pub home_lineup: Lineup,
    pub inning_state: InningState,
    pub inning: Inning,
    pub game_result: GameResult,
    pub stadium: Stadium,
}
impl GameState {
    pub fn new(
        mut rng: Box<dyn RandomProvider>,
        mut game_schedule: GameSchedule,
    ) -> Result<GameState, GameError> {
        let away_lineup = game_schedule.away_team.lineup(rng.as_mut())?;
        let home_lineup = game_schedule.home_team.lineup(rng.as_mut())?;

        Ok(GameState {
            rng: rng,
            inning_seq: 0, // CONSTRAINT: Initialization: 1 should be set at the beginning of the game
            inning_tb: TB::Bottom, // CONSTRAINT: Initialization: Top should be set at the beginning of the game
            count_seq: 1,
            away_total_point: 0,
            home_total_point: 0,
            inning_state: InningState::new(),
            inning: Inning::new(),
            game_result: GameResult::new(
                game_schedule.id,
                game_schedule.planned_date,
                &away_lineup.players,
                &home_lineup.players,
            ),
            away_lineup: away_lineup,
            home_lineup: home_lineup,
            stadium: game_schedule.stadium,
        })
    }

    fn is_top(&self) -> bool {
        self.inning_tb == TB::Top
    }

    fn is_bottom(&self) -> bool {
        self.inning_tb == TB::Bottom
    }

    pub fn progress(&self) -> GameProgress {
        if self.is_walk_off_condition() {
            return GameProgress::WalkOff;
        }

        if self.is_game_set_condition() {
            return GameProgress::GameSet;
        }

        if self.is_postponed() {
            return GameProgress::Postponed;
        }

        GameProgress::Ongoing
    }

    fn is_walk_off_condition(&self) -> bool {
        self.inning_seq >= MAX_INNING
            && self.is_bottom()
            && self.home_total_point > self.away_total_point
    }

    fn is_game_set_condition(&self) -> bool {
        self.inning_seq >= MAX_INNING
            && ((self.is_top() && self.away_total_point < self.home_total_point)
                || self.is_bottom())
    }

    fn is_postponed(&self) -> bool {
        // TODO: Check Rainout
        false
    }

    pub fn advance_half_inning(&mut self) {
        self.inning_state = InningState::new();

        self.inning_tb = match self.inning_tb {
            TB::Top => TB::Bottom,
            TB::Bottom => {
                self.inning_seq += 1;
                TB::Top
            }
        };

        self.inning = Inning {
            seq: self.inning_seq,
            tb: self.inning_tb,
            counts: Vec::new(),
        };
    }

    pub fn finish_half_inning(&mut self) {
        self.game_result.innings.push(self.inning.clone());
    }

    pub fn current_pitcher(&self) -> ActivePitcher {
        if self.inning_tb == TB::Top {
            self.home_lineup.pitcher()
        } else {
            self.away_lineup.pitcher()
        }
    }

    pub fn current_catcher(&self) -> ActiveCatcher {
        if self.inning_tb == TB::Top {
            self.home_lineup.catcher()
        } else {
            self.away_lineup.catcher()
        }
    }

    pub fn current_batter(&mut self) -> Result<ActiveBatter, GameError> {
        if self.inning_tb == TB::Top {
            self.away_lineup.next()
        } else {
            self.home_lineup.next()
        }
    }

    pub fn current_fielders(&self) -> Vec<ActiveFielder> {
        if self.inning_tb == TB::Top {
            self.home_lineup.fielders()
        } else {
            self.away_lineup.fielders()
        }
    }

    pub fn change_risk_tolerance(&mut self, risk_tolerance: FielderRiskTolerance) {
        if self.inning_tb == TB::Top {
            self.home_lineup.change_risk_tolerance(risk_tolerance);
        } else {
            self.away_lineup.change_risk_tolerance(risk_tolerance);
        }
    }

    fn prepare_plate_appearance(&mut self, batter_runner: ActiveRunner) {
        self.inning_state.runners.batter_runner = Some(batter_runner);
    }

    fn active_batter_for_count(&mut self) -> Result<ActiveBatter, GameError> {
        if let Some(active_batter) = &self.inning_state.active_batter {
            return Ok(active_batter.clone());
        }

        let active_batter = self.current_batter()?;
        self.inning_state.active_batter = Some(active_batter.clone());

        Ok(active_batter)
    }

    fn finish_plate_appearance(&mut self) {
        self.inning_state.active_batter = None;
        self.inning_state.runners.batter_runner = None;
    }

    fn resolve_foul(&mut self, pitcher_id: i64, batter_id: i64, ball: &BattedBall) {
        info!("Foul");

        self.game_result.add_player_batting(
            self.count_seq,
            pitcher_id,
            batter_id,
            *ball,
            None,
            BattingResult::Foul,
        );
        self.add_count(0);
    }

    fn resolve_homerun(&mut self, pitcher_id: i64, batter_id: i64, ball: &BattedBall) {
        info!("Homerun");

        let point = self.inning_state.runners.after_homerun();
        self.game_result.add_player_batting(
            self.count_seq,
            pitcher_id,
            batter_id,
            *ball,
            None,
            BattingResult::HomeRun,
        );
        self.add_count(point);
        self.finish_plate_appearance();
    }

    fn resolve_ground_rule_double(&mut self, pitcher_id: i64, batter_id: i64, ball: &BattedBall) {
        info!("Ground Rule Double");

        let point = self.inning_state.runners.after_ground_rule_double();
        self.game_result.add_player_batting(
            self.count_seq,
            pitcher_id,
            batter_id,
            *ball,
            None,
            BattingResult::Double,
        );
        self.add_count(point);
        self.finish_plate_appearance();
    }

    fn resolve_wild_pitch(&mut self) -> u8 {
        if self.inning_state.ball < MAX_BALL {
            self.inning_state.ball += 1;
        }

        let walk = self.inning_state.ball >= MAX_BALL;
        let point = self.inning_state.runners.after_wild_pitch(walk);

        if walk {
            self.inning_state.ball = 0;
            self.inning_state.strike = 0;
            self.finish_plate_appearance();
        }

        point
    }

    fn resolve_walk(&mut self) -> u8 {
        let point = self.inning_state.runners.after_walk();

        self.inning_state.ball = 0;
        self.inning_state.strike = 0;
        self.finish_plate_appearance();

        point
    }

    fn resolve_fly_catch(
        &mut self,
        pitcher_id: i64,
        batter_id: i64,
        ctx: &PlayContext,
    ) -> Result<(), GameError> {
        self.inning_state.add_out();

        let defense_play_result = evaluate_defense_play(&ctx, self.rng.as_mut())?;
        info!("Defense Play Result: {:#?}", defense_play_result);

        let mut point = 0;

        if ctx.fielded_ball.fielded_by.is_outfielder() && self.inning_state.allows_tagup() {
            info!("Fly catch out, Tag-up.");

            let runner_advance_result = self
                .inning_state
                .runners
                .after_tagup(&defense_play_result)?;

            self.game_result.add_player_running(
                self.count_seq,
                1,
                RunningEvent::TagUp,
                &runner_advance_result,
            );

            if runner_advance_result.ruling == Ruling::Out {
                self.inning_state.add_out();
            } else {
                point += runner_advance_result.runs_scored;
            }
        } else {
            info!("Fly catch out, No tag-up.");
        }

        self.game_result.add_player_fielding_from_fly_catch(
            self.count_seq,
            ctx.try_catch_fielder.id,
            ctx.try_catch_fielder.position,
            ctx.fielded_ball.time_to_field,
            PlayType::CatchPlay,
        );

        self.game_result.add_player_batting(
            self.count_seq,
            pitcher_id,
            batter_id,
            ctx.fielded_ball.ball,
            Some(ctx.fielded_ball.fielded_by),
            BattingResult::Out,
        );

        self.add_count(point);
        self.finish_plate_appearance();

        Ok(())
    }

    // TODO: stolen base should cover hit-and-run case
    fn try_steal_base(&mut self) -> Result<Ruling, GameError> {
        let pitcher = self.current_pitcher().pitcher.clone();
        let catcher = self.current_catcher().catcher;
        let steal_defense_play_result =
            evaluate_base_stealing(Base::Second, &pitcher, &catcher, self.rng.as_mut());

        info!(
            "Steal Defense Play Result: {:#?}",
            steal_defense_play_result
        );

        let steal_runner_advance_result = self
            .inning_state
            .runners
            .after_base_stealing(steal_defense_play_result)?;

        info!(
            "Steal Runner Advance Result: {:#?}",
            steal_runner_advance_result
        );

        self.game_result.add_player_running_from_stolen_base(
            self.count_seq,
            self.inning_state.runners.current_runners(),
            &steal_runner_advance_result,
        );

        let ruling = steal_runner_advance_result.ruling;
        if ruling == Ruling::Out {
            self.inning_state.add_out();
            self.add_count(0);
        };

        Ok(ruling)
    }

    fn resolve_ball_in_play(
        rng: &mut dyn RandomProvider,
        runners: &RunnersOnBase,
        ctx: &PlayContext,
        batting_side: RL,
    ) -> Result<(DefensePlayResult, RunnerAdvanceResult), GameError> {
        let defense_play_result = evaluate_defense_play(&ctx, rng)?;
        info!("Defense Play Result: {:#?}", defense_play_result);

        let runner_advance_result = if ctx.fielded_ball.fielded_by.is_outfielder() {
            info!("Running : Outfield Hit.");

            runners.after_outfield_hit(&defense_play_result, batting_side)?
        } else {
            info!("Running : Infield Grounder.");

            runners.after_infield_grounder(&defense_play_result, batting_side)?
        };

        info!("Runner Advance Result: {:#?}", runner_advance_result);

        Ok((defense_play_result, runner_advance_result))
    }

    fn try_double_play(
        &mut self,
        pitcher_id: i64,
        batter_id: i64,
        ctx: &PlayContext,
        running_seq: u8,
        defense_play_result: &DefensePlayResult,
        runner_advance_result: &RunnerAdvanceResult,
        batting_side: RL,
    ) -> Result<bool, GameError> {
        let double_play_defense_play_result =
            evaluate_double_play(&ctx, &defense_play_result, self.rng.as_mut())?;

        info!(
            "Double Play Defense Play Result: {:#?}",
            double_play_defense_play_result
        );

        if let Some(double_play_defense_play_result) = double_play_defense_play_result {
            self.game_result.add_player_fielding_from_double_play(
                self.count_seq,
                &double_play_defense_play_result,
            );

            info!("{:#?}", double_play_defense_play_result);

            let double_play_runner_advance_result = self.inning_state.runners.after_double_play(
                &double_play_defense_play_result,
                &runner_advance_result,
                batting_side,
            )?;

            info!("{:#?}", double_play_runner_advance_result);

            self.game_result.add_player_running_from_double_play(
                self.count_seq,
                running_seq,
                &double_play_runner_advance_result,
            );

            self.commit_count_result(
                pitcher_id,
                batter_id,
                &ctx,
                double_play_runner_advance_result.batting_result,
                runner_advance_result.runs_scored,
                double_play_runner_advance_result.unsaved_runners,
            )?;

            if double_play_runner_advance_result.ruling == Ruling::Out {
                self.inning_state.add_out();
            };

            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn commit_count_result(
        &mut self,
        pitcher_id: i64,
        batter_id: i64,
        ctx: &PlayContext,
        batting_result: BattingResult,
        point: u8,
        unsaved_runners: RunnersUnsaved,
    ) -> Result<(), GameError> {
        self.inning_state
            .runners
            .commit_unsaved_runners(unsaved_runners);

        self.game_result.add_player_batting(
            self.count_seq,
            pitcher_id,
            batter_id,
            ctx.fielded_ball.ball,
            Some(ctx.fielded_ball.fielded_by),
            batting_result,
        );

        self.add_count(point);
        self.finish_plate_appearance();

        Ok(())
    }

    pub fn process_count(&mut self) -> Result<(), GameError> {
        info!("new count started");

        let mut running_seq = 1;
        let mut point = 0;

        // TODO: stadium should move to Game.
        let stadium = Stadium::new(1, "AAA".to_string(), 98.0, 120.0, 2.0);

        let active_pitcher = self.current_pitcher();
        let pitcher_id = active_pitcher.id;
        let pitcher = active_pitcher.pitcher.clone();
        let active_batter = self.active_batter_for_count()?;
        let batter_id = active_batter.id;
        let batter = active_batter.batter.clone();
        let batting_side = active_batter.batter.batting_side;
        let batter_runner = active_batter.runner();

        self.prepare_plate_appearance(batter_runner);

        let hanging_pitch_effect = calculate_hanging_pitch_effect(self.rng.as_mut(), &pitcher);

        let pitched_ball = create_pitch(self.rng.as_mut(), &pitcher, hanging_pitch_effect)?;
        // TODO: expected_pitched_ball should be created by better logic.
        let expected_ball = create_pitch(self.rng.as_mut(), &pitcher, hanging_pitch_effect)?;

        let absolute_location = calculate_ball_movement(&pitched_ball);

        let matchup = MatchupContext {
            throw_side: pitcher.throw_side,
            batting_side: batter.batting_side,
        };

        let location_bias = calculate_location_bias(pitched_ball.actual_location);

        let pitch_displacement = calculate_pitch_offset(
            self.rng.as_mut(),
            &pitched_ball,
            &expected_ball,
            &matchup,
            &location_bias,
            batter.batting_eye,
        );

        let batting_factor = calculate_batting_factor(
            &pitcher,
            &batter,
            pitched_ball.pitch_type,
            expected_ball.pitch_type,
            &pitched_ball.actual_location,
            &expected_ball.actual_location,
        );

        let swing_factor = calculate_swing_factor(
            batter.sample_plate_approach(self.rng.as_mut())?,
            self.count_status(),
            pitched_ball.pitch_type,
            &batting_factor,
        );

        let swing_execution = select_swing_execution(self.rng.as_mut(), swing_factor);

        if swing_execution == SwingExecution::Take {
            info!("Take");

            let pitch_result = pitched_ball.actual_location.call(batter.batting_side);

            let batting_result = match pitch_result {
                PitchResult::Ball if self.inning_state.ball + 1 >= MAX_BALL => BattingResult::Walk,
                PitchResult::WildPitch if self.inning_state.ball + 1 >= MAX_BALL => {
                    BattingResult::Walk
                }
                PitchResult::Strike if self.inning_state.strike + 1 >= MAX_STRIKE => {
                    BattingResult::Strikeout
                }
                PitchResult::HitByPitch => BattingResult::HitByPitch,
                PitchResult::WildPitch => BattingResult::WildPitch,
                _ => BattingResult::Take,
            };

            self.game_result.add_player_batting(
                self.count_seq,
                pitcher_id,
                batter_id,
                BattedBall::default(),
                None,
                batting_result,
            );

            let point = match pitch_result {
                PitchResult::HitByPitch => self.resolve_walk(),
                PitchResult::WildPitch => self.resolve_wild_pitch(),
                PitchResult::Ball => self.add_ball(),
                PitchResult::Strike => {
                    self.add_strike();
                    0
                }
            };
            self.add_count(point);
        } else {
            let displacement =
                adapt_to_pitch(&pitch_displacement, batter.bat_control, &batting_factor);

            let swing_error = calculate_swing_execution_error(
                self.rng.as_mut(),
                &batter,
                &pitched_ball.actual_location,
            );

            let swing_contact = evaluate_swing_contact(&batter, &displacement, &swing_error);

            if swing_contact.contact_type == SwingContactType::SwungAndMiss {
                info!("SwungAndMiss");

                let batting_result = if self.inning_state.strike + 1 >= MAX_STRIKE {
                    BattingResult::Strikeout
                } else {
                    BattingResult::StrikeSwung
                };

                self.game_result.add_player_batting(
                    self.count_seq,
                    pitcher_id,
                    batter_id,
                    BattedBall::default(),
                    None,
                    batting_result,
                );
                self.add_strike();
                self.add_count(0);
            } else {
                let batted_ball =
                    calculate_batted_ball(&batter, pitched_ball, &swing_contact, &stadium).unwrap();

                info!("Batted Ball: {:#?}", batted_ball);

                match batted_ball.outbound_result {
                    OutboundResult::Foul => {
                        self.resolve_foul(pitcher_id, batter_id, &batted_ball);
                        return Ok(());
                    }
                    OutboundResult::HomeRun => {
                        self.resolve_homerun(pitcher_id, batter_id, &batted_ball);
                        return Ok(());
                    }
                    OutboundResult::GroundRuleDouble => {
                        self.resolve_ground_rule_double(pitcher_id, batter_id, &batted_ball);
                        return Ok(());
                    }
                    OutboundResult::InField => {}
                }

                let fielders = self.current_fielders();
                let field_play_result =
                    process_fielding(self.rng.as_mut(), &fielders, &batted_ball)?;
                let fielder = field_play_result.result().fielder;
                let fielded_ball = field_play_result.result().ball();

                info!("Fielded Ball: {:#?}", fielded_ball);

                let ctx = PlayContext {
                    runners: &self.inning_state.runners.clone(),
                    fielders: &fielders,
                    try_catch_fielder: fielder,
                    fielded_ball: &fielded_ball,
                };

                if fielded_ball.is_fly_catch {
                    self.resolve_fly_catch(pitcher_id, batter_id, &ctx)?;
                    return Ok(());
                }

                if batted_ball.is_foul() {
                    info!("Foul");

                    self.game_result.add_player_batting(
                        self.count_seq,
                        pitcher_id,
                        batter_id,
                        batted_ball,
                        None,
                        BattingResult::Foul,
                    );

                    if self.inning_state.strike < 2 {
                        self.add_strike();
                    };

                    self.add_count(0);

                    return Ok(());
                }

                let mut runners_after_steal = None;

                if self.inning_state.can_steal_base(&mut *self.rng) {
                    let steal_result = self.try_steal_base()?;

                    running_seq += 1;

                    if steal_result == Ruling::Out {
                        return Ok(());
                    }

                    runners_after_steal = Some(self.inning_state.runners.clone());
                }

                // NOTE: This logic is needed to update runners after stolen base case.
                let ctx = if let Some(runners) = &runners_after_steal {
                    PlayContext {
                        runners,
                        fielders: &fielders,
                        try_catch_fielder: fielder,
                        fielded_ball: &fielded_ball,
                    }
                } else {
                    ctx
                };

                let (defense_play_result, runner_advance_result) = Self::resolve_ball_in_play(
                    self.rng.as_mut(),
                    &self.inning_state.runners,
                    &ctx,
                    batting_side,
                )?;
                point += runner_advance_result.runs_scored;

                self.game_result
                    .add_player_fielding(self.count_seq, &defense_play_result);
                self.game_result.add_player_running(
                    self.count_seq,
                    running_seq,
                    RunningEvent::GrounderPlay,
                    &runner_advance_result,
                );
                running_seq += 1;

                if runner_advance_result.ruling == Ruling::Out {
                    self.inning_state.add_out();

                    if self.inning_state.inning_progress() == InningProgress::Over {
                        self.commit_count_result(
                            pitcher_id,
                            batter_id,
                            &ctx,
                            runner_advance_result.batting_result,
                            point,
                            runner_advance_result.unsaved_runners,
                        )?;

                        return Ok(());
                    }
                };

                info!("Runner Advance Result:{:#?}", runner_advance_result);

                if ctx.fielded_ball.fielded_by.is_infielder() && self.inning_state.can_double_play()
                {
                    let is_double_play_occured = self.try_double_play(
                        pitcher_id,
                        batter_id,
                        &ctx,
                        running_seq,
                        &defense_play_result,
                        &runner_advance_result,
                        batting_side,
                    )?;

                    if is_double_play_occured {
                        return Ok(());
                    }
                };

                self.commit_count_result(
                    pitcher_id,
                    batter_id,
                    &ctx,
                    runner_advance_result.batting_result,
                    point,
                    runner_advance_result.unsaved_runners,
                )?;
            };
        };

        Ok(())
    }

    fn count_status(&self) -> CountStatus {
        match (self.inning_state.ball, self.inning_state.strike) {
            (0, 0) => CountStatus::C00,
            (1, 0) => CountStatus::C10,
            (2, 0) => CountStatus::C20,
            (3, 0) => CountStatus::C30,
            (0, 1) => CountStatus::C01,
            (1, 1) => CountStatus::C11,
            (2, 1) => CountStatus::C21,
            (3, 1) => CountStatus::C31,
            (0, 2) => CountStatus::C02,
            (1, 2) => CountStatus::C12,
            (2, 2) => CountStatus::C22,
            (3, 2) => CountStatus::C32,
            _ => panic!(
                "invalid count: {} balls, {} strikes",
                self.inning_state.ball, self.inning_state.strike
            ),
        }
    }

    fn add_ball(&mut self) -> u8 {
        if self.inning_state.ball < MAX_BALL {
            self.inning_state.ball += 1;
        }

        if self.inning_state.ball < MAX_BALL {
            return 0;
        }

        self.resolve_walk()
    }

    fn add_strike(&mut self) {
        if self.inning_state.strike < MAX_STRIKE {
            self.inning_state.strike += 1;
        }

        if self.inning_state.strike >= MAX_STRIKE {
            self.inning_state.add_out();
            self.inning_state.ball = 0;
            self.inning_state.strike = 0;
            self.finish_plate_appearance();
        }
    }

    fn add_count(&mut self, point: u8) {
        match self.inning_tb {
            TB::Top => self.away_total_point += point,
            TB::Bottom => self.home_total_point += point,
        };

        self.inning.add_count(Count {
            seq: self.count_seq,
            ball: self.inning_state.ball,
            strike: self.inning_state.strike,
            out: self.inning_state.out,
            point: point,
        });

        self.count_seq += 1;
    }

    pub fn finish_game(&mut self) {
        self.game_result.away_total_point = self.away_total_point;
        self.game_result.home_total_point = self.home_total_point;
        self.game_result
            .update_player_game_entry_at_game_end(self.count_seq);
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InningProgress {
    Ongoing,
    Over,
}

#[derive(Debug)]
pub struct InningState {
    pub runners: RunnersOnBase,
    pub ball: u8,
    pub strike: u8,
    pub out: u8,
    pub active_batter: Option<ActiveBatter>,
}
impl InningState {
    pub fn new() -> InningState {
        InningState {
            runners: RunnersOnBase::default(),
            ball: 0,
            strike: 0,
            out: 0,
            active_batter: None,
        }
    }

    pub fn inning_progress(&self) -> InningProgress {
        if self.out >= MAX_OUT {
            return InningProgress::Over;
        }

        InningProgress::Ongoing
    }

    pub fn allows_tagup(&self) -> bool {
        self.out <= 2
            && (self.runners.has_runner_on(Base::Third) || self.runners.has_runner_on(Base::Second))
    }

    pub fn can_double_play(&self) -> bool {
        self.out < 2 && self.runners.has_runner_on(Base::First)
    }

    // TODO: Consider running attitude (early start, hit and run, etc)
    pub fn can_steal_base(&self, rng: &mut dyn RandomProvider) -> bool {
        if self.runners.has_runner_on(Base::First) {
            if rng.random() < TEMP_ATTEMPT_STEAL_BASE_WEIGHT {
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    pub fn add_out(&mut self) {
        self.out += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::random_provider::FixedRng;
    use crate::domain::shared::game::GameType;
    use crate::domain::shared::player::{
        ArmSlot, DefenseSkills, FielderType, PitchSkill, PitchType, PitcherStyle, Player,
        PlayerInfo, RL,
    };
    use crate::domain::shared::team::Team;
    use chrono::NaiveDate;

    fn fielder_info(fielder_type: FielderType) -> FielderInfo {
        FielderInfo {
            fielder_type,
            throw_speed: 35.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.6,
            catching: 0.8,
            reach_height: 2.5,
            reach_range: 0.0,
        }
    }

    fn player(id: i64, position: Position, batting_order: Option<u8>) -> Player {
        let mut player = Player::from_player_info(PlayerInfo::new(
            id,
            format!("First{id}"),
            format!("Last{id}"),
            25,
            id as u8,
        ));
        player.offense_skills.running = RunningSkills {
            speed: 7.7,
            lead_distance: 4.0,
            start_reaction: 0.1,
        };
        player.offense_skills.batter = batting_order.map(|order| BatterInfo {
            batting_side: RL::Right,
            batter_type: crate::domain::shared::player::BatterType::ClassicAnalyst,
            zone_aptitude: crate::domain::shared::player::ZoneAptitude::Balanced,
            hot_zone_scale: 0.1,
            batting_eye: 0.5,
            swing_speed: 100.0 - order as f64,
            swing_power: 1.0,
            attack_angle: 28.0,
            bat_control: 0.8,
            consistency: 0.03,
        });

        player.defense_skills = DefenseSkills::new(position);
        match position {
            Position::P => {
                let info = fielder_info(FielderType::Pitcher);
                player.defense_skills.pitcher = Some(PitcherInfo {
                    height: 1.85,
                    extension: 1.8,
                    throw_side: RL::Right,
                    arm_slot: ArmSlot::ThreeQuarter,
                    pitcher_style: PitcherStyle::BalancedPitcher,
                    velocity: 145.0,
                    spin_rate: 2200.0,
                    control: 10.0,
                    stamina: 0.5,
                    injury_proneness: 0.5,
                    clutch: 0.5,
                    hpp: 0.5,
                    platoon_splitting: 0.5,
                    delivery_motion_time: 1.4,
                    consistency: 0.03,
                    pitch_skills: vec![PitchSkill {
                        pitch_type: PitchType::FourSeamFastball,
                        velocity: 1.0,
                        control: 10.0,
                        stamina: 0.5,
                        injury_proneness: 0.5,
                        spin_rate: 1.0,
                        spin_angle: 0.0,
                        spin_efficiency: 0.9,
                        usage: 1.0,
                    }],
                    fielder_info: info,
                });
            }
            Position::C => {
                let info = fielder_info(FielderType::Catcher);
                player.defense_skills.catcher = Some(CatcherInfo { fielder_info: info });
            }
            Position::FB | Position::TB => {
                player.defense_skills.corner_infielder =
                    Some(fielder_info(FielderType::CornerInfielder));
            }
            Position::SB | Position::SS => {
                player.defense_skills.middle_infielder =
                    Some(fielder_info(FielderType::MiddleInfielder));
            }
            Position::LF | Position::CF | Position::RF => {
                player.defense_skills.outfielder = Some(fielder_info(FielderType::Outfielder));
            }
            Position::DH => {}
        }

        player
    }

    fn team(id: u16, name: &str, first_player_id: i64) -> Team {
        let batter_positions = [
            Position::C,
            Position::FB,
            Position::SB,
            Position::TB,
            Position::SS,
            Position::LF,
            Position::CF,
            Position::RF,
            Position::DH,
        ];
        let mut players = vec![player(first_player_id, Position::P, None)];
        players.extend(
            batter_positions
                .into_iter()
                .enumerate()
                .map(|(index, position)| {
                    player(
                        first_player_id + index as i64 + 1,
                        position,
                        Some((index + 1) as u8),
                    )
                }),
        );

        Team {
            id,
            name: name.into(),
            players,
        }
    }

    fn game_state() -> GameState {
        GameState::new(
            Box::new(FixedRng::new(0.5)),
            GameSchedule {
                id: 1,
                season: 2026,
                round_seq: 1,
                seq: 4,
                planned_date: NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
                away_team: team(1, "AAA", 1),
                home_team: team(2, "BBB", 101),
                stadium: Stadium::default(),
                game_type: GameType::Regular,
            },
        )
        .unwrap()
    }

    fn assert_no_runners(inning_state: &InningState) {
        assert!(inning_state.runners.batter_runner.is_none());
        assert!(inning_state.active_batter.is_none());
        assert!(!inning_state.runners.has_runner_on(Base::First));
        assert!(!inning_state.runners.has_runner_on(Base::Second));
        assert!(!inning_state.runners.has_runner_on(Base::Third));
    }

    fn runner(id: i64) -> ActiveRunner {
        ActiveRunner {
            id,
            skills: RunningSkills {
                speed: 7.7,
                lead_distance: 4.0,
                start_reaction: 0.1,
            },
        }
    }

    #[test]
    fn new_initializes_game_before_first_inning() {
        let game = game_state();

        assert_eq!(game.inning_seq, 0);
        assert_eq!(game.inning_tb, TB::Bottom);
        assert_eq!(game.away_total_point, 0);
        assert_eq!(game.home_total_point, 0);
        assert_eq!(game.away_lineup.players.len(), 10);
        assert_eq!(game.home_lineup.players.len(), 10);
        assert_eq!(game.away_lineup.batters().len(), 9);
        assert_eq!(game.home_lineup.batters().len(), 9);
        assert_eq!(game.away_lineup.fielders().len(), 9);
        assert_eq!(game.home_lineup.fielders().len(), 9);
        assert_eq!(game.away_lineup.current_index, 0);
        assert_eq!(game.home_lineup.current_index, 0);
    }

    #[test]
    fn advance_half_inning_cycles_top_and_bottom() {
        let mut game = game_state();

        game.advance_half_inning();
        assert_eq!(game.inning.seq, 1);
        assert_eq!(game.inning.tb, TB::Top);
        assert!(game.inning.counts.is_empty());
        assert_no_runners(&game.inning_state);
        assert_eq!(game.inning_state.out, 0);

        game.inning_state.add_out();
        game.advance_half_inning();
        assert_eq!(game.inning.seq, 1);
        assert_eq!(game.inning.tb, TB::Bottom);
        assert_no_runners(&game.inning_state);
        assert_eq!(game.inning_state.out, 0);

        game.advance_half_inning();
        assert_eq!(game.inning.seq, 2);
        assert_eq!(game.inning.tb, TB::Top);
    }

    #[test]
    fn process_count_records_count_for_current_batting_team() {
        let mut game = game_state();

        game.advance_half_inning();
        let away_batter_id = game.away_lineup.batters()[0].id;
        game.process_count().unwrap();

        assert_eq!(game.count_seq, 2);
        assert_eq!(game.inning.counts.len(), 1);
        assert_eq!(game.inning.counts[0].seq, 1);
        assert_eq!(game.away_lineup.current_index, 1);
        assert_eq!(game.home_lineup.current_index, 0);
        assert_eq!(
            game.game_result.player_battings.last().unwrap().batter_id,
            away_batter_id
        );
        if let Some(active_batter) = &game.inning_state.active_batter {
            assert_eq!(active_batter.id, away_batter_id);
        }

        game.advance_half_inning();
        let home_batter_id = game.home_lineup.batters()[0].id;
        game.process_count().unwrap();

        assert_eq!(game.count_seq, 3);
        assert_eq!(game.inning.counts.len(), 1);
        assert_eq!(game.inning.counts[0].seq, 2);
        assert_eq!(game.away_lineup.current_index, 1);
        assert_eq!(game.home_lineup.current_index, 1);
        assert_eq!(
            game.game_result.player_battings.last().unwrap().batter_id,
            home_batter_id
        );
        if let Some(active_batter) = &game.inning_state.active_batter {
            assert_eq!(active_batter.id, home_batter_id);
        }
    }

    #[test]
    fn current_batter_uses_correct_lineup_and_wraps_after_nine_batters() {
        let mut game = game_state();

        game.advance_half_inning();
        for expected_id in 2..=10 {
            assert_eq!(game.current_batter().unwrap().id, expected_id as i64);
        }
        assert_eq!(game.current_batter().unwrap().id, 2);

        game.advance_half_inning();
        assert_eq!(game.current_batter().unwrap().id, 102);
    }

    #[test]
    fn active_batter_for_count_reuses_batter_until_plate_appearance_finishes() {
        let mut game = game_state();

        game.advance_half_inning();

        let first_batter = game.active_batter_for_count().unwrap();
        assert_eq!(first_batter.id, 2);
        assert_eq!(game.away_lineup.current_index, 1);

        let same_batter = game.active_batter_for_count().unwrap();
        assert_eq!(same_batter.id, 2);
        assert_eq!(game.away_lineup.current_index, 1);

        game.finish_plate_appearance();

        let next_batter = game.active_batter_for_count().unwrap();
        assert_eq!(next_batter.id, 3);
        assert_eq!(game.away_lineup.current_index, 2);
    }

    #[test]
    fn add_strike_increments_strike_without_finishing_plate_appearance() {
        let mut game = game_state();
        game.advance_half_inning();
        let batter = game.active_batter_for_count().unwrap();
        game.prepare_plate_appearance(batter.runner());
        game.inning_state.ball = 2;

        game.add_strike();

        assert_eq!(game.inning_state.ball, 2);
        assert_eq!(game.inning_state.strike, 1);
        assert_eq!(game.inning_state.out, 0);
        assert_eq!(game.inning_state.active_batter.unwrap().id, batter.id);
        assert!(game.inning_state.runners.batter_runner.is_some());
    }

    #[test]
    fn add_strike_records_out_and_resets_plate_appearance_on_third_strike() {
        let mut game = game_state();
        game.advance_half_inning();
        let batter = game.active_batter_for_count().unwrap();
        game.prepare_plate_appearance(batter.runner());
        game.inning_state.ball = 3;
        game.inning_state.strike = MAX_STRIKE - 1;

        game.add_strike();

        assert_eq!(game.inning_state.ball, 0);
        assert_eq!(game.inning_state.strike, 0);
        assert_eq!(game.inning_state.out, 1);
        assert_no_runners(&game.inning_state);
    }

    #[test]
    fn add_ball_increments_ball_without_finishing_plate_appearance() {
        let mut game = game_state();
        game.advance_half_inning();
        let batter = game.active_batter_for_count().unwrap();
        game.prepare_plate_appearance(batter.runner());
        game.inning_state.ball = 2;
        game.inning_state.strike = 2;

        let point = game.add_ball();

        assert_eq!(point, 0);
        assert_eq!(game.inning_state.ball, 3);
        assert_eq!(game.inning_state.strike, 2);
        assert_eq!(game.inning_state.out, 0);
        assert_eq!(game.inning_state.active_batter.unwrap().id, batter.id);
        assert!(game.inning_state.runners.batter_runner.is_some());
    }

    #[test]
    fn add_ball_walks_batter_and_resets_plate_appearance_on_fourth_ball() {
        let mut game = game_state();
        game.advance_half_inning();
        let batter = game.active_batter_for_count().unwrap();
        game.prepare_plate_appearance(batter.runner());
        game.inning_state.ball = MAX_BALL - 1;
        game.inning_state.strike = 2;
        game.inning_state.runners.runner_2nd = Some(runner(20));

        let point = game.add_ball();

        assert_eq!(point, 0);
        assert_eq!(game.inning_state.ball, 0);
        assert_eq!(game.inning_state.strike, 0);
        assert_eq!(game.inning_state.out, 0);
        assert!(game.inning_state.active_batter.is_none());
        assert!(game.inning_state.runners.batter_runner.is_none());
        assert_eq!(game.inning_state.runners.runner_1st.unwrap().id, batter.id);
        assert_eq!(game.inning_state.runners.runner_2nd.unwrap().id, 20);
        assert!(game.inning_state.runners.runner_3rd.is_none());
    }

    #[test]
    fn add_ball_forces_runners_and_scores_when_bases_are_loaded() {
        let mut game = game_state();
        game.advance_half_inning();
        let batter = game.active_batter_for_count().unwrap();
        game.prepare_plate_appearance(batter.runner());
        game.inning_state.ball = MAX_BALL - 1;
        game.inning_state.runners.runner_1st = Some(runner(10));
        game.inning_state.runners.runner_2nd = Some(runner(20));
        game.inning_state.runners.runner_3rd = Some(runner(30));

        let point = game.add_ball();

        assert_eq!(point, 1);
        assert_eq!(game.inning_state.runners.runner_1st.unwrap().id, batter.id);
        assert_eq!(game.inning_state.runners.runner_2nd.unwrap().id, 10);
        assert_eq!(game.inning_state.runners.runner_3rd.unwrap().id, 20);
        assert!(game.inning_state.active_batter.is_none());
        assert!(game.inning_state.runners.batter_runner.is_none());
    }

    #[test]
    fn resolve_wild_pitch_advances_base_runners_without_moving_batter_before_walk() {
        let mut game = game_state();
        game.advance_half_inning();
        let batter = game.active_batter_for_count().unwrap();
        game.prepare_plate_appearance(batter.runner());
        game.inning_state.ball = 1;
        game.inning_state.strike = 2;
        game.inning_state.runners.runner_1st = Some(runner(10));
        game.inning_state.runners.runner_3rd = Some(runner(30));

        let point = game.resolve_wild_pitch();

        assert_eq!(point, 1);
        assert_eq!(game.inning_state.ball, 2);
        assert_eq!(game.inning_state.strike, 2);
        assert_eq!(game.inning_state.active_batter.unwrap().id, batter.id);
        assert_eq!(
            game.inning_state.runners.batter_runner.unwrap().id,
            batter.id
        );
        assert!(game.inning_state.runners.runner_1st.is_none());
        assert_eq!(game.inning_state.runners.runner_2nd.unwrap().id, 10);
        assert!(game.inning_state.runners.runner_3rd.is_none());
    }

    #[test]
    fn resolve_wild_pitch_on_ball_four_advances_runners_and_walks_batter() {
        let mut game = game_state();
        game.advance_half_inning();
        let batter = game.active_batter_for_count().unwrap();
        game.prepare_plate_appearance(batter.runner());
        game.inning_state.ball = MAX_BALL - 1;
        game.inning_state.strike = 2;
        game.inning_state.runners.runner_1st = Some(runner(10));
        game.inning_state.runners.runner_2nd = Some(runner(20));
        game.inning_state.runners.runner_3rd = Some(runner(30));

        let point = game.resolve_wild_pitch();

        assert_eq!(point, 1);
        assert_eq!(game.inning_state.ball, 0);
        assert_eq!(game.inning_state.strike, 0);
        assert!(game.inning_state.active_batter.is_none());
        assert!(game.inning_state.runners.batter_runner.is_none());
        assert_eq!(game.inning_state.runners.runner_1st.unwrap().id, batter.id);
        assert_eq!(game.inning_state.runners.runner_2nd.unwrap().id, 10);
        assert_eq!(game.inning_state.runners.runner_3rd.unwrap().id, 20);
    }

    #[test]
    fn current_pitcher_returns_fielding_teams_pitcher() {
        let mut game = game_state();

        game.advance_half_inning();
        assert_eq!(game.current_pitcher().id, 101);

        game.advance_half_inning();
        assert_eq!(game.current_pitcher().id, 1);
    }

    #[test]
    fn change_risk_tolerance_updates_current_fielding_lineup() {
        let mut game = game_state();

        game.change_risk_tolerance(FielderRiskTolerance::Conservative);
        assert!(
            game.away_lineup
                .fielders()
                .iter()
                .all(|fielder| fielder.risk_tolerance == FielderRiskTolerance::Conservative)
        );
        assert!(
            game.home_lineup
                .fielders()
                .iter()
                .all(|fielder| fielder.risk_tolerance == FielderRiskTolerance::Balanced)
        );

        game.advance_half_inning();
        game.change_risk_tolerance(FielderRiskTolerance::Aggressive);
        assert!(
            game.home_lineup
                .fielders()
                .iter()
                .all(|fielder| fielder.risk_tolerance == FielderRiskTolerance::Aggressive)
        );
        assert!(
            game.current_fielders()
                .iter()
                .all(|fielder| fielder.risk_tolerance == FielderRiskTolerance::Aggressive)
        );
    }

    #[test]
    fn progress_is_ongoing_before_ninth_inning_regardless_of_score() {
        let mut game = game_state();
        game.inning_seq = MAX_INNING - 1;
        game.inning_tb = TB::Bottom;
        game.home_total_point = 10;

        assert_eq!(game.progress(), GameProgress::Ongoing);
    }

    #[test]
    fn progress_is_game_set_at_top_of_ninth_when_home_leads() {
        let mut game = game_state();
        game.inning_seq = MAX_INNING;
        game.inning_tb = TB::Top;
        game.home_total_point = 1;

        assert_eq!(game.progress(), GameProgress::GameSet);
    }

    #[test]
    fn progress_is_ongoing_at_top_of_ninth_when_home_does_not_lead() {
        let mut game = game_state();
        game.inning_seq = MAX_INNING;
        game.inning_tb = TB::Top;

        assert_eq!(game.progress(), GameProgress::Ongoing);

        game.away_total_point = 1;
        assert_eq!(game.progress(), GameProgress::Ongoing);
    }

    #[test]
    fn progress_is_walk_off_at_bottom_of_ninth_when_home_takes_lead() {
        let mut game = game_state();
        game.inning_seq = MAX_INNING;
        game.inning_tb = TB::Bottom;
        game.home_total_point = 1;

        assert_eq!(game.progress(), GameProgress::WalkOff);
    }

    #[test]
    fn inning_state_initializes_empty_and_tracks_counts_and_outs() {
        let mut inning = InningState::new();

        assert_no_runners(&inning);
        assert_eq!(inning.ball, 0);
        assert_eq!(inning.strike, 0);
        assert_eq!(inning.out, 0);
        assert_eq!(inning.inning_progress(), InningProgress::Ongoing);

        inning.add_out();
        inning.add_out();
        assert_eq!(inning.out, 2);
        assert_eq!(inning.inning_progress(), InningProgress::Ongoing);

        inning.add_out();
        assert_eq!(inning.inning_progress(), InningProgress::Over);

        inning.add_out();
        assert_eq!(inning.inning_progress(), InningProgress::Over);
    }

    #[test]
    fn runner_dependent_inning_state_rules_follow_current_runners() {
        let runner = ActiveRunner {
            id: 1,
            skills: RunningSkills {
                speed: 7.7,
                lead_distance: 4.0,
                start_reaction: 0.1,
            },
        };
        let mut inning = InningState::new();

        assert!(!inning.allows_tagup());
        assert!(!inning.can_double_play());

        inning.runners.runner_1st = Some(runner);
        assert!(!inning.allows_tagup());
        assert!(inning.can_double_play());

        inning.runners.runner_2nd = Some(runner);
        assert!(inning.allows_tagup());
        assert!(inning.can_double_play());

        inning.out = 2;
        assert!(inning.allows_tagup());
        assert!(!inning.can_double_play());

        inning.out = MAX_OUT;
        assert!(!inning.allows_tagup());
    }
}
