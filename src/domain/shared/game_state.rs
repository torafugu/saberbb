use super::game::{BaseCode, BattingResult, Count, Inning, TB};
use super::player::{Player, Position};
use super::team::Lineup;
use crate::domain::random_provider::RandomProvider;
use crate::domain::resolver::batting_resolver::simulate_batting;
use crate::domain::resolver::running_resolver::RunnersOnBase;
use crate::domain::shared::game_history::BattingResultHistory;
use crate::domain::shared::stadium::Base;
use crate::domain::util::is_base_occupied;
use crate::t;
use std::fmt;

pub const MAX_INNING: u8 = 9;
pub const MAX_OUT: u8 = 3;

// TODO: Move to running strategy
const TEMP_ATTEMPT_STEAL_BASE_WEIGHT: f64 = 0.3;

#[derive(thiserror::Error, Debug)]
pub enum GameError {
    #[error("No players for position: {0}")]
    NoPlayerFor(String),

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

#[derive(Debug, Clone, PartialEq, Eq)]
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

#[derive(Debug, PartialEq, Eq)]
pub enum BallStatus {
    InPlay,
    Dead,
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
    pub away_total_point: u8,
    pub home_total_point: u8,
    pub away_lineup: Lineup,
    pub home_lineup: Lineup,
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
            inning_seq: 0, // Initialization: 1 should be set at the beginning of the game
            inning_tb: TB::Bottom, // Initialization: Top should be set at the beginning of the game
            away_total_point: 0,
            home_total_point: 0,
            away_lineup: away_lineup,
            home_lineup: home_lineup,
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

    pub fn advance_half_inning(&mut self) -> Inning {
        self.inning_tb = match self.inning_tb {
            TB::Top => TB::Bottom,
            TB::Bottom => {
                self.inning_seq += 1;
                TB::Top
            }
        };

        Inning {
            seq: self.inning_seq,
            tb: self.inning_tb,
            counts: Vec::new(),
        }
    }

    fn current_team_id(&self) -> u16 {
        if self.inning_tb == TB::Top {
            self.away_team_id
        } else {
            self.home_team_id
        }
    }

    pub fn current_pitcher(&mut self) -> Result<Player, GameError> {
        let lineup = if self.inning_tb == TB::Top {
            &self.home_lineup
        } else {
            &self.away_lineup
        };

        lineup
            .batters
            .iter()
            .find(|i| i.is(Position::P))
            .map(|i| i.player.clone())
            .ok_or_else(|| GameError::NoPlayerFor(Position::P.to_string()))
    }

    pub fn current_batter(&mut self) -> Result<Player, GameError> {
        if self.inning_tb == TB::Top {
            if let Some(player) = self.away_lineup.next() {
                Ok(player)
            } else {
                return Err(GameError::CurrentBatter);
            }
        } else {
            if let Some(player) = self.home_lineup.next() {
                Ok(player)
            } else {
                return Err(GameError::CurrentBatter);
            }
        }
    }

    pub fn batting_resolve(&mut self) -> Result<BattingResult, GameError> {
        Ok(simulate_batting(&self.current_batter()?))
    }

    pub fn add_batting_result_hisrory(
        &mut self,
        count_seq: u8,
        batting_result: &BattingResult,
    ) -> Result<BattingResultHistory, GameError> {
        Ok(BattingResultHistory {
            inning_seq: self.inning_seq,
            inning_tb: self.inning_tb,
            count_seq: count_seq,
            team_id: self.current_team_id(),
            pitcher: self.current_pitcher()?,
            batter: self.current_batter()?,
            result: batting_result.clone(),
        })
    }

