use super::domains::game::TOTAL_GAMES;
use crate::domains::{game::Game, game::GameRound, game::GameType};
use crate::repositories::game_repository::{load_game_season, update_scheduled_season};
use crate::repositories::schedule_repository::save_scheduled_game_rounds;
use crate::repositories::team_repository::load_all_leagues;
use anyhow::{Context, Result};
use chrono::{Datelike, Duration};
use std::collections::HashMap;
use std::collections::VecDeque;

pub const ERROR_LOAD_ALL_LEAGUE: &str = "An error occurred in load_all_leagus()";
pub const ERROR_LOAD_GAME_SEASON: &str = "An error occurred in load_game_season()";
pub const ERROR_SAVE_SCHEDULED_GAME_ROUNDS: &str =
    "An error occurred in save_scheduled_game_rounds()";
pub const ERROR_UPDATE_SCHEDULED_SEASON: &str = "An error occurred in update_scheduled_season()";

pub fn schedule_season() -> Result<()> {
    // 1. load the scheduled game season
    let game_season = load_game_season().context(ERROR_LOAD_GAME_SEASON)?;
    let current_season = game_season.scheduled_season + 1;

    // 2. load all leagues to schedule
    let leagues = load_all_leagues().context(ERROR_LOAD_ALL_LEAGUE)?;

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
            // Skip to Tuesday if Monday
            if current_date.weekday().number_from_monday() == 1 {
                current_date += Duration::days(1);
            }

            let mut temp_teams = dequed_teams.clone();
            let mut pairings = Vec::new();
            let fixed = temp_teams.pop_front().unwrap();
            let n = temp_teams.len(); // this value mudst be 5

            pairings.push((fixed, temp_teams[n / 2].clone())); // match the center of the temp_teams and the fixed team
            for i in 0..(n / 2) {
                pairings.push((temp_teams[i].clone(), temp_teams[n - 1 - i].clone()));
            }

            let mut game_round = GameRound {
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
                        game_round.games.push(Game {
                            seq: day_offset + 1,
                            date: game_date,
                            home_team: home.clone(),
                            away_team: away.clone(),
                            game_type: GameType::REGULAR,
                            innings: Vec::new(),
                            away_batters: Vec::new(),
                            home_batters: Vec::new(),
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

        save_scheduled_game_rounds(game_rounds).context(ERROR_SAVE_SCHEDULED_GAME_ROUNDS)?;
        update_scheduled_season(current_season).context(ERROR_UPDATE_SCHEDULED_SEASON)?;
    }

    Ok(())
}
