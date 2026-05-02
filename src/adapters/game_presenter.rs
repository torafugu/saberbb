use super::menu_component::MenuItem;
use crate::domain::shared::game::Game;
use crate::domain::shared::types::InningType;
use crate::repositories::game_repository::{load_processed_games, load_processed_seasons};
use crate::t;
use inquire::Select;
use std::collections::BTreeMap;

const LINE_SEPARATOR_TEXT: &str = "---";
const RUNNER_TEXT: &str = "R";
const NO_RUNNER_TEXT: &str = "-";
const SPACE_TEXT: &str = " ";
const SEPARATOR_TEXT: &str = ":";
const WALK_OFF_TEXT: &str = "x";

pub fn display_game_rounds_processed(num_of_rounds: i8) {
    println!("{} rounds processed.", num_of_rounds);
}

pub fn display_select_game(season: i16) {
    let game_rounds_res = load_processed_games(season);
    match game_rounds_res {
        Ok(games) => {
            let menu_items: Vec<MenuItem<Game>> = games
                .into_iter()
                .map(
                    |Game {
                         seq,
                         date,
                         away_team,
                         home_team,
                         game_type,
                         innings,
                         away_batters,
                         home_batters,
                     }| {
                        let label =
                            format!("[{}] {} vs {})", date, away_team.name, home_team.name,);

                        MenuItem {
                            label,
                            value: Game {
                                seq,
                                date,
                                away_team,
                                home_team,
                                game_type,
                                innings,
                                away_batters,
                                home_batters,
                            },
                        }
                    },
                )
                .collect();

            let selection = Select::new(&t!("select_game"), menu_items).prompt();

            if let Ok(selected) = selection {
                display_game_result(&selected.value);
                display_batting_results(&selected.value);
            }
        }
        Err(e) => {
            eprintln!(
                "{}:{}",
                t!("error", "function" => "load_processed_rounds"),
                e
            );
        }
    }
}

pub fn display_select_season() {
    let load_processed_seasons_res = load_processed_seasons();
    match load_processed_seasons_res {
        Ok(processed_seasons) => {
            let selection = Select::new(&t!("select_season"), processed_seasons)
                .with_help_message(&t!("help_message"))
                .prompt();

            match selection {
                Ok(season) => {
                    display_select_game(season);
                }
                Err(_) => {
                    println!("{}", t!("interrupted"));
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!(
                "{}:{}",
                t!("error", "function" => "load_processed_seasons"),
                e
            );
        }
    }
}

pub fn display_game_result(game: &Game) {
    let mut _top_innings = game.away_team.name.to_string();
    let mut _bottom_innings = game.home_team.name.to_string();
    _top_innings.push_str(SEPARATOR_TEXT);
    _bottom_innings.push_str(SEPARATOR_TEXT);

    let mut _top_total_score: i8 = 0;
    let mut _bottom_total_score: i8 = 0;
    let mut _inning_index: usize = 1; // to compare with innings.len()

    println!("game.seq:{}", game.seq);
    println!("inning.len:{}", game.innings.len());

    for inning in game.innings.iter() {
        println!("inning:{}({})", inning.seq, inning.tb);
        println!("{LINE_SEPARATOR_TEXT}");

        let mut _top_inning_score: i8 = 0;
        let mut _bottom_inning_score: i8 = 0;

        for count in inning.counts.iter() {
            println!("count.seq:{}", count.seq);

            let mut _top_scoreboard = _top_innings.clone();
            let mut _bottom_scoreboard = _bottom_innings.clone();

            if inning.tb == InningType::TOP {
                _top_inning_score += count.point;
                _top_scoreboard.push_str(&_top_inning_score.to_string());
            } else {
                _bottom_inning_score += count.point;
                _bottom_scoreboard.push_str(&_bottom_inning_score.to_string());
            }

            _top_scoreboard.push_str(SPACE_TEXT);
            _top_scoreboard.push_str(&_top_total_score.to_string());

            if inning.tb == InningType::TOP {
                if game.innings.len() == _inning_index {
                    _bottom_scoreboard.push_str(WALK_OFF_TEXT);
                } else {
                    _bottom_scoreboard.push_str(SPACE_TEXT);
                }
            }

            _bottom_scoreboard.push_str(SPACE_TEXT);
            _bottom_scoreboard.push_str(&_bottom_total_score.to_string());

            println!("{_top_scoreboard}");
            println!("{_bottom_scoreboard}");
            println!("  <{}>", display_runner(count.bases.second));
            println!(
                "<{}> <{}>",
                display_runner(count.bases.third),
                display_runner(count.bases.first)
            );
            println!("  <H>");
            println!("Out Count: {}", count.out);
            println!("Batter: {}", count.batter.name);
            let rounded_ba = (count.batter.hit_average() * 1000.0).round();
            println!(" BA : .{}", rounded_ba);
            let rounded_slg = (count.batter.slg() * 1000.0).round();
            println!(" SLG: .{}", rounded_slg);
            println!("Batting Result: {}", count.result);
            if count.point > 0 {
                println!("Scored: {}", count.point);
            }
            println!("{LINE_SEPARATOR_TEXT}");
        }

        if inning.tb == InningType::TOP {
            _top_innings.push_str(&inning.point.to_string());
            _top_total_score += &inning.point;
        } else {
            _bottom_innings.push_str(&inning.point.to_string());
            _bottom_total_score += &inning.point;
        }
        _inning_index += 1;
    }
}

fn display_runner(runner: bool) -> &'static str {
    if runner { RUNNER_TEXT } else { NO_RUNNER_TEXT }
}

pub fn display_batting_results(game: &Game) {
    println!("Batting Results:");
    println!("{}", game.away_team.name.to_string());

    let mut _top_results: BTreeMap<String, String> = BTreeMap::new();
    let mut _bottom_results: BTreeMap<String, String> = BTreeMap::new();

    for inning in game.innings.iter() {
        for count in inning.counts.iter() {
            if inning.tb == InningType::TOP {
                _top_results
                    .entry(count.batter.name.to_string())
                    .and_modify(|e| {
                        e.push_str(SPACE_TEXT);
                        e.push_str(count.result.to_string().as_str());
                    })
                    .or_insert(count.result.to_string());
            } else {
                _bottom_results
                    .entry(count.batter.name.to_string())
                    .and_modify(|e| {
                        e.push_str(SPACE_TEXT);
                        e.push_str(count.result.to_string().as_str());
                    })
                    .or_insert(count.result.to_string());
            }
        }
    }

    for (key, value) in &_top_results {
        println!("{}: {}", key, value);
    }

    println!("");
    println!("{}", game.home_team.name.to_string());

    for (key, value) in &_bottom_results {
        println!("{}: {}", key, value);
    }
    println!("{LINE_SEPARATOR_TEXT}");
    println!("{LINE_SEPARATOR_TEXT}");
}
