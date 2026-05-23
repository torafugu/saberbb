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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::game::GameSeason;
    use crate::domain::shared::team::{League, Team};
    use anyhow::anyhow;
    use std::cell::{Cell, RefCell};

    struct RecordingRepo {
        game_season: GameSeason,
        leagues: Vec<League>,
        load_game_season_error: bool,
        load_all_leagues_error: bool,
        save_error_at: Option<usize>,
        update_error: bool,
        load_game_season_calls: Cell<usize>,
        load_all_leagues_calls: Cell<usize>,
        update_calls: Cell<usize>,
        saved_batches: Vec<Vec<GameScheduler>>,
        call_log: RefCell<Vec<&'static str>>,
    }

    impl RecordingRepo {
        fn new(leagues: Vec<League>) -> Self {
            Self {
                game_season: GameSeason {
                    start_date: NaiveDate::from_ymd_opt(2026, 4, 1).unwrap(),
                    season: 2026,
                },
                leagues,
                load_game_season_error: false,
                load_all_leagues_error: false,
                save_error_at: None,
                update_error: false,
                load_game_season_calls: Cell::new(0),
                load_all_leagues_calls: Cell::new(0),
                update_calls: Cell::new(0),
                saved_batches: Vec::new(),
                call_log: RefCell::new(Vec::new()),
            }
        }
    }

    impl ScheduleRepository for RecordingRepo {
        fn load_game_season(&self) -> Result<GameSeason> {
            self.call_log.borrow_mut().push("load_game_season");
            self.load_game_season_calls
                .set(self.load_game_season_calls.get() + 1);

            if self.load_game_season_error {
                return Err(anyhow!("load game season failed"));
            }

            Ok(GameSeason {
                start_date: self.game_season.start_date,
                season: self.game_season.season,
            })
        }

        fn load_all_leagues(&self) -> Result<Vec<League>> {
            self.call_log.borrow_mut().push("load_all_leagues");
            self.load_all_leagues_calls
                .set(self.load_all_leagues_calls.get() + 1);

            if self.load_all_leagues_error {
                return Err(anyhow!("load all leagues failed"));
            }

            Ok(self.leagues.clone())
        }

        fn save_game_schedules(&mut self, game_schedules: Vec<GameScheduler>) -> Result<()> {
            self.call_log.borrow_mut().push("save_game_schedules");
            let call_index = self.saved_batches.len();

            if self.save_error_at == Some(call_index) {
                return Err(anyhow!("save game schedules failed"));
            }

            self.saved_batches.push(game_schedules);
            Ok(())
        }

        fn update_scheduled_season(&self) -> Result<()> {
            self.call_log.borrow_mut().push("update_scheduled_season");
            self.update_calls.set(self.update_calls.get() + 1);

            if self.update_error {
                return Err(anyhow!("update scheduled season failed"));
            }

            Ok(())
        }
    }

    fn team(id: u16) -> Team {
        Team::min(id, &format!("Team{id}"))
    }

    fn league(id: u16, first_team_id: u16) -> League {
        League {
            id,
            name: format!("League{id}").into(),
            teams: (first_team_id..first_team_id + 6).map(team).collect(),
        }
    }

    fn team_game_count(schedules: &[GameScheduler], team_id: u16) -> usize {
        schedules
            .iter()
            .filter(|schedule| schedule.home_team.id == team_id || schedule.away_team.id == team_id)
            .count()
    }

    #[test]
    fn schedule_season_loads_game_season_and_leagues() {
        let mut service = ScheduleService {
            repo: RecordingRepo::new(vec![league(1, 1)]),
        };

        let result = service.schedule_season();

        assert!(result.is_ok());
        assert_eq!(service.repo.load_game_season_calls.get(), 1);
        assert_eq!(service.repo.load_all_leagues_calls.get(), 1);
        assert_eq!(service.repo.saved_batches.len(), 1);
        assert_eq!(service.repo.update_calls.get(), 1);
    }

    #[test]
    fn schedule_season_saves_regular_games_for_loaded_season() {
        let mut service = ScheduleService {
            repo: RecordingRepo::new(vec![league(1, 1)]),
        };

        service.schedule_season().unwrap();

        let schedules = &service.repo.saved_batches[0];
        assert!(!schedules.is_empty());
        for schedule in schedules {
            assert_eq!(schedule.id, 0);
            assert_eq!(schedule.season, 2026);
            assert!(matches!(schedule.game_type, GameType::Regular));
        }
    }

    #[test]
    fn schedule_season_reaches_required_games_per_team() {
        let mut service = ScheduleService {
            repo: RecordingRepo::new(vec![league(1, 1)]),
        };

        service.schedule_season().unwrap();

        let schedules = &service.repo.saved_batches[0];
        for team_id in 1..=6 {
            let count = team_game_count(schedules, team_id);
            assert!(count >= TOTAL_GAMES as usize);
            assert!(count <= TOTAL_GAMES as usize + 2);
        }
    }

    #[test]
    fn schedule_season_generates_three_game_series() {
        let mut service = ScheduleService {
            repo: RecordingRepo::new(vec![league(1, 1)]),
        };

        service.schedule_season().unwrap();

        let first_round: Vec<_> = service.repo.saved_batches[0]
            .iter()
            .filter(|schedule| schedule.round_seq == 1)
            .collect();
        assert_eq!(first_round.len(), 9);
        assert_eq!(
            first_round
                .iter()
                .map(|schedule| schedule.seq)
                .collect::<Vec<_>>(),
            vec![1, 1, 1, 2, 2, 2, 3, 3, 3]
        );
    }

    #[test]
    fn schedule_season_skips_monday_start_date() {
        let mut repo = RecordingRepo::new(vec![league(1, 1)]);
        repo.game_season.start_date = NaiveDate::from_ymd_opt(2026, 4, 6).unwrap();
        let mut service = ScheduleService { repo };

        service.schedule_season().unwrap();

        assert_eq!(
            service.repo.saved_batches[0][0].planned_date,
            NaiveDate::from_ymd_opt(2026, 4, 7).unwrap()
        );
    }

    #[test]
    fn schedule_season_saves_once_for_each_league() {
        let mut service = ScheduleService {
            repo: RecordingRepo::new(vec![league(1, 1), league(2, 101)]),
        };

        service.schedule_season().unwrap();

        assert_eq!(service.repo.saved_batches.len(), 2);
        assert!(
            service.repo.saved_batches[0]
                .iter()
                .all(|schedule| schedule.home_team.id < 100 && schedule.away_team.id < 100)
        );
        assert!(
            service.repo.saved_batches[1]
                .iter()
                .all(|schedule| schedule.home_team.id >= 100 && schedule.away_team.id >= 100)
        );
    }

    #[test]
    fn schedule_season_updates_scheduled_season_after_saving() {
        let mut service = ScheduleService {
            repo: RecordingRepo::new(vec![league(1, 1)]),
        };

        service.schedule_season().unwrap();

        assert_eq!(
            *service.repo.call_log.borrow(),
            vec![
                "load_game_season",
                "load_all_leagues",
                "save_game_schedules",
                "update_scheduled_season"
            ]
        );
    }

    #[test]
    fn schedule_season_returns_error_when_load_game_season_fails() {
        let mut repo = RecordingRepo::new(vec![league(1, 1)]);
        repo.load_game_season_error = true;
        let mut service = ScheduleService { repo };

        let result = service.schedule_season();

        assert!(result.is_err());
        assert_eq!(service.repo.load_game_season_calls.get(), 1);
        assert_eq!(service.repo.load_all_leagues_calls.get(), 0);
        assert!(service.repo.saved_batches.is_empty());
        assert_eq!(service.repo.update_calls.get(), 0);
    }

    #[test]
    fn schedule_season_returns_error_when_load_all_leagues_fails() {
        let mut repo = RecordingRepo::new(vec![league(1, 1)]);
        repo.load_all_leagues_error = true;
        let mut service = ScheduleService { repo };

        let result = service.schedule_season();

        assert!(result.is_err());
        assert_eq!(service.repo.load_game_season_calls.get(), 1);
        assert_eq!(service.repo.load_all_leagues_calls.get(), 1);
        assert!(service.repo.saved_batches.is_empty());
        assert_eq!(service.repo.update_calls.get(), 0);
    }

    #[test]
    fn schedule_season_returns_error_when_save_game_schedules_fails() {
        let mut repo = RecordingRepo::new(vec![league(1, 1), league(2, 101)]);
        repo.save_error_at = Some(1);
        let mut service = ScheduleService { repo };

        let result = service.schedule_season();

        assert!(result.is_err());
        assert_eq!(service.repo.saved_batches.len(), 1);
        assert_eq!(service.repo.update_calls.get(), 0);
    }

    #[test]
    fn schedule_season_returns_error_when_update_scheduled_season_fails() {
        let mut repo = RecordingRepo::new(vec![league(1, 1)]);
        repo.update_error = true;
        let mut service = ScheduleService { repo };

        let result = service.schedule_season();

        assert!(result.is_err());
        assert_eq!(service.repo.saved_batches.len(), 1);
        assert_eq!(service.repo.update_calls.get(), 1);
    }

    #[test]
    fn schedule_season_with_no_leagues_only_updates_scheduled_season() {
        let mut service = ScheduleService {
            repo: RecordingRepo::new(Vec::new()),
        };

        let result = service.schedule_season();

        assert!(result.is_ok());
        assert!(service.repo.saved_batches.is_empty());
        assert_eq!(service.repo.update_calls.get(), 1);
    }

    #[test]
    #[should_panic]
    fn schedule_season_panics_for_empty_league() {
        let empty_league = League {
            id: 1,
            name: "Empty".into(),
            teams: Vec::new(),
        };
        let mut service = ScheduleService {
            repo: RecordingRepo::new(vec![empty_league]),
        };

        service.schedule_season().unwrap();
    }

    #[test]
    fn next_playable_date_keeps_non_monday_date() {
        assert_eq!(
            next_playable_date(NaiveDate::from_ymd_opt(2026, 4, 7).unwrap()),
            NaiveDate::from_ymd_opt(2026, 4, 7).unwrap()
        );
        assert_eq!(
            next_playable_date(NaiveDate::from_ymd_opt(2026, 4, 12).unwrap()),
            NaiveDate::from_ymd_opt(2026, 4, 12).unwrap()
        );
    }

    #[test]
    fn next_playable_date_moves_monday_to_tuesday() {
        assert_eq!(
            next_playable_date(NaiveDate::from_ymd_opt(2026, 4, 6).unwrap()),
            NaiveDate::from_ymd_opt(2026, 4, 7).unwrap()
        );
    }
}
