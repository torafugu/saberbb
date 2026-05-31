use super::menu_cui::{init_terminal, restore_terminal};
use crate::APP_CONTEXT;
use crate::domain::statistics_service::StatService;
use crate::i18n::I18nManager;
use crate::rprintln;
use crate::t;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{CellAlignment, Table};
use std::sync::Arc;

pub fn display_standings() {
    let _ = init_terminal();

    let mut table = Table::new();
    println!("{}", t!("standings"));
    let stat_service = StatService {
        repo: APP_CONTEXT.get().unwrap().statistics_repository.clone(),
    };
    let standings_res = stat_service.show_standings();
    match standings_res {
        Ok(standings_res) => {
            table.set_header(vec![
                t!("team"),
                t!("games"),
                t!("wins"),
                t!("losses"),
                t!("draws"),
                t!("pct"),
                t!("gb"),
                t!("r"),
                t!("ra"),
            ]);

            for c in 1..9 {
                table
                    .column_mut(c)
                    .unwrap()
                    .set_cell_alignment(CellAlignment::Right);
            }
            for standing in standings_res {
                table.add_row(vec![
                    standing.team.name,
                    Arc::from(standing.games.to_string()),
                    Arc::from(standing.wins.to_string()),
                    Arc::from(standing.losses.to_string()),
                    Arc::from(standing.draws.to_string()),
                    Arc::from(format!("{:.3}", standing.pct).replace("0.", ".")),
                    Arc::from(format!("{:.1}", standing.gb)),
                    Arc::from(standing.r.to_string()),
                    Arc::from(standing.ra.to_string()),
                ]);
            }
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS);
            rprintln!("{table}");
        }
        Err(e) => {
            eprintln!("{}:{}", t!("error", "function" => "show_standing"), e);
        }
    }
    let _ = restore_terminal();
}

pub fn display_batting_stats() {
    let _ = init_terminal();

    let mut table = Table::new();
    println!("{}", t!("batting_stats"));
    let batting_stats_service = StatService {
        repo: APP_CONTEXT.get().unwrap().statistics_repository.clone(),
    };
    let batting_stats_res = batting_stats_service.show_batting_stats();
    match batting_stats_res {
        Ok(batting_stats_res) => {
            table.set_header(vec![
                t!("batter"),
                t!("ab"),
                t!("single"),
                t!("double"),
                t!("triple"),
                t!("homerun"),
                t!("ba"),
                t!("rbi"),
            ]);

            for c in 1..8 {
                table
                    .column_mut(c)
                    .unwrap()
                    .set_cell_alignment(CellAlignment::Right);
            }
            for batting_stat in batting_stats_res {
                table.add_row(vec![
                    I18nManager::global().full_name(
                        &batting_stat.batter.first_name,
                        &batting_stat.batter.last_name,
                    ),
                    batting_stat.ab.to_string(),
                    batting_stat.single.to_string(),
                    batting_stat.double.to_string(),
                    batting_stat.triple.to_string(),
                    batting_stat.homerun.to_string(),
                    batting_stat.ba.to_string(),
                    batting_stat.rbi.to_string(),
                ]);
            }
            table
                .load_preset(UTF8_FULL)
                .apply_modifier(UTF8_ROUND_CORNERS);
            rprintln!("{table}");
        }
        Err(e) => {
            eprintln!("{}:{}", t!("error", "function" => "show_batting_stats"), e);
        }
    }
    let _ = restore_terminal();
}
