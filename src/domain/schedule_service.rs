use super::shared::game::{Game, GameRound, GameSeason, GameType, TOTAL_GAMES};
use crate::domain::shared::team::League;
use crate::t;
use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate};
use std::collections::HashMap;
use std::collections::VecDeque;

const DEFAULT_DATE: &str = "19000101";

pub trait ScheduleRepository {
    fn load_game_season(&self) -> Result<GameSeason>;
    fn load_all_leagues(&self) -> Result<Vec<League>>;
    fn save_scheduled_game_rounds(&mut self, game_rounds: Vec<GameRound>) -> Result<()>;
    fn load_last_game_round_id(&self) -> Result<i32>;
    fn load_last_game_id(&self) -> Result<i32>;
    fn update_scheduled_season(&self, scheduled_season: i16) -> Result<()>;
}

pub struct ScheduleService<R: ScheduleRepository> {
    pub repo: R,
}

impl<R: ScheduleRepository> ScheduleService<R> {
    pub fn schedule_season(&mut self) -> Result<()> {
        // 1. load the scheduled game season
        let game_season = self
            .repo
            .load_game_season()
            .context(t!("error", "function" => "load_game_season"))?;
        let current_season = game_season.scheduled_season + 1;

        // 2. load all leagues to schedule
        let leagues = self
            .repo
            .load_all_leagues()
            .context(t!("error", "function" => "load_all_leagues"))?;

        // 3. Set max game round id
        let mut last_game_round_id = self
            .repo
            .load_last_game_round_id()
            .context(t!("error", "function" => "load_last_game_round_id"))?;

        // 3. Set max game id
        let mut last_game_id = self
            .repo
            .load_last_game_id()
            .context(t!("error", "function" => "load_last_game_id"))?;

        for league in leagues {
            let mut dequed_teams = VecDeque::from(league.teams.clone());
            let mut game_rounds = Vec::new();
            let mut current_date = game_season.start_date;

            // Initialize the map with team IDs as keys and 0 as the starting game count
            let mut games_played: HashMap<i16, i16> =
                league.teams.iter().map(|t| (t.id as i16, 0)).collect();

            // We need a reference key for the while loop condition
            let first_team_id = league.teams[0].id;
            let mut round_count = 0;

            while *games_played.get(&first_team_id).unwrap_or(&0) < TOTAL_GAMES {
                current_date = next_playable_date(current_date);

                let mut temp_teams = dequed_teams.clone();
                let mut pairings = Vec::new();
                let fixed = temp_teams.pop_front().unwrap();
                let n = temp_teams.len(); // this value must be 5

                pairings.push((fixed, temp_teams[n / 2].clone())); // match the center of the temp_teams and the fixed team
                for i in 0..(n / 2) {
                    pairings.push((temp_teams[i].clone(), temp_teams[n - 1 - i].clone()));
                }

                last_game_round_id += 1;
                let mut game_round = GameRound {
                    id: last_game_round_id,
                    season: current_season,
                    seq: round_count + 1,
                    date: current_date,
                    games: Vec::new(),
                };

                // Set three-game series
                for day_offset in 0..3 {
                    let game_date = current_date + Duration::days(day_offset as i64);

                    for (t_a, t_b) in &pairings {
                        // roll over home and away by the round_count
                        let (home, away) = if round_count % 2 == 0 {
                            (t_a, t_b)
                        } else {
                            (t_b, t_a)
                        };

                        if *games_played.get(&home.id).unwrap_or(&0) < TOTAL_GAMES {
                            last_game_id += 1;
                            game_round.games.push(Game {
                                id: last_game_id,
                                planned_date: game_date,
                                actual_date: NaiveDate::parse_from_str(DEFAULT_DATE, "%Y%m%d")?,
                                home_team: home.clone(),
                                away_team: away.clone(),
                                game_type: GameType::Regular,
                                innings: Vec::new(),
                                away_point: 0,
                                home_point: 0,
                                away_players: Vec::new(),
                                home_players: Vec::new(),
                            });
                        }
                    }
                }
                game_rounds.push(game_round);

                for (t_a, t_b) in &pairings {
                    if let Some(count) = games_played.get_mut(&(t_a.id)) {
                        *count += 3;
                    }
                    if let Some(count) = games_played.get_mut(&(t_b.id)) {
                        *count += 3;
                    }
                }

                // Change to 3 days after. (Move to the next card.)
                current_date += Duration::days(3);

                // Roll the teams (1st id fixed)
                let last = dequed_teams.pop_back().unwrap();
                dequed_teams.insert(1, last);
                round_count += 1;
            }

            self.repo
                .save_scheduled_game_rounds(game_rounds)
                .context(t!("error", "function" => "save_scheduled_game_rounds"))?;
            self.repo
                .update_scheduled_season(current_season)
                .context(t!("error", "function" => "update_scheduled_season"))?;
        }

        Ok(())
    }
}

fn next_playable_date(mut current_date: NaiveDate) -> NaiveDate {
    // Skip to Tuesday if Monday
    if current_date.weekday().number_from_monday() == 1 {
        current_date += Duration::days(1);
    }
    // TODO: Skip if Special Day
    current_date
}
