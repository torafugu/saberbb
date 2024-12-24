use rand::Rng;
use std::fs::File;
use std::io::prelude::*;
use std::io::Error;

enum InningKind { Top, Bottom }
const MAX_OUT: i32 = 3;

fn main() {
    let _avg :f64 = 0.45;
    let mut inning_count = 0;
    let mut inning_kind: InningKind  = InningKind::Bottom;
    let mut is_in_game= true;
    let mut top_innings: Vec<Inning> = Vec::new();
    let mut bottom_innings: Vec<Inning> = Vec::new();

    
    let mut _batting_first_scores: Vec<i32> = Vec::new();
    let mut _field_first_scores: Vec<i32> = Vec::new();
    
    let mut inning_seq = 0;

    let mut rng = rand::thread_rng();
    _batting_first_scores.push(0);
    _field_first_scores.push(0);
    //let mut _current_scores: Vec<i32> = _batting_first_scores;

    while is_in_game {

        if matches!(inning_kind, InningKind::Top) {
            inning_kind = InningKind::Bottom;
        } else {
            inning_kind = InningKind::Top;
            inning_seq += 1;
        }

        let mut inning = Inning { kind: InningKind::Top, seq: 1, counts: Vec::new(), score: 0};
        let mut count_seq = 0;
        let mut is_first_runner= false;
        let mut is_second_runner= false;
        let mut is_third_runner= false;
        let mut out_count = 0;

        while out_count < MAX_OUT {

            count_seq += 1;
            let mut count = Count {
                 seq: count_seq,
                is_first_runner: is_first_runner,
                is_second_runner: is_second_runner,
                is_third_runner: is_third_runner,
                out: out_count,
            };
            let trial: f64 = rng.gen();
            println!("Trial: {trial}");
    
            if _avg > trial {
                println!("Hit!");
    
                if is_third_runner {
                    _batting_first_scores[0] = _batting_first_scores[0] + 1;
                }
                if is_second_runner {
                    is_third_runner = true;
                }
                if is_first_runner {
                    is_second_runner = true;
                }
                is_first_runner = true;
                
            } else {
                println!("Out!");
                if out_count < MAX_OUT {
                    out_count += 1;
                }
            }
        
            println!("Score: {}-{}", _batting_first_scores[0], _field_first_scores[0]);
            println!("  <{}>", runner_text(is_second_runner));
            println!("<{}> <{}>", runner_text(is_third_runner), runner_text(is_first_runner));
            println!("  <H>");
            println!("Out Count: {out_count}");
        }








        top_innings.push(inning);


        if inning_count == 9 {
            
        }

        is_in_game = false;
        
    } 
           
}

fn process_inning(inning: Inning) -> Inning {

    inning



    //false
}

fn runner_text(runner: bool) -> &'static str {
    if runner {
        "R"
    } else {
        "-"
    }
}

struct Inning {
    kind: InningKind,
    seq: i32,
    counts: Vec<Count>,
    score: i32,
}

struct Count {
    seq: i32,
    is_first_runner: bool,
    is_second_runner: bool,
    is_third_runner: bool,
    out: i32,
}