    pub fn add_point(&mut self, point: u8) {
        match self.inning_tb {
            TB::Top => self.away_total_point += point,
            TB::Bottom => self.home_total_point += point,
        };
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InningProgress {
    Ongoing,
    HalfInningOver,
}

#[derive(Debug)]
pub struct InningState {
    pub count_seq: u8,
    pub bases_occupied: u8,
    pub runners: RunnersOnBase,
    pub ball: u8,
    pub strike: u8,
    pub out: u8,
}
impl InningState {
    pub fn new() -> InningState {
        InningState {
            count_seq: 0,
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
        self.out == 2
            || (!self.runners.has_runner_on(Base::Third)
                && !self.runners.has_runner_on(Base::Second))
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

    pub fn add_count(&mut self, batting_result: &BattingResult) -> Count {
        self.count_seq += 1;

        if batting_result.is_out() {
            self.add_out();
        }
        let point = self.advance(&batting_result);

        // TODO: Consider ball updated
        // TODO: Consider strike updated
        Count {
            seq: self.count_seq,
            bases_occupied: self.bases_occupied,
            ball: self.ball,
            strike: self.strike,
            out: self.out,
            point: point,
        }
    }

    pub fn add_out(&mut self) {
        self.out += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::RL;
    use crate::domain::shared::team::BattingOrder;

    fn lineup(first_player_id: u32) -> Lineup {
        let batters = Position::ALL_NO_DH
            .into_iter()
            .enumerate()
            .map(|(index, position)| BattingOrder {
                index: (index + 1) as u8,
                position,
                player: Player::new(
                    first_player_id + index as u32,
                    &format!("Player{}", first_player_id + index as u32),
                    "Test",
                    25,
                    RL::Right,
                    RL::Right,
                    0.0,
                    0.0,
                ),
            })
            .collect();

        Lineup::new(batters).unwrap()
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
        assert_eq!(game.away_lineup.current_index, 0);
        assert_eq!(game.home_lineup.current_index, 0);
    }

    #[test]
    fn advance_half_inning_cycles_top_and_bottom() {
        let mut game = game_state();

        let first_top = game.advance_half_inning();
        assert_eq!(first_top.seq, 1);
        assert_eq!(first_top.tb, TB::Top);
        assert!(first_top.counts.is_empty());

        let first_bottom = game.advance_half_inning();
        assert_eq!(first_bottom.seq, 1);
        assert_eq!(first_bottom.tb, TB::Bottom);

        let second_top = game.advance_half_inning();
        assert_eq!(second_top.seq, 2);
        assert_eq!(second_top.tb, TB::Top);
    }

    #[test]
    fn add_point_updates_current_batting_team_only() {
        let mut game = game_state();

        game.advance_half_inning();
        game.add_point(2);
        assert_eq!(game.away_total_point, 2);
        assert_eq!(game.home_total_point, 0);

        game.advance_half_inning();
        game.add_point(3);
        assert_eq!(game.away_total_point, 2);
        assert_eq!(game.home_total_point, 3);
    }

    #[test]
    fn current_batter_uses_correct_lineup_and_wraps_after_nine_batters() {
        let mut game = game_state();

        game.advance_half_inning();
        for expected_id in 1..=9 {
            assert_eq!(game.current_batter().unwrap().id, expected_id);
        }
        assert_eq!(game.current_batter().unwrap().id, 1);

        game.advance_half_inning();
        assert_eq!(game.current_batter().unwrap().id, 101);
    }

    #[test]
    fn current_pitcher_returns_fielding_teams_pitcher() {
        let mut game = game_state();

        game.advance_half_inning();
        assert_eq!(game.current_pitcher().unwrap().id, 101);

        game.advance_half_inning();
        assert_eq!(game.current_pitcher().unwrap().id, 1);
    }

    #[test]
    fn current_pitcher_returns_error_when_lineup_has_no_pitcher() {
        let mut game = game_state();
        game.home_lineup.batters[0].position = Position::C;
        game.advance_half_inning();

        assert!(matches!(
            game.current_pitcher(),
            Err(GameError::NoPlayerFor(_))
        ));
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

        assert_eq!(inning.count_seq, 0);
        assert_eq!(inning.bases_occupied, 0);
        assert_eq!(inning.out, 0);
        assert_eq!(inning.progress(), InningProgress::Ongoing);

        inning.add_count(&BattingResult::Single);
        inning.add_out();
        inning.add_out();
        assert_eq!(inning.count_seq, 1);
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
                    count_seq: 0,
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
