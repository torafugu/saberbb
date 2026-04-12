mod repository;
mod resolver;
mod shared;

// use repository::BattingResultRecord;
use repository::ERROR_LOAD_GAME_MANAGER;
use repository::ERROR_SAVE_GAME_MANAGER;
use repository::load_game_manager;
use repository::save_game_manager;
use resolver::batting_resolve;
use shared::game::Game;
use shared::player::Batter;
use shared::team::Team;
use shared::types::BattingResult;
use shared::types::Count;
use shared::types::Inning;
use shared::types::InningType;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::shared::game::GameManager;

const MAX_INNING: i8 = 9;
const MAX_OUT: i8 = 3;
const SPACE_TEXT: &str = " ";
const SEPARATOR_TEXT: &str = ":";
const INNING_TOP_TEXT: &str = "Top";
const INNING_BOTTOM_TEXT: &str = "Bottom";
const WALK_OFF_TEXT: &str = "x";
const RUNNER_TEXT: &str = "R";
const NO_RUNNER_TEXT: &str = "-";
const LINE_SEPARATOR_TEXT: &str = "---";

fn main() {
    let mut _is_in_game: bool = true;
    let mut _top_scoreboard: Arc<str> = Arc::from("");
    let mut _bottom_scoreboard: Arc<str> = Arc::from("");

    let mut _game_manager: GameManager = GameManager {
        season: 1,
        phase: 2,
    };

    let mut _game: Game = Game {
        seq: 1,
        top_team: Team::new("AAA"),
        bottom_team: Team::new("BBB"),
        top_innings: Vec::new(),
        bottom_innings: Vec::new(),
        tb: InningType::Bottom,
        inning_seq: 0,
        top_batters: [
            Batter::default(),
            Batter::new("Top batter 1", 1.0, -0.5),
            Batter::new("Top batter 2", 1.2, -0.8),
            Batter::new("Top batter 3", 1.4, 0.8),
            Batter::new("Top batter 4", 1.6, 1.0),
            Batter::new("Top batter 5", 1.5, 0.9),
            Batter::new("Top batter 6", -0.1, 0.2),
            Batter::new("Top batter 7", 0.1, -0.3),
            Batter::new("Top batter 8", -1.0, -0.5),
            Batter::new("Top batter 9", -1.2, -1.2),
        ],
        bottom_batters: [
            Batter::default(),
            Batter::new("Bottom batter 1", 0.9, -0.8),
            Batter::new("Bottom batter 2", 1.1, -0.6),
            Batter::new("Bottom batter 3", 1.2, 1.0),
            Batter::new("Bottom batter 4", 1.4, 1.4),
            Batter::new("Bottom batter 5", 0.2, 1.1),
            Batter::new("Bottom batter 6", -0.5, -0.2),
            Batter::new("Bottom batter 7", -0.8, -0.1),
            Batter::new("Bottom batter 8", -1.3, -0.3),
            Batter::new("Bottom batter 9", -1.4, -0.4),
        ],
        current_top_batter_order: 0,
        current_bottom_batter_order: 0,
        current_batter: Batter::default(),
        top_total_score: 0,
        bottom_total_score: 0,
    };

    // loop for an innning
    while _is_in_game {
        _game.tb = next_tb(_game.tb);

        let mut _inning: Inning = Inning {
            //tb: _game.tb,
            //seq: _game.inning_seq,
            counts: Vec::new(),
            score: 0,
        };
        let mut _count_seq = 0;
        let mut _is_first_runner = false;
        let mut _is_second_runner = false;
        let mut _is_third_runner = false;
        let mut _out_count = 0;

        while _out_count < MAX_OUT {
            _count_seq += 1;

            let _current_batter: Arc<Batter>;

            let mut _count = Count {
                seq: _count_seq,
                is_first_runner: _is_first_runner,
                is_second_runner: _is_second_runner,
                is_third_runner: _is_third_runner,
                batter: Arc::from(_game.next_batter()),
                result: BattingResult::OUT,
                score: 0,
                out: _out_count,
            };

            // Batting result calculation
            _count.result = batting_resolve(_count.batter.clone());

            match _count.result {
                BattingResult::SINGLE => {
                    if _is_third_runner {
                        _count.score += 1;
                        _inning.score += 1;
                        _game.add_score(1);
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
                        _count.score += 1;
                        _inning.score += 1;
                        _game.add_score(1);
                        _is_third_runner = false;
                    }
                    if _is_second_runner {
                        _count.score += 1;
                        _inning.score += 1;
                        _game.add_score(1);
                    }
                    if _is_first_runner {
                        _is_first_runner = false;
                        _is_third_runner = true;
                    }
                    _is_second_runner = true;
                }
                BattingResult::TRIPLE => {
                    if _is_third_runner {
                        _count.score += 1;
                        _inning.score += 1;
                        _game.add_score(1);
                    }
                    if _is_second_runner {
                        _count.score += 1;
                        _inning.score += 1;
                        _game.add_score(1);
                        _is_second_runner = false;
                    }
                    if _is_first_runner {
                        _count.score += 1;
                        _inning.score += 1;
                        _game.add_score(1);
                        _is_first_runner = false;
                    }
                    _is_third_runner = true;
                }
                BattingResult::HOMERUN => {
                    if _is_third_runner {
                        _count.score += 1;
                        _inning.score += 1;
                        _game.add_score(1);
                        _is_third_runner = false;
                    }
                    if _is_second_runner {
                        _count.score += 1;
                        _inning.score += 1;
                        _game.add_score(1);
                        _is_second_runner = false;
                    }
                    if _is_first_runner {
                        _count.score += 1;
                        _inning.score += 1;
                        _game.add_score(1);
                        _is_first_runner = false;
                    }
                    _count.score += 1;
                    _inning.score += 1;
                    _game.add_score(1);
                }
                _ => {
                    _count.result = BattingResult::OUT;
                    if _out_count < MAX_OUT {
                        _out_count += 1;
                    }
                }
            }

            _count.is_first_runner = _is_first_runner;
            _count.is_second_runner = _is_second_runner;
            _count.is_third_runner = _is_third_runner;
            _count.out = _out_count;
            _inning.counts.push(_count);

            // Check walk-off
            if _game.inning_seq >= MAX_INNING && _game.bottom_total_score != _game.top_total_score {
                _is_in_game = false;
                break;
            }
        }

        _game.add_inning(_inning);

        // Check Game-Set
        if matches!(_game.tb, InningType::Top) {
            if _game.inning_seq >= MAX_INNING && _game.bottom_total_score > _game.top_total_score {
                _is_in_game = false;
            }
        } else {
            _game.inning_seq += 1;
        }
    }

    let mut _current_top_innings: Vec<Inning> = Vec::new();
    let mut _current_bottom_innings: Vec<Inning> = Vec::new();
    let mut _current_top_score: i8 = 0;
    let mut _current_bottom_score: i8 = 0;
    let mut _bottom_innning_score: i8 = 0;
    let mut _top_innning_score: i8 = 0;
    let mut _bottom_innning_score: i8 = 0;
    _top_scoreboard = shape_scoreboard(
        _game.top_team.name(),
        _current_top_innings.clone(),
        SPACE_TEXT,
        _current_top_score,
    );
    _bottom_scoreboard = shape_scoreboard(
        _game.bottom_team.name(),
        _current_bottom_innings.clone(),
        SPACE_TEXT,
        _current_bottom_score,
    );

    // Dislay the game result.
    for i in 0.._game.inning_seq {
        let usize_i: usize = i as usize;
        _top_innning_score = 0;
        _bottom_innning_score = 0;
        match _game.top_innings.get(usize_i) {
            Some(inning) => {
                for count in inning.counts.iter() {
                    _top_innning_score += count.score;
                    _current_top_score += count.score;
                    _top_scoreboard = shape_scoreboard(
                        _game.top_team.name(),
                        _current_top_innings.clone(),
                        &_top_innning_score.to_string(),
                        _current_top_score,
                    );
                    display_inning_and_count_seq(i + 1, INNING_TOP_TEXT.to_string(), count.seq);
                    display_scoreboads(_top_scoreboard.to_string(), _bottom_scoreboard.to_string());
                    display_count_detail(count);
                    display_line_separator();
                }
                _current_top_innings.push(inning.clone());
            }
            None => {}
        }
        match _game.bottom_innings.get(usize_i) {
            Some(inning) => {
                for count in inning.counts.iter() {
                    _bottom_innning_score += count.score;
                    _current_bottom_score += count.score;
                    _bottom_scoreboard = shape_scoreboard(
                        _game.bottom_team.name(),
                        _current_bottom_innings.clone(),
                        &_bottom_innning_score.to_string(),
                        _current_bottom_score,
                    );
                    display_inning_and_count_seq(i + 1, INNING_BOTTOM_TEXT.to_string(), count.seq);
                    display_scoreboads(_top_scoreboard.to_string(), _bottom_scoreboard.to_string());
                    display_count_detail(count);
                    display_line_separator();
                }
                _current_bottom_innings.push(inning.clone());
            }
            None => {
                _top_scoreboard = shape_scoreboard(
                    _game.top_team.name(),
                    _current_top_innings.clone(),
                    SPACE_TEXT,
                    _current_top_score,
                );
                _bottom_scoreboard = shape_scoreboard(
                    _game.bottom_team.name(),
                    _current_bottom_innings.clone(),
                    WALK_OFF_TEXT,
                    _current_bottom_score,
                );
            }
        }
    }
    println!("Game Set!");
    display_scoreboads(_top_scoreboard.to_string(), _bottom_scoreboard.to_string());
    println!("");
    display_batting_results(&_game);
    if let Err(e) = save_game_manager(_game_manager) {
        eprintln!("{}", ERROR_SAVE_GAME_MANAGER);
    }

    let load_game_manage_res: Result<GameManager, _> = load_game_manager();
    match load_game_manage_res {
        Ok(manager) => {}
        Err(e) => {
            eprintln!("{}", ERROR_LOAD_GAME_MANAGER);
        }
    }
}

