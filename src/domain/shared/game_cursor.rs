use super::game::{Count, GameDetail, Inning, TB};
use super::player::{Player, Position};
use super::team::Team;
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum GameViewError {
    #[error("No players for position: {0}")]
    NoPlayerFor(String),

    #[error("Failed to retrieve current batter")]
    CurrentBatter,
}

#[derive(Debug)]
pub struct GameCursor {
    game: Arc<GameDetail>,
    pub inning_seq: u8,
    pub inning_tb: TB,
    pub count_seq: u8,
    pub is_last_bottom_inning_skiped: bool,
}
impl GameCursor {
    pub fn new(game: GameDetail) -> Self {
        Self {
            game: game.into(),
            inning_seq: 1,
            inning_tb: TB::Top,
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
            self.inning_tb = TB::Top;
            self.count_seq = self.max_count_seq();
        } else if !self.is_first_inning() {
            self.prev_inning();
            self.inning_tb = TB::Bottom;
            self.count_seq = self.max_count_seq();
        }
        self.is_last_bottom_inning_skiped = false;
    }

    pub fn next(&mut self) {
        if !self.is_last_count() {
            self.next_count();
        } else if self.is_top_inning() && !self.is_last_bottom_inning_skiped {
            self.inning_tb = TB::Bottom;
            self.count_seq = 1;
        } else if !self.is_last_inning() {
            self.next_inning();
            self.inning_tb = TB::Top;
            self.count_seq = 1;
        }
    }

    fn is_last_bottom_inning_skiped(&mut self) -> bool {
        self.game.away_points < self.game.home_points
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
        self.inning_tb == TB::Top
    }

    fn is_bottom_inning(&self) -> bool {
        self.inning_tb == TB::Bottom
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
            .expect("Inning is not found")
            .clone()
    }

    pub fn current_count(&mut self) -> Count {
        self.current_inning()
            .counts
            .iter()
            .find(|i| i.seq == self.count_seq)
            .expect("Count is not found")
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
            if inning.tb == TB::Top {
                scoreboard.away_innning_points.push(0);
            } else {
                scoreboard.home_innning_points.push(0);
            }

            for count in &inning.counts {
                if inning.tb == TB::Top {
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

    fn current_team(&self) -> &Team {
        if self.inning_tb == TB::Top {
            &self.game.away_team
        } else {
            &self.game.home_team
        }
    }

    pub fn current_pitcher(&mut self) -> Result<Player, GameViewError> {
        self.current_position(Position::P)
    }

    pub fn current_catcher(&mut self) -> Result<Player, GameViewError> {
        self.current_position(Position::C)
    }

    pub fn current_fb(&mut self) -> Result<Player, GameViewError> {
        self.current_position(Position::FB)
    }

    pub fn current_sb(&mut self) -> Result<Player, GameViewError> {
        self.current_position(Position::SB)
    }

    pub fn current_tb(&mut self) -> Result<Player, GameViewError> {
        self.current_position(Position::TB)
    }

    pub fn current_ss(&mut self) -> Result<Player, GameViewError> {
        self.current_position(Position::SS)
    }

    pub fn current_rf(&mut self) -> Result<Player, GameViewError> {
        self.current_position(Position::RF)
    }

    pub fn current_cf(&mut self) -> Result<Player, GameViewError> {
        self.current_position(Position::CF)
    }

    pub fn current_lf(&mut self) -> Result<Player, GameViewError> {
        self.current_position(Position::LF)
    }

    fn current_position(&mut self, position: Position) -> Result<Player, GameViewError> {
        self.game
            .batting_order_histories
            .iter()
            .find(|i| {
                i.is_position(
                    self.current_team().id,
                    position.clone(),
                    self.inning_seq,
                    self.count_seq,
                )
            })
            .map(|i| i.player.clone())
            .ok_or_else(|| GameViewError::NoPlayerFor(position.to_string()))
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
