use super::menu_component::{MenuItem, init_terminal, restore_terminal};
use crate::APP_CONTEXT;
use crate::domain::shared::game::{Count, GameHeader, GameRow};
use crate::domain::shared::game_cursor::{GameCursor, ScoreBoard};
use crate::domain::shared::types::{Base, InningType};
use crate::domain::utils::is_base_occupied;
use crate::i18n::I18nManager;
use crate::repositories::game_repository::GameRepository;
use crate::rprintln;
use crate::t;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{CellAlignment, Table};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use crossterm::{cursor, execute};
use inquire::Select;
use std::collections::BTreeMap;
use std::io;

const LINE_SEPARATOR: &str = "---";
const RUNNER: &str = "R";
const NO_RUNNER: &str = "-";
const SPACE: &str = " ";
const SEPARATOR: &str = ":";
const WALK_OFF: &str = "x";

pub fn display_game_rounds_processed(num_of_rounds: i8) {
    println!(
        "{}",
        t!("game_rounds_processed", "num_of_rounds" => num_of_rounds.to_string())
    );
}

pub fn display_game_detail(game: &GameRow) -> io::Result<()> {
    let mut cursor = GameCursor::new(game.clone());

    let mut stdout = io::stdout();
    let _ = init_terminal();
    let mut should_redraw = true;

    loop {
        if should_redraw {
            execute!(stdout, cursor::MoveTo(0, 0))?;
            // Display header
            rprintln!("{}", format_header(&cursor));

            // Display scoreboard
            rprintln!("{}", format_scoreboard(&cursor.current_scoreboard()));

            // Display count
            rprintln!("{}", format_count(&cursor.current_count()));
            should_redraw = false;
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key_event) = event::read()? {
                match key_event {
                    KeyEvent {
                        code: KeyCode::Left,
                        ..
                    } => {
                        cursor.prev();
                        should_redraw = true;
                    }
                    KeyEvent {
                        code: KeyCode::Right,
                        ..
                    } => {
                        cursor.next();
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
                        let _ = restore_terminal();
                        break;
                    }
                    _ => {}
                }
            }
        }
    }
    let _ = restore_terminal();
    Ok(())
}

fn format_header(cursor: &GameCursor) -> String {
    let mut formated_header = format!("<- {} {} ->", t!("prev_count"), t!("next_count"));
    formated_header.push_str("\r\n");
    formated_header.push_str(&format!(
        "<{}> {}:{}({}) {}:{}",
        cursor.game_type(),
        t!("current_inning"),
        cursor.inning_seq,
        cursor.inning_tb.to_string(),
        t!("current_count"),
        cursor.count_seq
    ));
    formated_header
}

fn format_scoreboard(scoreboard: &ScoreBoard) -> String {
    let mut table = Table::new();
    let mut headers: Vec<String> = Vec::new();
    let mut away_scores: Vec<String> = Vec::new();
    let mut home_scores: Vec<String> = Vec::new();
    headers.push(t!("team"));
    away_scores.push((scoreboard.away_team_name).to_string());
    home_scores.push((scoreboard.home_team_name).to_string());

    for inning_seq in 0..scoreboard.max_inning_num {
        headers.push((inning_seq + 1).to_string());
        away_scores.push("".to_string());
        home_scores.push("".to_string());
    }

    for inning_seq in 0..scoreboard.away_innning_points.len() {
        away_scores[inning_seq + 1] = scoreboard.away_innning_points[inning_seq].to_string();
    }

    for inning_seq in 0..scoreboard.home_innning_points.len() {
        home_scores[inning_seq + 1] = scoreboard.home_innning_points[inning_seq].to_string();
    }

    if scoreboard.is_last_bottom_inning_skiped {
        home_scores[scoreboard.max_inning_num as usize] = WALK_OFF.to_string();
    }

    headers.push(t!("total_score"));
    away_scores.push(scoreboard.away_total_point.to_string());
    home_scores.push(scoreboard.home_total_point.to_string());

    table.set_header(headers);
    table.add_row(away_scores);
    table.add_row(home_scores);
    for inning_seq in 1..table.column_count() {
        table
            .column_mut(inning_seq as usize)
            .unwrap()
            .set_cell_alignment(CellAlignment::Right);
    }
    table
        .load_preset(UTF8_FULL)
        .apply_modifier(UTF8_ROUND_CORNERS);
    format!("{table}")
}

