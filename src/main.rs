use rand::Rng;
use std::fs::File;
use std::io::prelude::*;
use std::io::Error;


#[derive(Clone)]
#[derive(Debug)]
enum InningKind { Top, Bottom }
const MAX_INNING: i32 = 9;
const MAX_OUT: i32 = 3;
const HIT_TEXT: &str = "Hit!";
const OUT_TEXT: &str = "Out!";
const SPACE_TEXT: &str = " ";

fn main() {
    let _avg :f64 = 0.4;
    let mut _inning_kind = InningKind::Bottom;
    let mut _is_in_game= true;
    let mut _top_innings: Vec<Inning> = Vec::new();
    let mut _bottom_innings: Vec<Inning> = Vec::new();
    let mut _top_team_name = "Team A";
    let mut _bottom_team_name = "Team B";
    let mut _top_total_score = 0;
    let mut _bottom_total_score = 0;
    let mut _top_scoreboard = String::from("");
    let mut _bottom_scoreboard = String::from("");
    
    let mut _inning_seq = 1;
    let mut rng = rand::thread_rng();

    _top_scoreboard = shape_scoreboard_text(_top_team_name, _top_innings.clone(), SPACE_TEXT, _top_total_score);
    _bottom_scoreboard = shape_scoreboard_text(_bottom_team_name, _bottom_innings.clone(), SPACE_TEXT, _bottom_total_score);

    while _is_in_game {

        if matches!(_inning_kind, InningKind::Top) {
            _inning_kind = InningKind::Bottom;
        } else {
            _inning_kind = InningKind::Top;
        }
        
        let mut _inning = Inning {
             kind: _inning_kind.clone(),
             seq: _inning_seq,
             counts: Vec::new(),
             score: 0
            };
        let mut _count_seq = 0;
        let mut _is_first_runner= false;
        let mut _is_second_runner= false;
        let mut _is_third_runner= false;
        let mut _batting_result = "";
        let mut _out_count = 0;

        while _out_count < MAX_OUT {

            _count_seq += 1;
            let mut _count = Count {
                seq: _count_seq,
                is_first_runner: _is_first_runner,
                is_second_runner: _is_second_runner,
                is_third_runner: _is_third_runner,
                out: _out_count,
            };
            let trial: f64 = rng.gen();
            //println!("Trial: {trial}");
    
            // In case of single hit.
            if _avg > trial {
                _batting_result = HIT_TEXT;
    
                if _is_third_runner {
                    _inning.score += 1;
                    if matches!(_inning_kind, InningKind::Top) {
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
                if _out_count < MAX_OUT {
                    _out_count += 1;
                }
            }

            _count.is_first_runner = _is_first_runner;
            _count.is_second_runner = _is_second_runner;
            _count.is_third_runner = _is_third_runner;
            _count.out = _out_count;
            _inning.counts.push(_count);

            let _inning_kind_text: String = format!("{:?}", _inning_kind);
            println!("Sequence:{}({})-{}", _inning_seq, display_innning_kind(_inning_kind.clone()), _count_seq);

            if matches!(_inning_kind, InningKind::Top) {
                _top_scoreboard = shape_scoreboard_text(_top_team_name, _top_innings.clone(), &_top_total_score.to_string(), _top_total_score);
            } else {
                _bottom_scoreboard = shape_scoreboard_text(_bottom_team_name, _bottom_innings.clone(), &_bottom_total_score.to_string(), _bottom_total_score);
            }
            println!("{_top_scoreboard}");
            println!("{_bottom_scoreboard}");
            println!("  <{}>", runner_text(_is_second_runner));
            println!("<{}> <{}>", runner_text(_is_third_runner), runner_text(_is_first_runner));
            println!("  <H>");
            println!("Batting Result: {_batting_result}");
            println!("Out Count: {_out_count}");
            println!("---");
        }

        if matches!(_inning_kind, InningKind::Top) {
            _top_innings.push(_inning);
            if _inning_seq >= MAX_INNING && _bottom_total_score > _top_total_score {
                _is_in_game = false;
            }
        } else {
            if _inning_seq >= MAX_INNING && _bottom_total_score != _top_total_score {
                _is_in_game = false;
            } else {
                _bottom_innings.push(_inning);
                _inning_seq += 1;
            }   
        }
    } 
           
}

fn runner_text(runner: bool) -> &'static str {
    if runner {
        "R"
    } else {
        "-"
    }
}

fn shape_scoreboard_text(team: &str, innings: Vec<Inning>, score: &str, total_score: i32) -> String {
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

    _scoreboard_text.insert_str(0,":");
    _scoreboard_text.insert_str(0, team);
    _scoreboard_text.push_str(SPACE_TEXT);
    _scoreboard_text.push_str(total_score.to_string().as_str());
    _scoreboard_text
}

fn display_innning_kind(inning_kind: InningKind) -> &'static str {
    match inning_kind {
        InningKind::Top => "Top",
        InningKind::Bottom => "Bottom",
    }
}

#[derive(Clone)]
struct Inning {
    kind: InningKind,
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
    out: i32,
}