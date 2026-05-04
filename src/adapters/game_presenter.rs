use super::menu_component::MenuItem;
use crate::domain::shared::game::Game;
use crate::domain::shared::types::InningType;
use crate::repositories::game_repository::{load_processed_games, load_processed_seasons};
use crate::t;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{CellAlignment, Table};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::terminal::{Clear, ClearType, disable_raw_mode, enable_raw_mode};
use crossterm::{cursor, execute};
use inquire::Select;
use std::collections::BTreeMap;
use std::io;

const LINE_SEPARATOR_TEXT: &str = "---";
const RUNNER_TEXT: &str = "R";
const NO_RUNNER_TEXT: &str = "-";
const SPACE_TEXT: &str = " ";
const SEPARATOR_TEXT: &str = ":";
const WALK_OFF_TEXT: &str = "x";

macro_rules! rprintln {
    ($($arg:tt)*) => {
        let output = format!($($arg)*);
        for line in output.lines() {
            let _ = crossterm::execute!(
                std::io::stdout(),
                crossterm::cursor::MoveToColumn(0),
                crossterm::terminal::Clear(crossterm::terminal::ClearType::UntilNewLine)
            );
            print!("{}\r\n", line);
        }
        let _ = std::io::Write::flush(&mut std::io::stdout());
    };
}

pub fn display_game_rounds_processed(num_of_rounds: i8) {
    println!("{} rounds processed.", num_of_rounds);
}

pub fn display_game_detail(
    game: &Game,
    inning_seq: i8,
    inning_tb: InningType,
    count_id: i8,
) -> io::Result<()> {
    enable_raw_mode()?;
    let mut should_redraw = true;
    // println!("<- {} {} ->");
    let mut stdout = io::stdout();

    loop {
        if should_redraw {
            let mut table = Table::new();
            execute!(stdout, cursor::MoveTo(0, 0))?;
            rprintln!("Game.id:{}", game.id);
            rprintln!("inning:{}({})", inning_seq, inning_tb);
            rprintln!("{LINE_SEPARATOR_TEXT}");

            let max_ining_seq = game.innings.iter().map(|i| i.seq).max().unwrap_or(0);

            let mut headers: Vec<String> = Vec::new();
            headers.push(t!("team"));
            for inning_num in 1..max_ining_seq + 1 {
                headers.push(inning_num.to_string());
            }
            headers.push(t!("total_score"));

            // for inning in &game.innings {
            //     headers.push(inning.seq.to_string());
            // }
            table.set_header(headers);
            table.add_row(vec![&game.away_team.name]);
            table.add_row(vec![&game.home_team.name]);
            rprintln!("{table}");
            should_redraw = false;
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key_event) = event::read()? {
                match key_event {
                    KeyEvent {
                        code: KeyCode::Left,
                        ..
                    } => {
                        println!("前のカウント");
                    }
                    KeyEvent {
                        code: KeyCode::Right,
                        ..
                    } => {
                        // let _ = display_game_detail(&game, inning_seq, inning_tb, count_id);
                        rprintln!("Redraw!");
                        should_redraw = true;
                    }
                    KeyEvent {
                        code: KeyCode::Char('c'),
                        modifiers: KeyModifiers::CONTROL,
                        ..
                    }
                    | KeyEvent {
                        code: KeyCode::Esc, ..
                    } => {
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    disable_raw_mode()?;
    Ok(())
}

pub fn display_select_game(season: i16) {
    let game_rounds_res = load_processed_games(season);
    match game_rounds_res {
        Ok(games) => {
            let menu_items: Vec<MenuItem<Game>> = games
                .into_iter()
                .map(
                    |Game {
                         id,
                         planned_date,
                         actual_date,
                         away_team,
                         home_team,
                         game_type,
                         innings,
                         away_point,
                         home_point,
                         away_batters,
                         home_batters,
                     }| {
                        let label = format!(
                            "[{}] {} vs {})",
                            actual_date, away_team.name, home_team.name,
                        );

                        MenuItem {
                            label,
                            value: Game {
                                id,
                                planned_date,
                                actual_date,
                                away_team,
                                home_team,
                                game_type,
                                innings,
                                away_point,
                                home_point,
                                away_batters,
                                home_batters,
                            },
                        }
                    },
                )
                .collect();

            let selection = Select::new(&t!("select_game"), menu_items).prompt();

            if let Ok(selected) = selection {
                let _ = display_game_detail(&selected.value, 1, InningType::TOP, 1);

                // display_game_result(&selected.value);
                // display_batting_results(&selected.value);
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
    // TODO: load_inning, load_count
    let mut _top_innings = game.away_team.name.to_string();
    let mut _bottom_innings = game.home_team.name.to_string();
    _top_innings.push_str(SEPARATOR_TEXT);
    _bottom_innings.push_str(SEPARATOR_TEXT);

    let mut _top_total_score: i8 = 0;
    let mut _bottom_total_score: i8 = 0;
    let mut _inning_index: usize = 1; // to compare with innings.len()

    println!("game.id:{}", game.id);
    // println!("inning.len:{}", game.innings.len());

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