fn format_count(count: &Count) -> String {
    let mut formated_count = format!(
        "  <{}>\n<{}> <{}>\n",
        display_runner(count.bases_occupied, Base::Second),
        display_runner(count.bases_occupied, Base::Third),
        display_runner(count.bases_occupied, Base::First)
    );
    formated_count.push_str(&format!("  <H>\n"));
    formated_count.push_str(&format!("{}: {}\n", t!("out_count"), count.out));
    formated_count.push_str(&format!(
        "{}: {}\n",
        t!("batter"),
        I18nManager::global().full_name(&count.batter.first_name, &count.batter.last_name)
    ));
    let rounded_ba = (count.batter.hit_average() * 1000.0).round();
    formated_count.push_str(&format!(" {} : .{}\n", t!("ba"), rounded_ba));
    let rounded_slg = (count.batter.slg() * 1000.0).round();
    formated_count.push_str(&format!(" {}: .{}\n", t!("slg"), rounded_slg));
    formated_count.push_str(&format!("{}: {}\n", t!("batting_result"), count.result));
    if count.point > 0 {
        formated_count.push_str(&format!("{}: +{}\n", t!("score"), count.point));
    } else {
        formated_count.push_str("");
    }
    formated_count
}

fn display_runner(bases_occupied: u8, base: Base) -> &'static str {
    if is_base_occupied(bases_occupied, base) {
        RUNNER
    } else {
        NO_RUNNER
    }
}

pub fn display_select_game(season: u16) {
    let game_headers_res = APP_CONTEXT
        .get()
        .unwrap()
        .game_repository
        .load_processed_game_headers(season);
    match game_headers_res {
        Ok(game_headers) => {
            let menu_items: Vec<MenuItem<GameHeader>> = game_headers
                .into_iter()
                .map(
                    |GameHeader {
                         id,
                         actual_date,
                         away_team,
                         home_team,
                         game_type,
                         away_points,
                         home_points,
                     }| {
                        let label = format!(
                            "[{}] {} vs {})",
                            actual_date, away_team.name, home_team.name,
                        );

                        MenuItem {
                            label,
                            value: GameHeader {
                                id,
                                actual_date,
                                away_team,
                                home_team,
                                game_type,
                                away_points,
                                home_points,
                            },
                        }
                    },
                )
                .collect();

            let selection = Select::new(&t!("select_game"), menu_items).prompt();

            if let Ok(selected) = selection {
                let game_row_res = APP_CONTEXT
                    .get()
                    .unwrap()
                    .game_repository
                    .load_game_row(&selected.value)
                    .expect(&t!("error", "function" => "load_game_schedules_to_process"));
                display_game_detail(&game_row_res).expect(&t!("screen_io_error"));
            }
        }
        Err(e) => {
            eprintln!(
                "{}:{}",
                t!("error", "function" => "load_processed_game_headers"),
                e
            );
        }
    }
}

pub fn display_select_season() {
    let load_processed_seasons_res = APP_CONTEXT
        .get()
        .unwrap()
        .game_repository
        .load_processed_seasons();
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

pub fn display_batting_results(game: &GameRow) {
    println!("Batting Results:");
    println!("{}", game.away_team.name.to_string());

    let mut _top_results: BTreeMap<String, String> = BTreeMap::new();
    let mut _bottom_results: BTreeMap<String, String> = BTreeMap::new();

    for inning in game.innings.iter() {
        for count in inning.counts.iter() {
            if inning.tb == InningType::Top {
                _top_results
                    .entry(count.batter.last_name.to_string())
                    .and_modify(|e| {
                        e.push_str(SPACE);
                        e.push_str(count.result.to_string().as_str());
                    })
                    .or_insert(count.result.to_string());
            } else {
                _bottom_results
                    .entry(count.batter.last_name.to_string())
                    .and_modify(|e| {
                        e.push_str(SPACE);
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
    println!("{LINE_SEPARATOR}");
    println!("{LINE_SEPARATOR}");
}
