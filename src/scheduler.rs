use crate::shared::{game::GameRound, game::GameSchedule, game::GameType, team::League};

use super::shared::game::TOTAL_GAMES;
use chrono::{Datelike, Duration, NaiveDate};
use std::collections::HashMap;
use std::collections::VecDeque;

pub fn schedule_season(season: i16, start_date: NaiveDate, league: &League) -> Vec<GameRound> {
    let mut dequed_teams = VecDeque::from(league.teams.clone());
    let mut game_rounds = Vec::new();
    let mut current_date = start_date;

    // let mut games_played = [0; 6];
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

        // Set three-game series
        for day_offset in 0..3 {
            let match_date = current_date + Duration::days(day_offset as i64);
            let mut game_round = GameRound {
                season: season,
                seq: round_count + 1,
                date: match_date,
                game_schedules: Vec::new(),
            };

            for (t_a, t_b) in &pairings {
                // roll over home and away by the round_count
                let (home, away) = if round_count % 2 == 0 {
                    (t_a, t_b)
                } else {
                    (t_b, t_a)
                };

                // if games_played[home.id as usize] < TOTAL_GAMES {
                if *games_played.get(&home.id).unwrap_or(&0) < TOTAL_GAMES {
                    game_round.game_schedules.push(GameSchedule {
                        seq: day_offset + 1,
                        home_team: home.clone(),
                        away_team: away.clone(),
                        game_type: GameType::REGULAR,
                    });
                }
            }
            game_rounds.push(game_round);
        }

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

    game_rounds
}
