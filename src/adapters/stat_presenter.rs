use crate::domain::stat_service::StatService;
use crate::repositories::persistence_config::get_db_conn;
use crate::repositories::stat_repository::SqlStatRepository;
use crate::t;
use comfy_table::modifiers::UTF8_ROUND_CORNERS;
use comfy_table::presets::UTF8_FULL;
use comfy_table::{CellAlignment, Table};
use std::sync::Arc;

pub fn display_standings() {
    let mut table = Table::new();

    println!("{}", t!("standings"));
    let db_repo = SqlStatRepository {
        pool: get_db_conn().unwrap(),
    };
    let stat_service = StatService { repo: db_repo };
    let standings_res = stat_service.show_standing();
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
            println!("{table}");
        }
        Err(e) => {
            eprintln!("{}:{}", t!("error", "function" => "show_standing"), e);
        }
    }
}
