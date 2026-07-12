use super::game::{BaseCode, BattingResult, Count, Inning, TB};
use super::player::{BatterInfo, CatcherInfo, FielderInfo, PitcherInfo, Position, RunningSkills};
use super::team::Lineup;
use crate::domain::random_provider::RandomProvider;
use crate::domain::resolver::fielding_config::{
    FENCE_BOUNCE_COEFF, FENCE_DISTANCE, FIRST_BOUNCE_TIME,
};
use crate::domain::resolver::fielding_resolver::BoundedBallResult;
use crate::domain::resolver::running_resolver::RunnersOnBase;
use crate::domain::shared::ball::{BattedBall, FieldedBall, TrajectoryType};
use crate::domain::shared::game_history::BattingResultHistory;
use crate::domain::shared::stadium::Base;
use crate::domain::util::{PolarPosition, calculate_polar_distance, is_base_occupied};
use crate::t;
use serde::{Deserialize, Serialize};
use std::fmt;

pub const MAX_INNING: u8 = 9;
pub const MAX_OUT: u8 = 3;

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

    #[error("Same target bases are passeed")]
    SameTargetBase,

    #[error("Path of from base and to base is not supported")]
    UnsupportedPath,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActiveBatter {
    pub player_id: i64,
    pub index: u8,
    pub batter: BatterInfo,
}
impl ActiveBatter {
    pub fn new(player_id: i64, batter: BatterInfo) -> Self {
        Self {
            player_id: player_id,
            index: 0,
            batter: batter,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ActiveRunner {
    pub player_id: i64,
    pub skills: RunningSkills,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActivePitcher {
    pub player_id: i64,
    pub pitcher: PitcherInfo,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ActiveCatcher {
    pub player_id: i64,
    pub catcher: CatcherInfo,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ActiveFielder {
    pub position: Position,
    pub player_id: i64,
    pub info: FielderInfo,
    pub polar_position: PolarPosition,
}
impl ActiveFielder {
    pub fn new(position: Position, player_id: i64, info: FielderInfo) -> Self {
        Self {
            position: position,
            player_id: player_id,
            info: info,
            polar_position: PolarPosition::default(),
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

    pub fn try_catch(&self, ball: &BattedBall) -> FieldedBall {
        // $$\text{arrival\_time} = \text{reaction\_time} + \frac{\text{required\_distance}}{\text{fielder\_speed}}$$
        // 1. Calculate straight-line distance from position to landing point
        let required_distance =
            calculate_polar_distance(&self.polar_position, &ball.polar_position);
        let dy = self.y() - ball.y();

        // 3. Adjust initial reaction speed based on hit type (secret ingredient)
        let mut final_reaction = self.info.reaction;
        if ball.trajectory == TrajectoryType::Liner && dy < 0.0 {
            // Delay reaction when moving forward on a liner (harder to judge)
            final_reaction += self.info.reaction;
        }

        // 4. Calculate arrival time (seconds)
        let arrival_time = final_reaction + (required_distance / self.info.running_speed);

        // 5. Compare arrival time vs hang time
        if ball.trajectory == TrajectoryType::Grounder {
            return FieldedBall {
                ball: ball.clone(),
                fielded_by: self.position,
                time_to_field: arrival_time,
                is_fly_catch: false,
            };
        }

        if arrival_time <= ball.hang_time {
            return FieldedBall {
                ball: ball.clone(),
                fielded_by: self.position,
                time_to_field: ball.hang_time, // Fielder need to wait until catch.
                is_fly_catch: true,
            };
        }

        let bounded_ball = self.process_bounded_ball(ball);

        let mut final_ball = ball.clone();
        final_ball.polar_position.distance = bounded_ball.final_distance;

        FieldedBall {
            ball: final_ball,
            fielded_by: self.position,
            time_to_field: bounded_ball.time_to_fumble,
            is_fly_catch: false,
        }
    }

    // Processing when a fly/liner wasn't caught (became a hit)
    fn process_bounded_ball(&self, ball: &BattedBall) -> BoundedBallResult {
        // 1. Damping coefficient at the moment of the first bounce (liner bounces sharply, fly dies)
        let k_impact = match ball.trajectory {
            TrajectoryType::Liner => 0.60,
            TrajectoryType::Fly => 0.35,
            _ => 0.0,
        };

        // 2. Initial speed as a grounder right after the bounce
        let v_horizontal = ball.launch_speed_ms() * ball.azimuth().cos() * 0.7; // Velocity including in-flight air resistance
        let v_bounce = v_horizontal * k_impact;

        // 3. Additional rolling distance and time until stop
        let roll_distance = v_bounce * 1.8;

        // 4. Provisional final resting position (landing point + roll distance)
        let mut final_distance = ball.distance() + roll_distance;

        // The fence bounce (cushion) logic
        if final_distance > FENCE_DISTANCE {
            let overflow = final_distance - FENCE_DISTANCE;
            final_distance = FENCE_DISTANCE - (overflow * FENCE_BOUNCE_COEFF);
        }

        // 5. Defense: time for the fielder to chase down and pick up the rolling ball
        // The fielder was initially running toward the landing point but didn't make it.
        // Simple calculation of time to loop around toward the direction the ball rolled (final_distance)
        let fielder_distance_to_ball = (final_distance - self.distance()).abs();

        // Time for the fielder to reach the final resting point (or cushion treatment position)
        let fielder_arrival_time =
            self.info.reaction + (fielder_distance_to_ball / self.info.running_speed);

        // Time the fielder picks up the ball (either waiting for it to stop or cutting it off mid-roll)
        let time_to_pick_up = fielder_arrival_time.max(ball.hang_time + FIRST_BOUNCE_TIME);
        BoundedBallResult {
            final_distance,
            time_to_fumble: time_to_pick_up, // ★This becomes the time_to_field for the next throw play!
        }
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
    away_team_id: u16,
    home_team_id: u16,
    pub inning_seq: u8,
    pub inning_tb: TB,
    pub count_seq: u16,
    pub away_total_point: u8,
    pub home_total_point: u8,
    pub away_lineup: Lineup,
    pub home_lineup: Lineup,
    pub inning_state: InningState,
    pub inning: Inning,
    pub batting_result_hisrory: BattingResultHistory,
}
impl GameState {
    pub fn new(
        away_team_id: u16,
        home_team_id: u16,
        away_lineup: Lineup,
        home_lineup: Lineup,
    ) -> Result<GameState, GameError> {
        Ok(GameState {
            away_team_id: away_team_id,
            home_team_id: home_team_id,
            inning_seq: 0, // CONSTRAINT: Initialization: 1 should be set at the beginning of the game
            inning_tb: TB::Bottom, // CONSTRAINT: Initialization: Top should be set at the beginning of the game
            count_seq: 0,
            away_total_point: 0,
            home_total_point: 0,
            away_lineup: away_lineup,
            home_lineup: home_lineup,
            inning_state: InningState::new(),
            inning: Inning::new(),
            batting_result_hisrory: BattingResultHistory::new(),
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

    fn current_team_id(&self) -> u16 {
        if self.inning_tb == TB::Top {
            self.away_team_id
        } else {
            self.home_team_id
        }
    }

    pub fn current_pitcher(&mut self) -> &ActivePitcher {
        if self.inning_tb == TB::Top {
            &self.home_lineup.pitcher
        } else {
            &self.away_lineup.pitcher
        }
    }

    pub fn current_batter(&mut self) -> Result<ActiveBatter, GameError> {
        if self.inning_tb == TB::Top {
            self.away_lineup.next()
        } else {
            self.home_lineup.next()
        }
    }

    pub fn batting_resolve(&mut self) -> Result<(), GameError> {
        // TODO: replace to new batting logic. // Ok(simulate_batting(&self.current_batter()?))

        let batting_result = BattingResult::Out;

        self.count_seq += 1;

        if batting_result.is_out() {
            self.inning_state.add_out();
        }
        let point = self.inning_state.advance(&batting_result);

        match self.inning_tb {
            TB::Top => self.away_total_point += point,
            TB::Bottom => self.home_total_point += point,
        };

        // TODO: Consider ball updated
        // TODO: Consider strike updated
        self.inning.add_count(Count {
            seq: self.count_seq,
            bases_occupied: self.inning_state.bases_occupied,
            ball: self.inning_state.ball,
            strike: self.inning_state.strike,
            out: self.inning_state.out,
            point: point,
        });

        self.batting_result_hisrory = BattingResultHistory {
            count_seq: self.count_seq,
            pitcher_id: self.current_pitcher().player_id,
            batter_id: self.current_batter()?.player_id,
            result: batting_result,
        };

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InningProgress {
    Ongoing,
    HalfInningOver,
}

#[derive(Debug)]
pub struct InningState {
    pub bases_occupied: u8,
    pub runners: RunnersOnBase,
    pub ball: u8,
    pub strike: u8,
    pub out: u8,
}
impl InningState {
    pub fn new() -> InningState {
        InningState {
            bases_occupied: 0,
            runners: RunnersOnBase::default(),
            ball: 0,
            strike: 0,
            out: 0,
        }
    }

    pub fn progress(&self) -> InningProgress {
        if self.out >= MAX_OUT {
            return InningProgress::HalfInningOver;
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
    pub fn can_steal_base(&self, mut rng: Box<dyn RandomProvider>) -> bool {
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

    // TODO: To be replaced by RunnersOnBase
    pub fn advance(&mut self, result: &BattingResult) -> u8 {
        let mut points = 0u8;

        match result {
            BattingResult::Single => {
                if is_base_occupied(self.bases_occupied, BaseCode::Third) {
                    points += 1;
                }
                self.shift_runners(1); // All runners go to next base
                self.put_runner_on(BaseCode::First);
            }
            BattingResult::Double => {
                if is_base_occupied(self.bases_occupied, BaseCode::Third) {
                    points += 1;
                }
                if is_base_occupied(self.bases_occupied, BaseCode::Second) {
                    points += 1;
                }
                self.shift_runners(2);
                self.put_runner_on(BaseCode::Second);
            }
            BattingResult::Triple => {
                points += self.how_many_runners(); // All runners home in
                self.clear();
                self.put_runner_on(BaseCode::Third);
            }
            BattingResult::HomeRun => {
                points += self.how_many_runners() + 1; // All runners and the batter home in
                self.clear();
            }
            BattingResult::Foul => {}
            BattingResult::FieldersChoice => {} // TODO: Do something
            BattingResult::Out => {}
        }
        points
    }

    // TODO: To be replaced by RunnersOnBase
    fn shift_runners(&mut self, bases: u8) {
        self.bases_occupied = (self.bases_occupied << bases) & 0b00000111;
    }

    // TODO: To be replaced by RunnersOnBase
    fn put_runner_on(&mut self, base: BaseCode) {
        self.bases_occupied |= 1 << (base as u8);
    }

    // TODO: To be replaced by RunnersOnBase
    fn clear(&mut self) {
        self.bases_occupied = 0;
    }

    // TODO: To be replaced by RunnersOnBase
    fn how_many_runners(&self) -> u8 {
        self.bases_occupied.count_ones() as u8
    }

    pub fn add_out(&mut self) {
        self.out += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::{FielderType, PitcherStyle, RL};

    fn batter(player_id: i64) -> ActiveBatter {
        ActiveBatter::new(
            player_id,
            BatterInfo {
                batting_side: RL::Right,
                swing_speed: player_id as f64,
                weight_pull: 0.2,
                weight_center: 0.2,
                weight_opposite: 0.2,
                weight_foul_left: 0.2,
                weight_foul_right: 0.2,
            },
        )
    }

    fn fielder_info(fielder_type: FielderType) -> FielderInfo {
        FielderInfo {
            fielder_type,
            throw_speed: 35.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.6,
        }
    }

    fn fielder_type(position: Position) -> FielderType {
        match position {
            Position::P => FielderType::Pitcher,
            Position::C => FielderType::Catcher,
            Position::FB | Position::TB => FielderType::CornerInfielder,
            Position::SB | Position::SS => FielderType::MiddleInfielder,
            Position::LF | Position::CF | Position::RF => FielderType::Outfielder,
            Position::DH => unreachable!("DH is not a fielder"),
        }
    }

    fn active_fielder(position: Position, player_id: i64) -> ActiveFielder {
        ActiveFielder::new(position, player_id, fielder_info(fielder_type(position)))
    }

    fn pitcher(player_id: i64) -> ActivePitcher {
        let info = fielder_info(FielderType::Pitcher);
        ActivePitcher {
            player_id,
            pitcher: PitcherInfo {
                pitcher_style: PitcherStyle::BalancedPitcher,
                velocity: 145.0,
                control: 0.5,
                stamina: 0.5,
                injury_proneness: 0.5,
                clutch: 0.5,
                hpp: 0.5,
                platoon_splitting: 0.5,
                delivery_motion_time: 1.4,
                pitch_skills: Vec::new(),
                fielder_info: info,
            },
        }
    }

    fn catcher(player_id: i64) -> ActiveCatcher {
        ActiveCatcher {
            player_id,
            catcher: CatcherInfo {
                fielder_info: fielder_info(FielderType::Catcher),
            },
        }
    }

    fn lineup(first_player_id: i64) -> Lineup {
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
        let batters = batter_positions
            .into_iter()
            .enumerate()
            .map(|(index, _position)| {
                let mut active_batter = batter(first_player_id + index as i64);
                active_batter.index = (index + 1) as u8;
                active_batter
            })
            .collect();

        let fielders = [
            Position::P,
            Position::C,
            Position::FB,
            Position::SB,
            Position::TB,
            Position::SS,
            Position::LF,
            Position::CF,
            Position::RF,
        ]
        .into_iter()
        .enumerate()
        .map(|(index, position)| active_fielder(position, first_player_id + index as i64))
        .collect();

        Lineup::new(
            batters,
            fielders,
            pitcher(first_player_id),
            catcher(first_player_id + 1),
        )
        .unwrap()
    }

    fn game_state() -> GameState {
        GameState::new(1, 4, lineup(1), lineup(101)).unwrap()
    }

    #[test]
    fn new_initializes_game_before_first_inning() {
        let game = game_state();

        assert_eq!(game.inning_seq, 0);
        assert_eq!(game.inning_tb, TB::Bottom);
        assert_eq!(game.away_total_point, 0);
        assert_eq!(game.home_total_point, 0);
        assert_eq!(game.away_lineup.batters.len(), 9);
        assert_eq!(game.home_lineup.batters.len(), 9);
        assert_eq!(game.away_lineup.fielders.len(), 9);
        assert_eq!(game.home_lineup.fielders.len(), 9);
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
        assert_eq!(game.inning_state.bases_occupied, 0);
        assert_eq!(game.inning_state.out, 0);

        game.inning_state.add_out();
        game.advance_half_inning();
        assert_eq!(game.inning.seq, 1);
        assert_eq!(game.inning.tb, TB::Bottom);
        assert_eq!(game.inning_state.bases_occupied, 0);
        assert_eq!(game.inning_state.out, 0);

        game.advance_half_inning();
        assert_eq!(game.inning.seq, 2);
        assert_eq!(game.inning.tb, TB::Top);
    }

    #[test]
    fn batting_resolve_records_an_out_for_current_batting_team() {
        let mut game = game_state();

        game.advance_half_inning();
        game.batting_resolve().unwrap();

        assert_eq!(game.count_seq, 1);
        assert_eq!(game.away_total_point, 0);
        assert_eq!(game.home_total_point, 0);
        assert_eq!(game.inning_state.out, 1);
        assert_eq!(game.inning.counts.len(), 1);
        assert_eq!(game.inning.counts[0].seq, 1);
        assert_eq!(game.inning.counts[0].out, 1);
        assert_eq!(game.batting_result_hisrory.count_seq, 1);
        assert_eq!(game.batting_result_hisrory.pitcher_id, 101);
        assert_eq!(game.batting_result_hisrory.batter_id, 1);
        assert_eq!(game.away_lineup.current_index, 1);

        game.advance_half_inning();
        game.batting_resolve().unwrap();

        assert_eq!(game.count_seq, 2);
        assert_eq!(game.away_total_point, 0);
        assert_eq!(game.home_total_point, 0);
        assert_eq!(game.inning_state.out, 1);
        assert_eq!(game.batting_result_hisrory.pitcher_id, 1);
        assert_eq!(game.batting_result_hisrory.batter_id, 101);
        assert_eq!(game.home_lineup.current_index, 1);
    }

    #[test]
    fn current_batter_uses_correct_lineup_and_wraps_after_nine_batters() {
        let mut game = game_state();

        game.advance_half_inning();
        for expected_id in 1..=9 {
            assert_eq!(game.current_batter().unwrap().player_id, expected_id as i64);
        }
        assert_eq!(game.current_batter().unwrap().player_id, 1);

        game.advance_half_inning();
        assert_eq!(game.current_batter().unwrap().player_id, 101);
    }

    #[test]
    fn current_pitcher_returns_fielding_teams_pitcher() {
        let mut game = game_state();

        game.advance_half_inning();
        assert_eq!(game.current_pitcher().player_id, 101);

        game.advance_half_inning();
        assert_eq!(game.current_pitcher().player_id, 1);
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

        assert_eq!(inning.bases_occupied, 0);
        assert!(!inning.runners.has_runner_on(Base::First));
        assert!(!inning.runners.has_runner_on(Base::Second));
        assert!(!inning.runners.has_runner_on(Base::Third));
        assert_eq!(inning.ball, 0);
        assert_eq!(inning.strike, 0);
        assert_eq!(inning.out, 0);
        assert_eq!(inning.progress(), InningProgress::Ongoing);

        inning.add_out();
        inning.add_out();
        assert_eq!(inning.out, 2);
        assert_eq!(inning.progress(), InningProgress::Ongoing);

        inning.add_out();
        assert_eq!(inning.progress(), InningProgress::HalfInningOver);

        inning.add_out();
        assert_eq!(inning.progress(), InningProgress::HalfInningOver);
    }

    #[test]
    fn advance_handles_every_base_occupancy_for_every_result() {
        for bases in 0u8..=0b111 {
            let runners = bases.count_ones() as u8;
            let runner_on_second = (bases >> BaseCode::Second as u8) & 1;
            let runner_on_third = (bases >> BaseCode::Third as u8) & 1;

            let cases = [
                (
                    BattingResult::Single,
                    ((bases << 1) & 0b111) | 0b001,
                    runner_on_third,
                ),
                (
                    BattingResult::Double,
                    ((bases << 2) & 0b111) | 0b010,
                    runner_on_second + runner_on_third,
                ),
                (BattingResult::Triple, 0b100, runners),
                (BattingResult::HomeRun, 0b000, runners + 1),
                (BattingResult::Out, bases, 0),
            ];

            for (result, expected_bases, expected_points) in cases {
                let mut inning = InningState {
                    bases_occupied: bases,
                    runners: RunnersOnBase::default(),
                    ball: 0,
                    strike: 0,
                    out: 0,
                };

                assert_eq!(
                    inning.advance(&result),
                    expected_points,
                    "{result:?} with bases {bases:03b}"
                );
                assert_eq!(
                    inning.bases_occupied, expected_bases,
                    "{result:?} with bases {bases:03b}"
                );
            }
        }
    }
}
