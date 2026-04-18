use chrono::{Datelike, Duration, NaiveDate};
use std::collections::VecDeque;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum STeam {
    T1,
    T2,
    T3,
    T4,
    T5,
    T6,
}

#[derive(Debug)]
pub struct SGame {
    pub date: NaiveDate,
    pub home: STeam,
    pub away: STeam,
}

pub fn generate_schedule(start_date: NaiveDate, total_games_per_team: usize) -> Vec<SGame> {
    let mut teams = VecDeque::from(vec![
        STeam::T1,
        STeam::T2,
        STeam::T3,
        STeam::T4,
        STeam::T5,
        STeam::T6,
    ]);
    let mut schedule = Vec::new();
    let mut current_date = start_date;

    let mut games_played = [0usize; 6];
    let mut round_count = 0;

    while games_played[0] < total_games_per_team {
        // Skip to Tuesday if Monday
        if current_date.weekday().number_from_monday() == 1 {
            current_date += Duration::days(1);
        }

        let mut temp_teams = teams.clone();
        let mut pairings = Vec::new();
        let fixed = temp_teams.pop_front().unwrap();
        let n = temp_teams.len(); // this value mudst be 5

        pairings.push((fixed, temp_teams[n / 2])); // match the center of the temp_teams and the fixed team
        for i in 0..(n / 2) {
            pairings.push((temp_teams[i], temp_teams[n - 1 - i]));
        }

        // Set three-game series
        for day_offset in 0..3 {
            let match_date = current_date + Duration::days(day_offset as i64);

            for (t_a, t_b) in &pairings {
                // roll over home and away by the round_count
                let (home, away) = if round_count % 2 == 0 {
                    (*t_a, *t_b)
                } else {
                    (*t_b, *t_a)
                };

                if games_played[home as usize] < total_games_per_team {
                    schedule.push(SGame {
                        date: match_date,
                        home,
                        away,
                    });
                }
            }
        }

        // update the counter
        for (t_a, t_b) in &pairings {
            games_played[*t_a as usize] += 3;
            games_played[*t_b as usize] += 3;
        }

        // Change to 3 days after. (Move to the next card.)
        current_date += Duration::days(3);

        // Roll the teams (1st id fixed)
        let last = teams.pop_back().unwrap();
        teams.insert(1, last);
        round_count += 1;
    }

    schedule
}
