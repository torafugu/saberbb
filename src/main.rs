mod adapters;
mod domains;
mod game_engine;
mod repositories;
mod resolver;
mod scheduler;

use adapters::game_presenter::{
    display_batting_results, display_game_result, display_no_games, display_no_round_processed,
    display_rounds_processed,
};
use adapters::schedule_presenter::{
    display_game_seasons_scheduled, display_no_game_season_scheduled,
};
use clap::Parser;
use game_engine::process_game;
use repositories::game_repository::load_last_games;
use scheduler::schedule_season;

pub const ERROR_SCHEDULED_SEASON: &str = "An error occurred in schedule_season()";
pub const ERROR_PROCESS_GAME: &str = "An error occurred in process_game()";
pub const ERROR_LOAD_LAST_GAME: &str = "An error occurred in load_last_games()";

fn main() {
    let args = Args::parse();

    // Game Display Mode
    if let Some(_p) = args.display {
        let load_games_res = load_last_games();
        match load_games_res {
            Ok(games) => {
                for game in games.iter() {
                    display_game_result(&game);
                    display_batting_results(&game);
                }
            }
            Err(e) => {
                eprintln!("{}:{}", ERROR_LOAD_LAST_GAME, e);
            }
        }
    } else {
        display_no_games();
    }

    // Game Process Mode
    if let Some(num_of_rounds) = args.process {
        for _ in 0..num_of_rounds {
            if let Err(e) = process_game() {
                eprintln!("{}:{}", ERROR_PROCESS_GAME, e);
            }
        }
        display_rounds_processed(num_of_rounds);
    } else {
        display_no_round_processed();
    }

    // Game Schedule Generate Mode
    if let Some(num_of_schedules) = args.schedule {
        for _ in 0..num_of_schedules {
            if let Err(e) = schedule_season() {
                eprintln!("{}:{}", ERROR_SCHEDULED_SEASON, e);
            }
        }
        display_game_seasons_scheduled(num_of_schedules);
    } else {
        display_no_game_season_scheduled();
    }
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Proess games
    #[arg(short, long)]
    process: Option<i8>,

    /// Display game result
    #[arg(short, long)]
    display: Option<i32>,

    /// Schedule games
    #[arg(short, long)]
    schedule: Option<i8>,
}
