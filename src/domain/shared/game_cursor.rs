use super::game::{BattingResult, Count, GameDetail, Inning, TB};
use super::player::{Player, Position};
use super::team::Team;
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum GameViewError {
    #[error("No players for position: {0}")]
    NoPlayerFor(String),

    #[error("Failed to retrieve current batter")]
    CurrentBatter,

    #[error("Failed to retrieve current batting result")]
    CurrentBattingResult,
}

#[derive(Debug)]
pub struct GameCursor {
    game: Arc<GameDetail>,
    pub inning_seq: u8,
    pub inning_tb: TB,
    pub count_seq: u16,
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
        } else if let Some((inning_seq, inning_tb, count_seq)) = self.prev_inning_cursor() {
            self.inning_seq = inning_seq;
            self.inning_tb = inning_tb;
            self.count_seq = count_seq;
        }
        self.is_last_bottom_inning_skiped = false;
    }

    pub fn next(&mut self) {
        if !self.is_last_count() {
            self.next_count();
        } else if let Some((inning_seq, inning_tb, count_seq)) = self.next_inning_cursor() {
            self.inning_seq = inning_seq;
            self.inning_tb = inning_tb;
            self.count_seq = count_seq;
        } else if self.is_top_inning()
            && self.is_last_inning()
            && self.is_last_bottom_inning_skiped()
        {
            self.is_last_bottom_inning_skiped = true;
        }
    }

    fn is_last_bottom_inning_skiped(&mut self) -> bool {
        self.game.away_points < self.game.home_points
    }

    pub fn is_last_count(&self) -> bool {
        self.count_seq == self.max_count_seq()
    }

    pub fn is_last_inning(&mut self) -> bool {
        self.inning_seq == self.max_inning_seq()
    }

    fn is_first_count(&self) -> bool {
        self.count_seq == self.min_count_seq()
    }

    fn is_top_inning(&self) -> bool {
        self.inning_tb == TB::Top
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

    pub fn max_inning_seq(&mut self) -> u8 {
        self.game.innings.iter().map(|i| i.seq).max().unwrap_or(0)
    }

    pub fn max_count_seq(&self) -> u16 {
        self.current_inning()
            .counts
            .iter()
            .map(|count| count.seq)
            .max()
            .unwrap_or(0)
    }

    fn min_count_seq(&self) -> u16 {
        self.current_inning()
            .counts
            .iter()
            .map(|count| count.seq)
            .min()
            .unwrap_or(0)
    }

    fn next_inning_cursor(&self) -> Option<(u8, TB, u16)> {
        let innings = self.innings_with_counts();
        let current_index = self.current_inning_index(&innings)?;
        let inning = innings.get(current_index + 1)?;
        let count_seq = inning.counts.iter().map(|count| count.seq).min()?;

        Some((inning.seq, inning.tb, count_seq))
    }

    fn prev_inning_cursor(&self) -> Option<(u8, TB, u16)> {
        let innings = self.innings_with_counts();
        let current_index = self.current_inning_index(&innings)?;
        let inning = current_index
            .checked_sub(1)
            .and_then(|index| innings.get(index))?;
        let count_seq = inning.counts.iter().map(|count| count.seq).max()?;

        Some((inning.seq, inning.tb, count_seq))
    }

    fn current_inning_index(&self, innings: &[&Inning]) -> Option<usize> {
        innings
            .iter()
            .position(|inning| inning.is(self.inning_seq, self.inning_tb))
    }

    fn innings_with_counts(&self) -> Vec<&Inning> {
        let mut innings: Vec<&Inning> = self
            .game
            .innings
            .iter()
            .filter(|inning| !inning.counts.is_empty())
            .collect();
        innings.sort_by_key(|inning| (inning.seq, Self::tb_order(inning.tb)));
        innings
    }

    fn tb_order(tb: TB) -> u8 {
        match tb {
            TB::Top => 0,
            TB::Bottom => 1,
        }
    }

    fn current_inning(&self) -> Inning {
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
            .find(|i| i.seq == self.count_seq as u16)
            .expect("Count is not found")
            .clone()
    }

    pub fn has_runner_on_first(&self) -> bool {
        self.current_running()
            .and_then(|running| running.runner_1st_id)
            .is_some()
    }

    pub fn has_runner_on_second(&self) -> bool {
        self.current_running()
            .and_then(|running| running.runner_2nd_id)
            .is_some()
    }

    pub fn has_runner_on_third(&self) -> bool {
        self.current_running()
            .and_then(|running| running.runner_3rd_id)
            .is_some()
    }

    fn current_running(&self) -> Option<crate::domain::shared::game_stats::PlayerGameRunning> {
        self.game
            .player_runnings
            .iter()
            .filter(|running| running.count_seq == self.count_seq)
            .max_by_key(|running| running.seq)
            .copied()
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
                    && count.seq == self.count_seq as u16
                {
                    break 'inning;
                }
            }
        }

        scoreboard
    }

    fn current_fielding_team(&self) -> &Team {
        if self.inning_tb == TB::Top {
            &self.game.home_team
        } else {
            &self.game.away_team
        }
    }

    fn player_by_id(&self, player_id: i64) -> Option<Player> {
        self.game
            .away_team
            .players
            .iter()
            .chain(self.game.home_team.players.iter())
            .find(|player| player.info.id == player_id)
            .cloned()
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
            .player_entries
            .iter()
            .find(|i| i.is_position(self.current_fielding_team().id, position, self.count_seq))
            .and_then(|i| self.player_by_id(i.player.id))
            .ok_or_else(|| GameViewError::NoPlayerFor(position.to_string()))
    }

    pub fn current_batter(&mut self) -> Result<Player, GameViewError> {
        self.game
            .player_battings
            .iter()
            .find(|i| i.is(self.count_seq as u16))
            .and_then(|i| self.player_by_id(i.batter.id))
            .ok_or_else(|| GameViewError::NoPlayerFor("batter".to_string()))
    }

    pub fn current_batting_result(&self) -> Result<BattingResult, GameViewError> {
        self.game
            .player_battings
            .iter()
            .find(|i| i.is(self.count_seq as u16))
            .map(|i| i.result)
            .ok_or(GameViewError::CurrentBattingResult)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::game::GameType;
    use chrono::NaiveDate;

    fn game_detail(innings: Vec<Inning>) -> GameDetail {
        GameDetail {
            id: 1,
            actual_date: NaiveDate::from_ymd_opt(2026, 7, 20).unwrap(),
            away_team: Team::min(1, "away"),
            home_team: Team::min(2, "home"),
            game_type: GameType::Exhibition,
            innings,
            away_points: 0,
            home_points: 0,
            player_entries: Vec::new(),
            player_battings: Vec::new(),
            player_runnings: Vec::new(),
        }
    }

    fn inning(seq: u8, tb: TB, count_seqs: &[u16]) -> Inning {
        Inning {
            seq,
            tb,
            counts: count_seqs
                .iter()
                .map(|seq| Count {
                    seq: *seq,
                    point: 0,
                    ball: 0,
                    strike: 0,
                    out: 0,
                })
                .collect(),
        }
    }

    #[test]
    fn next_moves_to_first_game_wide_count_seq_in_next_inning() {
        let mut cursor = GameCursor::new(game_detail(vec![
            inning(1, TB::Top, &[1, 2]),
            inning(1, TB::Bottom, &[3, 4]),
        ]));
        cursor.count_seq = 2;

        cursor.next();

        assert_eq!(cursor.inning_seq, 1);
        assert_eq!(cursor.inning_tb, TB::Bottom);
        assert_eq!(cursor.count_seq, 3);
    }

    #[test]
    fn prev_moves_to_last_game_wide_count_seq_in_previous_inning() {
        let mut cursor = GameCursor::new(game_detail(vec![
            inning(1, TB::Top, &[1, 2]),
            inning(1, TB::Bottom, &[3, 4]),
        ]));
        cursor.inning_tb = TB::Bottom;
        cursor.count_seq = 3;

        cursor.prev();

        assert_eq!(cursor.inning_seq, 1);
        assert_eq!(cursor.inning_tb, TB::Top);
        assert_eq!(cursor.count_seq, 2);
    }

    #[test]
    fn next_stays_on_last_game_wide_count_seq_in_current_inning() {
        let mut cursor = GameCursor::new(game_detail(vec![inning(1, TB::Bottom, &[3, 4])]));
        cursor.inning_tb = TB::Bottom;
        cursor.count_seq = 4;

        cursor.next();

        assert_eq!(cursor.inning_seq, 1);
        assert_eq!(cursor.inning_tb, TB::Bottom);
        assert_eq!(cursor.count_seq, 4);
    }

    #[test]
    fn next_does_not_enter_empty_skipped_bottom_inning() {
        let mut game = game_detail(vec![
            inning(9, TB::Top, &[17, 18]),
            inning(9, TB::Bottom, &[]),
        ]);
        game.away_points = 1;
        game.home_points = 2;
        let mut cursor = GameCursor::new(game);
        cursor.inning_seq = 9;
        cursor.count_seq = 18;

        cursor.next();

        assert_eq!(cursor.inning_seq, 9);
        assert_eq!(cursor.inning_tb, TB::Top);
        assert_eq!(cursor.count_seq, 18);
        assert!(cursor.is_last_bottom_inning_skiped);
    }
}
