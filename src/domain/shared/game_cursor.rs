use super::game::{BattingResult, Count, GameDetail, Inning, TB};
use super::game_stats::{PlayerGameBattingView, PlayerGameRunningView};
use super::player::{Player, PlayerInfo, Position};
use super::team::Team;
use std::collections::HashMap;
use std::sync::Arc;

#[derive(thiserror::Error, Debug)]
pub enum GameViewError {
    #[error("No players for position: {0}")]
    NoPlayerFor(String),

    #[error("No players for base: {0}")]
    NoRunnerFor(String),

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

#[derive(Clone, Debug)]
pub struct BatterGameStatView {
    pub team_id: u16,
    pub batting_order: u8,
    pub position: Position,
    pub player: PlayerInfo,
    pub plate_appearances: u16,
    pub at_bats: u16,
    pub hits: u16,
    pub doubles: u16,
    pub triples: u16,
    pub home_runs: u16,
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

    pub fn away_team_id(&self) -> u16 {
        self.game.away_team.id
    }

    pub fn home_team_id(&self) -> u16 {
        self.game.home_team.id
    }

    pub fn away_team_name(&self) -> String {
        self.game.away_team.name.to_string()
    }

    pub fn home_team_name(&self) -> String {
        self.game.home_team.name.to_string()
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
        self.current_running_view()
            .and_then(|running| running.runner_1st)
            .is_some()
    }

    pub fn has_runner_on_second(&self) -> bool {
        self.current_running_view()
            .and_then(|running| running.runner_2nd)
            .is_some()
    }

    pub fn has_runner_on_third(&self) -> bool {
        self.current_running_view()
            .and_then(|running| running.runner_3rd)
            .is_some()
    }

    pub fn current_running_view(&self) -> Option<PlayerGameRunningView> {
        self.game
            .player_runnings
            .iter()
            .filter(|running| running.count_seq == self.count_seq)
            .max_by_key(|running| running.seq)
            .cloned()
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
        self.current_or_previous_batting_view()
            .and_then(|i| self.player_by_id(i.batter.id))
            .ok_or_else(|| GameViewError::NoPlayerFor("batter".to_string()))
    }

    pub fn current_batting_result(&self) -> Result<BattingResult, GameViewError> {
        self.current_batting_view()
            .map(|i| i.result)
            .ok_or(GameViewError::CurrentBattingResult)
    }

    pub fn current_batting_view(&self) -> Option<&PlayerGameBattingView> {
        self.game
            .player_battings
            .iter()
            .find(|i| i.is(self.count_seq))
    }

    fn current_or_previous_batting_view(&self) -> Option<&PlayerGameBattingView> {
        self.game
            .player_battings
            .iter()
            .filter(|i| i.count_seq <= self.count_seq)
            .max_by_key(|i| i.count_seq)
    }

    pub fn current_batting_stats(&self) -> Vec<PlayerGameBattingView> {
        self.game
            .player_battings
            .iter()
            .filter(|batting| batting.count_seq <= self.count_seq)
            .cloned()
            .collect()
    }

