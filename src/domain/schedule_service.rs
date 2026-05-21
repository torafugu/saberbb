use super::shared::game::{GameScheduler, GameType, TOTAL_GAMES};
use crate::repositories::schedule_repository::ScheduleRepository;
use crate::t;
use anyhow::{Context, Result};
use chrono::{Datelike, Duration, NaiveDate};
use std::collections::HashMap;
use std::collections::VecDeque;

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

        // 2. load all leagues to schedule
        let leagues = self
            .repo
            .load_all_leagues()
            .context(t!("error", "function" => "load_all_leagues"))?;

        for league in leagues {
            let mut dequed_teams = VecDeque::from(league.teams.clone());
            // let mut game_rounds = Vec::new();
            let mut current_date = game_season.start_date;

            // Initialize the map with team IDs as keys and 0 as the starting game count
            let mut games_played: HashMap<u16, u16> =
                league.teams.iter().map(|t| (t.id as u16, 0)).collect();

            // We need a reference key for the while loop condition
            let first_team_id = league.teams[0].id;
            let mut round_seq = 1;
            let mut game_schedules = Vec::new();

            while *games_played.get(&first_team_id).unwrap_or(&0) < TOTAL_GAMES {
                current_date = next_playable_date(current_date);

                let mut temp_teams = dequed_teams.clone();
                let mut pairings = Vec::new();
                let fixed = temp_teams.pop_front().unwrap();
                let n = temp_teams.len(); // this value must be 5
                // let round_seq = round_count + 1;

                pairings.push((fixed, temp_teams[n / 2].clone())); // match the center of the temp_teams and the fixed team
                for i in 0..(n / 2) {
                    pairings.push((temp_teams[i].clone(), temp_teams[n - 1 - i].clone()));
                }

                // Set three-game series
                for day_offset in 0..3 {
                    let game_date = current_date + Duration::days(day_offset as i64);

                    for (t_a, t_b) in &pairings {
                        // roll over home and away by the round_count
                        let (home, away) = if round_seq % 2 == 0 {
                            (t_a, t_b)
                        } else {
                            (t_b, t_a)
                        };

                        if *games_played.get(&home.id).unwrap_or(&0) < TOTAL_GAMES {
                            // last_game_id += 1;
                            game_schedules.push(GameScheduler {
                                id: 0, // Dummy
                                season: game_season.season,
                                round_seq: round_seq,
                                seq: day_offset + 1,
                                planned_date: game_date,
                                home_team: home.clone(),
                                away_team: away.clone(),
                                game_type: GameType::Regular,
                            });
                        }
                    }
                }
                // game_rounds.push(game_round);

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
                round_seq += 1;
            }

            self.repo
                .save_game_schedules(game_schedules)
                .context(t!("error", "function" => "save_scheduled_game_rounds"))?;
        }
        self.repo
            .update_scheduled_season()
            .context(t!("error", "function" => "update_scheduled_season"))?;

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