fn next_tb(tb: InningType) -> InningType {
    if matches!(tb, InningType::Bottom) {
        InningType::Top
    } else {
        InningType::Bottom
    }
}

fn shape_scoreboard(team: &str, innings: Vec<Inning>, score: &str, total_score: i8) -> Arc<str> {
    let mut _scoreboard_text = String::from("");

    for inning in innings.iter() {
        _scoreboard_text.push_str(inning.score.to_string().as_str());
    }

    _scoreboard_text.push_str(score);

    let _scoreboard_text_length: i8 = _scoreboard_text.chars().count() as i8;

    if _scoreboard_text_length <= MAX_INNING {
        for _ in 0..(MAX_INNING - _scoreboard_text_length) {
            _scoreboard_text.push_str(SPACE_TEXT);
        }
    }

    _scoreboard_text.insert_str(0, SEPARATOR_TEXT);
    _scoreboard_text.insert_str(0, team);
    _scoreboard_text.push_str(SPACE_TEXT);
    _scoreboard_text.push_str(total_score.to_string().as_str());
    Arc::from(_scoreboard_text)
}

fn display_line_separator() {
    println!("{LINE_SEPARATOR_TEXT}");
}

fn display_inning_and_count_seq(inning_seq: i8, inning_type: String, count_seq: i32) {
    println!("Inning:{}({}) Count:{}", inning_seq, inning_type, count_seq);
}

