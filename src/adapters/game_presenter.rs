use super::menu_component::{MenuItem, init_terminal, restore_terminal};
use crate::domain::game_service::GameRepository;
use crate::domain::game_service::GameService;
use crate::domain::shared::game::{Count, Game};
use crate::domain::shared::types::InningType;
use crate::repositories::game_repository::SqlGameRepository;
use crate::repositories::persistence_config::get_db_conn;
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

pub fn display_game_detail(game: &Game) -> io::Result<()> {
    let mut inning_seq: i8 = 1;
    let mut inning_tb: InningType = InningType::Top;
    let mut count_seq: i8 = 1;

    let mut current_inning = game
        .innings
        .iter()
        .find(|i| i.is(inning_seq, inning_tb))
        .expect(&t!("inning_not_found"));
    let mut current_count;

    let mut stdout = io::stdout();

    let _ = init_terminal();
    let mut should_redraw = true;

    loop {
        if should_redraw {
            let mut table = Table::new();
            execute!(stdout, cursor::MoveTo(0, 0))?;

            let max_inning_seq = game.innings.iter().map(|i| i.seq).max().unwrap_or(0);
            current_inning = game
                .innings
                .iter()
                .find(|i| i.is(inning_seq, inning_tb))
                .expect(&t!("inning_not_found"));

            current_count = current_inning
                .counts
                .iter()
                .find(|i| i.seq == count_seq)
                .expect(&t!("count_not_found"));

            rprintln!("<- {} {} ->", t!("prev_count"), t!("next_count"));
            rprintln!("\r\n");
            rprintln!(
                "<{}> {}:{}({}) {}:{}",
                game.game_type,
                t!("current_inning"),
                current_inning.seq,
                current_inning.tb.to_string(),
                t!("current_count"),
                current_count.seq
            );

            // Display score board
            let mut headers: Vec<String> = Vec::new();
            let mut top_scores: Vec<String> = Vec::new();
            let mut bottom_scores: Vec<String> = Vec::new();
            headers.push(t!("team"));
            top_scores.push((&game.away_team.name).to_string());
            bottom_scores.push((&game.home_team.name).to_string());

            for inning_num in 1..max_inning_seq + 1 {
                headers.push(inning_num.to_string());
                top_scores.push("".to_string());
                bottom_scores.push("".to_string());
            }
            headers.push(t!("total_score"));
            top_scores.push("".to_string());
            bottom_scores.push("".to_string());

            let mut top_total_point: i8 = 0;
            let mut bottom_total_point: i8 = 0;

            'inning: for inning in &game.innings {
                let mut top_inning_point = 0;
                let mut bottom_inning_point = 0;
                for count in &inning.counts {
                    if inning.tb == InningType::Top {
                        top_inning_point += count.point;
                        top_total_point += count.point;
                        top_scores[inning.seq as usize] = (top_inning_point).to_string();
                    } else {
                        bottom_inning_point += count.point;
                        bottom_total_point += count.point;
                        bottom_scores[inning.seq as usize] = (bottom_inning_point).to_string();
                    }
                    if inning.seq == inning_seq && inning.tb == inning_tb && count.seq == count_seq
                    {
                        break 'inning;
                    }
                }
            }

            if inning_seq == max_inning_seq
                && count_seq == (current_inning.counts.len()) as i8
                && bottom_scores[max_inning_seq as usize] == ""
            {
                bottom_scores[max_inning_seq as usize] = WALK_OFF.to_string();
            }

            top_scores[(max_inning_seq + 1) as usize] = top_total_point.to_string();
            bottom_scores[(max_inning_seq + 1) as usize] = bottom_total_point.to_string();

            table.set_header(headers);
            table.add_row(top_scores);
            table.add_row(bottom_scores);
            for inning_num in 1..max_inning_seq + 2 {
                table
                    .column_mut(inning_num as usize)
                    .unwrap()
                    .set_cell_alignment(CellAlignment::Right);
            }
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS);

            rprintln!("{table}");

            // Display count
            rprintln!("{}", format_count(&current_count));
            should_redraw = false;
        }

        if event::poll(std::time::Duration::from_millis(100))? {
            if let Event::Key(key_event) = event::read()? {
                match key_event {
                    KeyEvent {
                        code: KeyCode::Left,
                        ..
                    } => {
                        if count_seq > 1 {
                            count_seq -= 1;
                            should_redraw = true;
                        } else if current_inning.tb == InningType::Bottom {
                            inning_tb = InningType::Top;
                            count_seq = game
                                .innings
                                .iter()
                                .find(|i| i.is(inning_seq, inning_tb))
                                .expect(&t!("inning_not_found"))
                                .counts
                                .len() as i8;
                            should_redraw = true;
                        } else if let Some(_) = game
                            .innings
                            .iter()
                            .find(|i| i.is(inning_seq - 1, InningType::Top))
                        {
                            inning_seq -= 1;
                            inning_tb = InningType::Bottom;
                            count_seq = game
                                .innings
                                .iter()
                                .find(|i| i.is(inning_seq, inning_tb))
                                .expect(&t!("inning_not_found"))
                                .counts
                                .len() as i8;
                            should_redraw = true;
                        }
                    }
                    KeyEvent {
                        code: KeyCode::Right,
                        ..
                    } => {
                        if count_seq < current_inning.counts.len() as i8 {
                            count_seq += 1;
                            should_redraw = true;
                        } else if current_inning.tb == InningType::Top
                            && let Some(_) = game
                                .innings
                                .iter()
                                .find(|i| i.is(inning_seq, InningType::Bottom))
                        {
                            inning_tb = InningType::Bottom;
                            count_seq = 1;
                            should_redraw = true;
                        } else if let Some(_) = game
                            .innings
                            .iter()
                            .find(|i| i.is(inning_seq + 1, InningType::Top))
                        {
                            inning_seq += 1;
                            inning_tb = InningType::Top;
                            count_seq = 1;
                            should_redraw = true;
                        }
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

fn format_count(count: &Count) -> String {
    let mut formated_count = format!("  <{}>\n", display_runner(count.bases.second));
    formated_count.push_str(&format!(
        "<{}> <{}>\n",
        display_runner(count.bases.third),
        display_runner(count.bases.first)
    ));
    formated_count.push_str(&format!("  <H>\n"));
    formated_count.push_str(&format!("{}: {}\n", &t!("out_count"), count.out));
    formated_count.push_str(&format!("{}: {}\n", &t!("batter"), count.batter.last_name));
    let rounded_ba = (count.batter.hit_average() * 1000.0).round();
    formated_count.push_str(&format!(" {} : .{}\n", &t!("ba"), rounded_ba));
    let rounded_slg = (count.batter.slg() * 1000.0).round();
    formated_count.push_str(&format!(" {}: .{}\n", &t!("slg"), rounded_slg));
    formated_count.push_str(&format!("{}: {}\n", &t!("batting_result"), count.result));
    if count.point > 0 {
        formated_count.push_str(&format!("{}: +{}\n", &t!("score"), count.point));
    } else {
        formated_count.push_str("");
    }
    formated_count
}

fn display_runner(runner: bool) -> &'static str {
    if runner { RUNNER } else { NO_RUNNER }
}

pub fn display_select_game(season: i16) {
    let db_repo = SqlGameRepository {
        pool: get_db_conn().unwrap(),
    };
    let game_service = GameService { repo: db_repo };
    let game_rounds_res = game_service.repo.load_processed_games(season);
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
                            },
                        }
                    },
                )
                .collect();

            let selection = Select::new(&t!("select_game"), menu_items).prompt();

            if let Ok(selected) = selection {
                let _ = display_game_detail(&selected.value);
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
    let db_repo = SqlGameRepository {
        pool: get_db_conn().unwrap(),
    };
    let game_service = GameService { repo: db_repo };
    let load_processed_seasons_res = game_service.repo.load_processed_seasons();
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

// TODO: Display in Game progress mode
pub fn display_batting_results(game: &Game) {
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