    pub fn current_batting_stats_for_team(&self, team_id: u16) -> Vec<BatterGameStatView> {
        let mut stats = self
            .game
            .player_entries
            .iter()
            .filter(|entry| entry.team_id == team_id)
            .filter(|entry| entry.start_count_seq == 1)
            .filter(|entry| entry.batting_order > 0)
            .map(|entry| {
                (
                    entry.player.id,
                    BatterGameStatView {
                        team_id,
                        batting_order: entry.batting_order,
                        position: entry.position,
                        player: entry.player.clone(),
                        plate_appearances: 0,
                        at_bats: 0,
                        hits: 0,
                        doubles: 0,
                        triples: 0,
                        home_runs: 0,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        for batting in self
            .game
            .player_battings
            .iter()
            .filter(|batting| batting.count_seq <= self.count_seq)
        {
            let Some(stat) = stats.get_mut(&batting.batter.id) else {
                continue;
            };

            match batting.result {
                BattingResult::Single => {
                    stat.plate_appearances += 1;
                    stat.at_bats += 1;
                    stat.hits += 1;
                }
                BattingResult::Double => {
                    stat.plate_appearances += 1;
                    stat.at_bats += 1;
                    stat.hits += 1;
                    stat.doubles += 1;
                }
                BattingResult::Triple => {
                    stat.plate_appearances += 1;
                    stat.at_bats += 1;
                    stat.hits += 1;
                    stat.triples += 1;
                }
                BattingResult::HomeRun => {
                    stat.plate_appearances += 1;
                    stat.at_bats += 1;
                    stat.hits += 1;
                    stat.home_runs += 1;
                }
                BattingResult::FieldersChoice | BattingResult::Out | BattingResult::DoublePlay => {
                    stat.plate_appearances += 1;
                    stat.at_bats += 1;
                }
                BattingResult::Foul => {}
            }
        }

        let mut rows = stats.into_values().collect::<Vec<_>>();
        rows.sort_by_key(|row| row.batting_order);
        rows
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
    use crate::domain::shared::ball::{BattedBall, TrajectoryType};
    use crate::domain::shared::game::GameType;
    use crate::domain::shared::game_stats::PlayerGameEntryView;
    use crate::domain::shared::player::PlayerInfo;
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

    fn player(id: i64) -> Player {
        Player::from_player_info(PlayerInfo::new_min(
            id,
            format!("First{id}"),
            format!("Last{id}"),
        ))
    }

    fn player_entry(team_id: u16, batting_order: u8, player_id: i64) -> PlayerGameEntryView {
        PlayerGameEntryView {
            start_count_seq: 1,
            end_count_seq: 3,
            team_id,
            position: Position::DH,
            batting_order,
            player: PlayerInfo::new_min(
                player_id,
                format!("First{player_id}"),
                format!("Last{player_id}"),
            ),
        }
    }

    fn batting_view(count_seq: u16, batter_id: i64) -> PlayerGameBattingView {
        PlayerGameBattingView {
            count_seq,
            pitcher: PlayerInfo::new_min(1, "Pitcher".to_string(), "One".to_string()),
            batter: PlayerInfo::new_min(
                batter_id,
                format!("First{batter_id}"),
                format!("Last{batter_id}"),
            ),
            ball: BattedBall::new(100.0, 20.0, 30.0, 80.0, 3.0, TrajectoryType::Fly),
            fielder_position: None,
            result: BattingResult::Single,
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

    #[test]
    fn current_batter_uses_exact_batting_view_when_available() {
        let mut game = game_detail(vec![inning(1, TB::Top, &[1, 2, 3])]);
        game.away_team.players = vec![player(10), player(11)];
        game.player_battings = vec![batting_view(1, 10), batting_view(3, 11)];
        let mut cursor = GameCursor::new(game);
        cursor.count_seq = 3;

        let batter = cursor.current_batter().unwrap();

        assert_eq!(batter.info.id, 11);
        assert_eq!(cursor.current_batting_view().unwrap().count_seq, 3);
    }

    #[test]
    fn current_batter_falls_back_to_most_recent_batting_view() {
        let mut game = game_detail(vec![inning(1, TB::Top, &[1, 2])]);
        game.away_team.players = vec![player(10)];
        game.player_battings = vec![batting_view(1, 10)];
        let mut cursor = GameCursor::new(game);
        cursor.count_seq = 2;

        let batter = cursor.current_batter().unwrap();

        assert_eq!(batter.info.id, 10);
        assert_eq!(
            cursor.current_batting_result().unwrap_err().to_string(),
            GameViewError::CurrentBattingResult.to_string()
        );
        assert!(cursor.current_batting_view().is_none());
    }

    #[test]
    fn current_batting_stats_returns_battings_up_to_current_count() {
        let mut game = game_detail(vec![inning(1, TB::Top, &[1, 2, 3])]);
        game.player_battings = vec![
            batting_view(1, 10),
            batting_view(2, 11),
            batting_view(3, 12),
        ];
        let mut cursor = GameCursor::new(game);
        cursor.count_seq = 2;

        let stats = cursor.current_batting_stats();

        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].batter.id, 10);
        assert_eq!(stats[1].batter.id, 11);
    }

    #[test]
    fn current_batting_stats_for_team_lists_opening_lineup_and_updates_by_count() {
        let mut game = game_detail(vec![inning(1, TB::Top, &[1, 2, 3])]);
        game.player_entries = vec![player_entry(1, 1, 10), player_entry(1, 2, 11)];
        game.player_battings = vec![batting_view(1, 10), batting_view(2, 11)];
        let mut cursor = GameCursor::new(game);
        cursor.count_seq = 1;

        let stats = cursor.current_batting_stats_for_team(1);

        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].player.id, 10);
        assert_eq!(stats[0].position, Position::DH);
        assert_eq!(stats[0].plate_appearances, 1);
        assert_eq!(stats[0].hits, 1);
        assert_eq!(stats[1].player.id, 11);
        assert_eq!(stats[1].plate_appearances, 0);
        assert_eq!(stats[1].hits, 0);

        cursor.count_seq = 2;
        let stats = cursor.current_batting_stats_for_team(1);

        assert_eq!(stats[1].plate_appearances, 1);
        assert_eq!(stats[1].hits, 1);
    }
}
