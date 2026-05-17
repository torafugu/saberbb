use super::game::{Count, Game, Inning};
use super::player::Player;
use super::types::{Base, BattingResult, InningType, Position};
use crate::domain::resolver::simulate_batting;
use crate::domain::utils::is_base_occupied;
use crate::t;
use std::sync::Arc;

pub const MAX_INNING: u8 = 9;
pub const MAX_OUT: u8 = 3;

#[derive(Debug, PartialEq)]
pub enum GameProgress {
    Ongoing,
    WalkOff,
    GameSet,
    Postponed,
}

#[derive(Debug)]
pub struct GameState {
    pub inning_seq: u8,
    pub inning_tb: InningType,
    pub away_total_point: u8,
    pub home_total_point: u8,
    pub away_batting_lineup: Lineup,
    pub home_batting_lineup: Lineup,
}
impl GameState {
    pub fn new() -> GameState {
        GameState {
            inning_seq: 0,                 // Initialization
            inning_tb: InningType::Bottom, // Initialization
            away_total_point: 0,
            home_total_point: 0,
            away_batting_lineup: Lineup {
                current_index: 0,
                batting_orders: Vec::new(),
            },
            home_batting_lineup: Lineup {
                current_index: 0,
                batting_orders: Vec::new(),
            },
        }
    }

    fn is_top(&self) -> bool {
        self.inning_tb == InningType::Top
    }

    fn is_bottom(&self) -> bool {
        self.inning_tb == InningType::Bottom
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
            InningType::Top => InningType::Bottom,
            InningType::Bottom => {
                self.inning_seq += 1;
                InningType::Top
            }
        };