fn display_scoreboads(top_scoreboard: String, bottom_scoreboard: String) {
    println!("{top_scoreboard}");
    println!("{bottom_scoreboard}");
}

fn display_runner(runner: bool) -> &'static str {
    if runner { RUNNER_TEXT } else { NO_RUNNER_TEXT }
}

fn display_count_detail(count: &Count) {
    println!("  <{}>", display_runner(count.is_second_runner));
    println!(
        "<{}> <{}>",
        display_runner(count.is_third_runner),
        display_runner(count.is_first_runner)
    );
    println!("  <H>");
    println!("Out Count: {}", count.out);
    println!("Batter: {}", count.batter.name);
    let rounded_ba = (count.batter.hit_average() * 1000.0).round();
    println!(" BA : .{}", rounded_ba);
    let rounded_slg = (count.batter.slg() * 1000.0).round();
    println!(" SLG: .{}", rounded_slg);
    println!("Batting Result: {}", count.result);
    if count.score > 0 {
        println!("Scored: {}", count.score);
    }
}

fn display_batting_results(in_game_params: &Game) {
    println!("Batting Results:");
    println!("{}", in_game_params.top_team.name().to_string());

    let _top_results: BTreeMap<String, String> =
        shape_batting_results(in_game_params.top_innings.clone());

    for (key, value) in &_top_results {
        println!("{}: {}", key, value);
    }

    println!("");
    println!("{}", in_game_params.bottom_team.name().to_string());

    let _bottom_results: BTreeMap<String, String> =
        shape_batting_results(in_game_params.bottom_innings.clone());

    for (key, value) in &_bottom_results {
        println!("{}: {}", key, value);
    }
}

fn shape_batting_results(innings: Vec<Inning>) -> BTreeMap<String, String> {
    let mut _results: BTreeMap<String, String> = BTreeMap::new();

    for inning in innings.iter() {
        for count in inning.counts.iter() {
            _results
                .entry(count.batter.name.to_string())
                .and_modify(|e| {
                    e.push_str(SPACE_TEXT);
                    e.push_str(count.result.to_string().as_str());
                })
                .or_insert(count.result.to_string());
        }
    }
    _results
}
