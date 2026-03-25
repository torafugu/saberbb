mod team;

use rand::Rng;
use std::fmt;
use std::fs::File;
use std::io::prelude::*;
use std::io::Error;
use team::Batter;
use team::Team;

const MAX_INNING: i32 = 9;
const MAX_OUT: i32 = 3;
const HIT_TEXT: &str = "Hit!";
const OUT_TEXT: &str = "Out!";
const SPACE_TEXT: &str = " ";
const SEPARATOR_TEXT: &str = ":";
const INNING_TOP_TEXT: &str = "Top";
const INNING_BOTTOM_TEXT: &str = "Bottom";
const WALK_OFF_TEXT: &str = "x";
const RUNNER_TEXT: &str = "R";
const NO_RUNNER_TEXT: &str = "-";
const LINE_SEPARATOR_TEXT: &str = "---";
#[derive(Clone)]
enum InningType {
    Top,
    Bottom,
}
#[derive(Clone)]
enum BattingResult {
    Hit,
    Out,
}
impl fmt::Display for BattingResult {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            BattingResult::Hit => write!(f, "{HIT_TEXT}"),
            BattingResult::Out => write!(f, "{OUT_TEXT}"),
        }
    }
}

fn main() {
    let mut _inning_kind = InningType::Bottom;
    let mut _is_in_game = true;
    let mut _top_innings: Vec<Inning> = Vec::new();
    let mut _bottom_innings: Vec<Inning> = Vec::new();

    let _top_team = Team::new("AAA");
    let _bottom_team = Team::new("BBB");
    let _top_batter1: Batter = Batter::new("Top batter 1", 0.35);
    let _top_batter2: Batter = Batter::new("Top batter 2", 0.35);
    let _top_batter3: Batter = Batter::new("Top batter 3", 0.35);
    let _top_batter4: Batter = Batter::new("Top batter 4", 0.35);
    let _top_batter5: Batter = Batter::new("Top batter 5", 0.35);
    let _top_batter6: Batter = Batter::new("Top batter 6", 0.35);
    let _top_batter7: Batter = Batter::new("Top batter 7", 0.35);
    let _top_batter8: Batter = Batter::new("Top batter 8", 0.35);
    let _top_batter9: Batter = Batter::new("Top batter 9", 0.35);
    let mut _current_top_batter_order: i8 = 0;
    let _bottom_batter1: Batter = Batter::new("Bottom batter 1", 0.35);
    let _bottom_batter2: Batter = Batter::new("Bottom batter 2", 0.35);
    let _bottom_batter3: Batter = Batter::new("Bottom batter 3", 0.35);
    let _bottom_batter4: Batter = Batter::new("Bottom batter 4", 0.35);
    let _bottom_batter5: Batter = Batter::new("Bottom batter 5", 0.35);
    let _bottom_batter6: Batter = Batter::new("Bottom batter 6", 0.35);
    let _bottom_batter7: Batter = Batter::new("Bottom batter 7", 0.35);
    let _bottom_batter8: Batter = Batter::new("Bottom batter 8", 0.35);
    let _bottom_batter9: Batter = Batter::new("Bottom batter 9", 0.35);
    let mut _current_bottom_batter_order: i8 = 0;

    let mut _top_total_score = 0;
    let mut _bottom_total_score = 0;
    let mut _top_scoreboard = String::from("");
    let mut _bottom_scoreboard = String::from("");

    let mut _inning_seq = 1;
    let mut rng = rand::thread_rng();

    while _is_in_game {
        if matches!(_inning_kind, InningType::Bottom) {
            _inning_kind = InningType::Top;
        } else {
            _inning_kind = InningType::Bottom;
            if _inning_seq >= 9 {}
        }

        let mut _inning = Inning {
            kind: _inning_kind.clone(),
            seq: _inning_seq,
            counts: Vec::new(),
            score: 0,
        };
        let mut _count_seq = 0;
        let mut _is_first_runner = false;
        let mut _is_second_runner = false;
        let mut _is_third_runner = false;
        let mut _batting_result = "";
        let mut _out_count = 0;

        while _out_count < MAX_OUT {
            _count_seq += 1;

            let _current_batter: Batter;

            if matches!(_inning_kind, InningType::Top) {
                if _current_top_batter_order == 9 {
                    _current_top_batter_order = 1;
                } else {
                    _current_top_batter_order += 1;
                }

                match _current_top_batter_order {
                    1 => _current_batter = _top_batter1.clone(),
                    2 => _current_batter = _top_batter2.clone(),
                    3 => _current_batter = _top_batter3.clone(),
                    4 => _current_batter = _top_batter4.clone(),
                    5 => _current_batter = _top_batter5.clone(),
                    6 => _current_batter = _top_batter6.clone(),
                    7 => _current_batter = _top_batter7.clone(),
                    8 => _current_batter = _top_batter8.clone(),
                    9 => _current_batter = _top_batter9.clone(),
                    _ => _current_batter = _top_batter1.clone(),
                }
            } else {
                if _current_bottom_batter_order == 9 {
                    _current_bottom_batter_order = 1;
                } else {
                    _current_bottom_batter_order += 1;
                }

                match _current_bottom_batter_order {
                    1 => _current_batter = _bottom_batter1.clone(),
                    2 => _current_batter = _bottom_batter2.clone(),
                    3 => _current_batter = _bottom_batter3.clone(),
                    4 => _current_batter = _bottom_batter4.clone(),
                    5 => _current_batter = _bottom_batter5.clone(),
                    6 => _current_batter = _bottom_batter6.clone(),
                    7 => _current_batter = _bottom_batter7.clone(),
                    8 => _current_batter = _bottom_batter8.clone(),
                    9 => _current_batter = _bottom_batter9.clone(),
                    _ => _current_batter = _bottom_batter1.clone(),
                }
            }

            let mut _count = Count {
                seq: _count_seq,
                is_first_runner: _is_first_runner,
                is_second_runner: _is_second_runner,
                is_third_runner: _is_third_runner,
                batter: _current_batter,
                result: BattingResult::Out,
                score: 0,
                out: _out_count,
            };
            let trial: f32 = rng.gen();

            // In case of single hit.
            if _count.batter.average() > trial {
                _batting_result = HIT_TEXT;
                _count.result = BattingResult::Hit;

                if _is_third_runner {
                    _count.score += 1;
                    _inning.score += 1;
                    if matches!(_inning_kind, InningType::Top) {
                        _top_total_score += 1;
                    } else {
                        _bottom_total_score += 1;
                    }
                }
                if _is_second_runner {
                    _is_third_runner = true;
                }
                if _is_first_runner {
                    _is_second_runner = true;
                }
                _is_first_runner = true;
            } else {
                _batting_result = OUT_TEXT;
                _count.result = BattingResult::Out;
                if _out_count < MAX_OUT {
                    _out_count += 1;
                }
            }

            _count.is_first_runner = _is_first_runner;
            _count.is_second_runner = _is_second_runner;
            _count.is_third_runner = _is_third_runner;
            _count.out = _out_count;
            _inning.counts.push(_count);

            if _inning_seq >= MAX_INNING && _bottom_total_score != _top_total_score {
                _is_in_game = false;
                break;
            }
        }

        if matches!(_inning_kind, InningType::Top) {
            _top_innings.push(_inning);
            if _inning_seq >= MAX_INNING && _bottom_total_score > _top_total_score {
                _is_in_game = false;
            }
        } else {
            _bottom_innings.push(_inning);
            _inning_seq += 1;
        }
    }

    let mut _current_top_innings: Vec<Inning> = Vec::new();
    let mut _current_bottom_innings: Vec<Inning> = Vec::new();
    let mut _current_top_score = 0;
    let mut _current_bottom_score = 0;
    let mut _bottom_innning_score = 0;
    let mut _top_innning_score = 0;
    let mut _bottom_innning_score = 0;
    _top_scoreboard = shape_scoreboard_text(
        _top_team.name(),
        _current_top_innings.clone(),
        SPACE_TEXT,
        _current_top_score,
    );
    _bottom_scoreboard = shape_scoreboard_text(
        _bottom_team.name(),
        _current_bottom_innings.clone(),
        SPACE_TEXT,
        _current_bottom_score,
    );

    // Dislay the game result.
    for i in 0.._inning_seq {
        let usize_i: usize = i as usize;
        _top_innning_score = 0;
        _bottom_innning_score = 0;
        match _top_innings.get(usize_i) {
            Some(inning) => {
                for count in inning.counts.iter() {
                    _top_innning_score += count.score;
                    _current_top_score += count.score;
                    _top_scoreboard = shape_scoreboard_text(
                        _top_team.name(),
                        _current_top_innings.clone(),
                        &_top_innning_score.to_string(),
                        _current_top_score,
                    );
                    display_inning_and_count_seq(i + 1, INNING_TOP_TEXT.to_string(), count.seq);
                    println!("{_top_scoreboard}");
                    println!("{_bottom_scoreboard}");
                    display_count_detail(count);
                    display_line_separator();
                }
                _current_top_innings.push(inning.clone());
            }
            None => {}
        }
        match _bottom_innings.get(usize_i) {
            Some(inning) => {
                for count in inning.counts.iter() {
                    _bottom_innning_score += count.score;
                    _current_bottom_score += count.score;
                    _bottom_scoreboard = shape_scoreboard_text(
                        _bottom_team.name(),
                        _current_bottom_innings.clone(),
                        &_bottom_innning_score.to_string(),
                        _current_bottom_score,
                    );
                    display_inning_and_count_seq(i + 1, INNING_BOTTOM_TEXT.to_string(), count.seq);
                    println!("{_top_scoreboard}");
                    println!("{_bottom_scoreboard}");
                    display_count_detail(count);
                    display_line_separator();
                }
                _current_bottom_innings.push(inning.clone());
            }
            None => {
                _top_scoreboard = shape_scoreboard_text(
                    _top_team.name(),
                    _current_top_innings.clone(),
                    SPACE_TEXT,
                    _current_top_score,
                );
                _bottom_scoreboard = shape_scoreboard_text(
                    _bottom_team.name(),
                    _current_bottom_innings.clone(),
                    WALK_OFF_TEXT,
                    _current_bottom_score,
                );
            }
        }
    }
    println!("Game Set!");
    println!("{_top_scoreboard}");
    println!("{_bottom_scoreboard}");
}

