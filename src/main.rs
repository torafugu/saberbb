mod presenter;
mod repository;
mod resolver;
mod scheduler;
mod shared;

use clap::Parser;
use presenter::{
    display_batting_results, display_game_processed, display_game_result, display_game_scheduled,
};
use repository::constatns_repository::{ERROR_LOAD_GAME_SEASON, get_game_season};
use repository::game_repository::ERROR_LOAD_GAME;
use repository::game_repository::{ERROR_SAVE_GAME, load_game, save_game};
use repository::schedule_repository::{ERROR_SAVE_GAME_ROUNDS, save_game_rounds};
use repository::team_repository::{ERROR_LOAD_ALL_LEAGUE, load_all_leagus};
use resolver::batting_resolve;
use scheduler::schedule_season;
use shared::game::Count;
use shared::game::Game;
use shared::game::Inning;
use shared::game::MAX_INNING;
use shared::game::MAX_OUT;
use shared::player::Batter;
use shared::team::{League, Team};
use shared::types::{BattingResult, InningType};
use shared::utils::next_tb;
use std::sync::Arc;

fn main() {
    let args = Args::parse();

    // Game Display Mode
    if let Some(p) = args.display {
        display(p);
    } else {
        println!("No games diplayed.");
    }

    // Game Process Mode
    if let Some(p) = args.process {
        process(p);
    } else {
        println!("No games processed.");
    }

    // Game Schedule Generate Mode
    if let Some(p) = args.schedule {
        schedule(p);
    } else {
        println!("No games scheduled.");
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

fn display(seq: i32) {
    let load_game_res: Result<Game, _> = load_game(seq);
    match load_game_res {
        Ok(game) => {
            display_game_result(&game);
            display_batting_results(&game);
        }
        Err(e) => {
            eprintln!("{}:{}", ERROR_LOAD_GAME, e);
        }
    }
}
fn schedule(_num_of_season: i8) {
    let game_seaon_res = get_game_season();
    match game_seaon_res {
        Ok(game_seaon) => {
            let leagues_res: Result<Vec<League>, _> = load_all_leagus();
            match leagues_res {
                Ok(leagues) => {
                    for league in leagues {
                        let rounds =
                            schedule_season(game_seaon.season, game_seaon.start_date, &league);
                        if let Err(e) = save_game_rounds(rounds) {
                            eprintln!("{}:{}", ERROR_SAVE_GAME_ROUNDS, e);
                        }
                    }
                }
                Err(e) => {
                    eprintln!("{}:{}", ERROR_LOAD_ALL_LEAGUE, e);
                }
            }
            display_game_scheduled(game_seaon.season);
        }
        Err(e) => {
            eprintln!("{}:{}", ERROR_LOAD_GAME_SEASON, e);
        }
    }
}

fn process(num_of_games: i8) {
    let mut _is_in_game: bool = true;
    let mut _inning_seq: i8 = 1;
    let mut _inning_tb: InningType = InningType::TOP;
    let mut _top_total_score: i8 = 0;
    let mut _bottom_total_score: i8 = 0;
    let mut _top_batter_order: usize = 1;
    let mut _bottom_batter_order: usize = 1;

    let mut _game: Game = Game {
        seq: 1,
        top_team: Team::new(1, "AAA"),
        bottom_team: Team::new(2, "BBB"),
        innings: Vec::new(),
        top_batters: Vec::from([
            Batter::new(1, "Top batter 1", 1.0, -0.5),
            Batter::new(2, "Top batter 2", 1.2, -0.8),
            Batter::new(3, "Top batter 3", 1.4, 0.8),
            Batter::new(4, "Top batter 4", 1.6, 1.0),
            Batter::new(5, "Top batter 5", 1.5, 0.9),
            Batter::new(6, "Top batter 6", -0.1, 0.2),
            Batter::new(7, "Top batter 7", 0.1, -0.3),
            Batter::new(8, "Top batter 8", -1.0, -0.5),
            Batter::new(9, "Top batter 9", -1.2, -1.2),
        ]),
        bottom_batters: Vec::from([
            Batter::new(10, "Bottom batter 1", 0.9, -0.8),
            Batter::new(11, "Bottom batter 2", 1.1, -0.6),
            Batter::new(12, "Bottom batter 3", 1.2, 1.0),
            Batter::new(13, "Bottom batter 4", 1.4, 1.4),
            Batter::new(14, "Bottom batter 5", 0.2, 1.1),
            Batter::new(15, "Bottom batter 6", -0.5, -0.2),
            Batter::new(16, "Bottom batter 7", -0.8, -0.1),
            Batter::new(17, "Bottom batter 8", -1.3, -0.3),
            Batter::new(18, "Bottom batter 9", -1.4, -0.4),
        ]),
    };

    // loop for an innning
    while _is_in_game {
        let mut _inning: Inning = Inning {
            tb: _inning_tb,
            seq: _inning_seq,
            counts: Vec::new(),
            point: 0,
        };
        let mut _count_seq = 0;
        let mut _is_first_runner = false;
        let mut _is_second_runner = false;
        let mut _is_third_runner = false;
        let mut _out_count = 0;

        while _out_count < MAX_OUT {
            _count_seq += 1;

            let _current_batter: &Batter;
            if _inning_tb == InningType::TOP {
                _current_batter = &_game.top_batters[_top_batter_order - 1];
            } else {
                _current_batter = &_game.bottom_batters[_bottom_batter_order - 1];
            }

            let mut _count = Count {
                seq: _count_seq,
                is_first_runner: _is_first_runner,
                is_second_runner: _is_second_runner,
                is_third_runner: _is_third_runner,
                batter: Arc::new(_current_batter.clone()),
                result: BattingResult::OUT,
                point: 0,
                out: _out_count,
            };

            // Batting result calculation
            _count.result = batting_resolve(_count.batter.clone());

            match _count.result {
                BattingResult::SINGLE => {
                    if _is_third_runner {
                        _count.point += 1;
                        _inning.point += 1;
                        _is_third_runner = false;
                    }
                    if _is_second_runner {
                        _is_second_runner = false;
                        _is_third_runner = true;
                    }
                    if _is_first_runner {
                        _is_second_runner = true;
                    }
                    _is_first_runner = true;
                }
                BattingResult::DOUBLE => {
                    if _is_third_runner {
                        _count.point += 1;
                        _inning.point += 1;
                        _is_third_runner = false;
                    }
                    if _is_second_runner {
                        _count.point += 1;
                        _inning.point += 1;
                    }
                    if _is_first_runner {
                        _is_first_runner = false;
                        _is_third_runner = true;
                    }
                    _is_second_runner = true;
                }
                BattingResult::TRIPLE => {
                    if _is_third_runner {
                        _count.point += 1;
                        _inning.point += 1;
                    }
                    if _is_second_runner {
                        _count.point += 1;
                        _inning.point += 1;
                        _is_second_runner = false;
                    }
                    if _is_first_runner {
                        _count.point += 1;
                        _inning.point += 1;
                        _is_first_runner = false;
                    }
                    _is_third_runner = true;
                }
                BattingResult::HOMERUN => {
                    if _is_third_runner {
                        _count.point += 1;
                        _inning.point += 1;
                        _is_third_runner = false;
                    }
                    if _is_second_runner {
                        _count.point += 1;
                        _inning.point += 1;
                        _is_second_runner = false;
                    }
                    if _is_first_runner {
                        _count.point += 1;
                        _inning.point += 1;
                        _is_first_runner = false;
                    }
                    _count.point += 1;
                    _inning.point += 1;
                }
                _ => {
                    _count.result = BattingResult::OUT;
                    if _out_count < MAX_OUT {
                        _out_count += 1;
                    }
                }
            }

            if _inning_tb == InningType::TOP {
                _top_total_score += _count.point;
                if _top_batter_order == 9 {
                    _top_batter_order = 1;
                } else {
                    _top_batter_order += 1;
                }
            } else {
                _bottom_total_score += _count.point;
                if _bottom_batter_order == 9 {
                    _bottom_batter_order = 1;
                } else {
                    _bottom_batter_order += 1;
                }
            }
            _count.is_first_runner = _is_first_runner;
            _count.is_second_runner = _is_second_runner;
            _count.is_third_runner = _is_third_runner;
            _count.out = _out_count;
            _inning.counts.push(_count);

            // Check walk-off
            if _inning_seq == MAX_INNING
                && _inning_tb == InningType::BOTTOM
                && _bottom_total_score > _top_total_score
            {
                _is_in_game = false;
                break;
            }
        }

        _game.add_inning(_inning);

        // Check Game-Set
        if _inning_seq == MAX_INNING {
            if _inning_tb == InningType::BOTTOM {
                break;
            } else if _bottom_total_score > _top_total_score {
                break;
            }
        } else {
            if _inning_tb == InningType::BOTTOM {
                _inning_seq += 1;
            }
        }
        _inning_tb = next_tb(_inning_tb);
    }

    if let Err(e) = save_game(&_game) {
        eprintln!("{}:{}", ERROR_SAVE_GAME, e);
    }
    display_game_processed(num_of_games);
}