        Inning {
            seq: self.inning_seq,
            tb: self.inning_tb,
            counts: Vec::new(),
            point: 0,
        }
    }

    pub fn current_batter(&mut self) -> Player {
        if self.inning_tb == InningType::Top {
            self.away_batting_lineup.next().expect(&t!("lineup_failed"))
        } else {
            self.home_batting_lineup.next().expect(&t!("lineup_failed"))
        }
    }

    pub fn add_point(&mut self, point: u8) {
        match self.inning_tb {
            InningType::Top => self.away_total_point += point,
            InningType::Bottom => self.home_total_point += point,
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
    pub out: u8,
}
impl InningState {
    pub fn new() -> InningState {
        InningState {
            count_seq: 0,
            bases_occupied: 0,
            out: 0,
        }
    }

    pub fn progress(&self) -> InningProgress {
        if self.out >= MAX_OUT {
            return InningProgress::HalfInningOver;
        }

        InningProgress::Ongoing
    }

    pub fn batting_resolve(&mut self, batter: &Player) -> Count {
        let batting_result = simulate_batting(batter);
        if batting_result.is_out() {
            self.add_out(1);
        }
        let point = self.advance(&batting_result);
        Count {
            seq: self.count_seq,
            bases_occupied: self.bases_occupied,
            batter: Arc::new(batter.clone()),
            result: batting_result,
            point: point,
            out: self.out,
        }
    }

    pub fn advance(&mut self, result: &BattingResult) -> u8 {
        let mut points = 0u8;

        match result {
            BattingResult::Single => {
                if is_base_occupied(self.bases_occupied, Base::Third) {
                    points += 1;
                }
                self.shift_runners(1); // All runners go to next base
                self.put_runner_on(Base::First);
            }
            BattingResult::Double => {
                if is_base_occupied(self.bases_occupied, Base::Third) {
                    points += 1;
                }
                if is_base_occupied(self.bases_occupied, Base::Second) {
                    points += 1;
                }
                self.shift_runners(2);
                self.put_runner_on(Base::Second);
            }
            BattingResult::Triple => {
                points += self.how_many_runners(); // All runners home in
                self.clear();
                self.put_runner_on(Base::Third);
            }
            BattingResult::HomeRun => {
                points += self.how_many_runners() + 1; // All runners and the batter home in
                self.clear();
            }
            BattingResult::Out => {}
        }
        points
    }

    fn shift_runners(&mut self, bases: u8) {
        self.bases_occupied = (self.bases_occupied << bases) & 0b00000111;
    }

    fn put_runner_on(&mut self, base: Base) {
        self.bases_occupied |= 1 << (base as u8);
    }

    fn clear(&mut self) {
        self.bases_occupied = 0;
    }

    fn how_many_runners(&self) -> u8 {
        self.bases_occupied.count_ones() as u8
    }

    pub fn add_count_seq(&mut self) {
        self.count_seq += 1;
    }

    pub fn add_out(&mut self, additional: u8) {
        self.out += additional;
    }
}

#[derive(Clone, Debug)]
pub struct Lineup {
    pub current_index: usize,
    pub batting_orders: Vec<BattingOrder>,
}
impl Lineup {
    pub fn new(batting_orders: Vec<BattingOrder>) -> Self {
        Self {
            current_index: 0,
            batting_orders,
        }
    }
}
impl Iterator for Lineup {
    type Item = Player;

    fn next(&mut self) -> Option<Self::Item> {
        if self.batting_orders.is_empty() {
            return None;
        }

        let player = self.batting_orders[self.current_index].player.clone();

        // Use the modulo operator (%) to rotate the index around the range 0..N
        self.current_index = (self.current_index + 1) % self.batting_orders.len();
        Some(player)
    }
}

#[derive(Clone, Debug)]
pub struct BattingOrder {
    pub order: u8,
    pub position: Position,
    pub player: Player,
}

#[derive(Debug)]
pub struct GameCursor {
    game: Arc<Game>,
    pub inning_seq: u8,
    pub inning_tb: InningType,
    pub count_seq: u8,
    pub is_last_bottom_inning_skiped: bool,
}
impl GameCursor {
    pub fn new(game: Game) -> Self {
        Self {
            game: game.into(),
            inning_seq: 1,
            inning_tb: InningType::Top,
            count_seq: 1,
            is_last_bottom_inning_skiped: false,
        }
    }

    pub fn game_type(&self) -> String {
        self.game.game_type.to_string()
    }

    pub fn prev(&mut self) {
        if !self.is_first_count() {
            self.prev_count();
        } else if self.is_bottom_inning() {
            self.inning_tb = InningType::Top;
            self.count_seq = self.max_count_seq();
        } else if !self.is_first_inning() {
            self.prev_inning();
            self.inning_tb = InningType::Bottom;
            self.count_seq = self.max_count_seq();
        }
        self.is_last_bottom_inning_skiped = false;
    }

    pub fn next(&mut self) {
        if !self.is_last_count() {
            self.next_count();
        } else if self.is_top_inning() && !self.is_last_bottom_inning_skiped {
            self.inning_tb = InningType::Bottom;
            self.count_seq = 1;
        } else if !self.is_last_inning() {
            self.next_inning();
            self.inning_tb = InningType::Top;
            self.count_seq = 1;
        }
    }

    fn is_last_bottom_inning_skiped(&mut self) -> bool {
        self.game.away_point < self.game.home_point
    }

    fn is_first_inning(&self) -> bool {
        self.inning_seq == 1
    }

    pub fn is_last_count(&mut self) -> bool {
        self.count_seq == self.max_count_seq()
    }

    pub fn is_last_inning(&mut self) -> bool {
        self.inning_seq == self.max_inning_seq()
    }

    fn is_first_count(&self) -> bool {
        self.count_seq == 1
    }

    fn is_top_inning(&self) -> bool {
        self.inning_tb == InningType::Top
    }

    fn is_bottom_inning(&self) -> bool {
        self.inning_tb == InningType::Bottom
    }

    fn prev_count(&mut self) {
        self.count_seq -= 1;
    }

    fn next_count(&mut self) {
        self.count_seq += 1;

        if self.is_top_inning()
            && self.is_last_inning()
            && self.is_last_count()
            && self.is_last_bottom_inning_skiped()
        {
            self.is_last_bottom_inning_skiped = true;
        }
    }

    fn prev_inning(&mut self) {
        self.inning_seq -= 1;
    }

    fn next_inning(&mut self) {
        self.inning_seq += 1;
    }

    pub fn max_inning_seq(&mut self) -> u8 {
        self.game.innings.iter().map(|i| i.seq).max().unwrap_or(0)
    }

    pub fn max_count_seq(&mut self) -> u8 {
        self.current_inning().counts.len() as u8
    }

    fn current_inning(&mut self) -> Inning {
        self.game
            .innings
            .iter()
            .find(|i| i.is(self.inning_seq, self.inning_tb))
            .expect(&t!("inning_not_found"))
            .clone()
    }

    pub fn current_count(&mut self) -> Count {
        self.current_inning()
            .counts
            .iter()
            .find(|i| i.seq == self.count_seq)
            .expect(&t!("count_not_found"))
            .clone()
    }

    pub fn current_scoreboard(&mut self) -> ScoreBoard {
        let mut scoreboard = ScoreBoard {
            away_team_name: self.game.away_team.name.to_string(),
            home_team_name: self.game.home_team.name.to_string(),
            max_inning_num: self.max_inning_seq(),
            away_total_point: 0,
            home_total_point: 0,
            away_innning_points: Vec::new(),
            home_innning_points: Vec::new(),
            is_last_bottom_inning_skiped: self.is_last_bottom_inning_skiped,
        };

        'inning: for inning in &self.game.innings {
            if inning.tb == InningType::Top {
                scoreboard.away_innning_points.push(0);
            } else {
                scoreboard.home_innning_points.push(0);
            }

            for count in &inning.counts {
                if inning.tb == InningType::Top {
                    *scoreboard.away_innning_points.last_mut().unwrap() += count.point;
                    scoreboard.away_total_point += count.point;
                } else {
                    *scoreboard.home_innning_points.last_mut().unwrap() += count.point;
                    scoreboard.home_total_point += count.point;
                }
                if inning.seq == self.inning_seq
                    && inning.tb == self.inning_tb
                    && count.seq == self.count_seq
                {
                    break 'inning;
                }
            }
        }

        scoreboard
    }
}

#[derive(Debug)]
pub struct ScoreBoard {
    pub away_team_name: String,
    pub home_team_name: String,
    pub max_inning_num: u8,
    pub away_total_point: u8,
    pub home_total_point: u8,
    pub away_innning_points: Vec<u8>,
    pub home_innning_points: Vec<u8>,
    pub is_last_bottom_inning_skiped: bool,
}
impl ScoreBoard {
    pub fn away_scores() -> String {
        let scores = "0";
        scores.to_string()
    }
}