fn display_inning_and_count_seq(inning_seq: i32, inning_kind: String, count_seq: i32) {
    println!("Inning:{}({}) Count:{}", inning_seq, inning_kind, count_seq);
}

fn display_count_detail(count: &Count) {
    println!("  <{}>", runner_text(count.is_second_runner));
    println!(
        "<{}> <{}>",
        runner_text(count.is_third_runner),
        runner_text(count.is_first_runner)
    );
    println!("  <H>");
    println!("Out Count: {}", count.out);
    println!("Batter: {}", count.batter.name());
    println!("Batting Result: {}", count.result);
}

fn runner_text(runner: bool) -> &'static str {
    if runner {
        RUNNER_TEXT
    } else {
        NO_RUNNER_TEXT
    }
}

fn display_line_separator() {
    println!("{LINE_SEPARATOR_TEXT}");
}

fn shape_scoreboard_text(
    team: &str,
    innings: Vec<Inning>,
    score: &str,
    total_score: i32,
) -> String {
    let mut _scoreboard_text = String::from("");

    for inning in innings.iter() {
        _scoreboard_text.push_str(inning.score.to_string().as_str());
    }

    _scoreboard_text.push_str(score);

    let _scoreboard_text_length: i32 = _scoreboard_text.chars().count() as i32;

    if _scoreboard_text_length <= MAX_INNING {
        for _ in 0..(MAX_INNING - _scoreboard_text_length) {
            _scoreboard_text.push_str(SPACE_TEXT);
        }
    }

    _scoreboard_text.insert_str(0, SEPARATOR_TEXT);
    _scoreboard_text.insert_str(0, team);
    _scoreboard_text.push_str(SPACE_TEXT);
    _scoreboard_text.push_str(total_score.to_string().as_str());
    _scoreboard_text
}

#[derive(Clone)]
struct Inning {
    kind: InningType,
    seq: i32,
    counts: Vec<Count>,
    score: i32,
}

#[derive(Clone)]
struct Count {
    seq: i32,
    is_first_runner: bool,
    is_second_runner: bool,
    is_third_runner: bool,
    batter: Batter,
    result: BattingResult,
    score: i32,
    out: i32,
}
