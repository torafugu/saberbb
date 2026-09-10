use crate::domain::shared::game::Count;
use crate::domain::shared::game_cursor::{BatterGameStatView, GameCursor};
use crate::domain::util::ms_to_kmh;
use crate::t;
use ratatui::prelude::*;
use ratatui::style::Color;

const RUNNER: &str = "R";
const NO_RUNNER: &str = "-";

pub(super) fn format_count(count: &Count) -> String {
    let mut formatted_count = format!("{}: {}\n", "B", display_count_number(count.ball));
    formatted_count.push_str(&format!(
        "{}: {}\n",
        "S",
        display_count_number(count.strike)
    ));
    formatted_count.push_str(&format!("{}: {}\n", "O", display_count_number(count.out)));
    formatted_count
}

pub(super) fn format_runner(game_cursor: &GameCursor) -> String {
    format!(
        "  <{}>\n<{}> <{}>\n  <H>\n",
        display_runner(game_cursor.has_runner_on_second()),
        display_runner(game_cursor.has_runner_on_third()),
        display_runner(game_cursor.has_runner_on_first())
    )
}

pub(super) fn format_batter_and_pitcher(
    game_cursor: &mut GameCursor,
) -> color_eyre::Result<String> {
    let pitching_view = game_cursor.current_pitching_view()?;

    let mut formatted_batter_and_pitcher = format!(
        "{}km/h\n{}\n\n",
        ms_to_kmh(pitching_view.ball.speed),
        pitching_view.ball.pitch_type
    );

    if let Some(batting_view) = game_cursor.current_batting_view() {
        formatted_batter_and_pitcher.push_str(&format!(
            "{}: {}\n",
            t!("result"),
            batting_view.outcome()?
        ));

        let ball = &batting_view.ball;
        formatted_batter_and_pitcher.push_str(&format!(
            "{}: {:.0}m\n",
            t!("distance"),
            ball.final_position.distance
        ));
    } else if let Some(running_view) = game_cursor.current_running_view() {
        formatted_batter_and_pitcher.push_str(&format!(
            "\n{}: {}\n",
            t!("running_event"),
            running_view.event
        ));
        formatted_batter_and_pitcher.push_str(&format!(
            "{}: {}\n",
            t!("target_base"),
            running_view.throw_target_base
        ));
        formatted_batter_and_pitcher.push_str(&format!(
            "{}: {}\n",
            t!("ruling"),
            running_view.ruling
        ));

        if let Some(target_runner) = running_view.target_runner {
            formatted_batter_and_pitcher.push_str(&format!(
                "{}: {}\n",
                t!("runner"),
                target_runner.full_name()
            ));
        }
    }

    Ok(formatted_batter_and_pitcher)
}

pub(super) fn format_lineup(game_cursor: &mut GameCursor) -> color_eyre::Result<Text<'static>> {
    let pitcher = game_cursor.current_pitcher()?;
    let catcher = game_cursor.current_catcher()?;
    let fb = game_cursor.current_fb()?;
    let sb = game_cursor.current_sb()?;
    let tb = game_cursor.current_tb()?;
    let ss = game_cursor.current_ss()?;
    let rf = game_cursor.current_rf()?;
    let cf = game_cursor.current_cf()?;
    let lf = game_cursor.current_lf()?;

    let current_batter_id = game_cursor
        .current_batter()
        .ok()
        .map(|batter| batter.info.id);

    let mut formatted_lineup = vec![
        Line::from(format!("({}) {}", t!("p"), pitcher.full_name())),
        Line::from(format!("({}) {}", t!("c"), catcher.full_name())),
        Line::from(format!("({}) {}", t!("fb"), fb.full_name())),
        Line::from(format!("({}) {}", t!("sb"), sb.full_name())),
        Line::from(format!("({}) {}", t!("tb"), tb.full_name())),
        Line::from(format!("({}) {}", t!("ss"), ss.full_name())),
        Line::from(format!("({}) {}", t!("rf"), rf.full_name())),
        Line::from(format!("({}) {}", t!("cf"), cf.full_name())),
        Line::from(format!("({}) {}", t!("lf"), lf.full_name())),
    ];
    formatted_lineup.extend(format_batting_order(
        game_cursor.current_batting_stats_for_team(game_cursor.current_batting_team_id()),
        current_batter_id,
    ));

    Ok(Text::from(formatted_lineup))
}

pub(super) fn format_batting_order(
    batting_order: Vec<BatterGameStatView>,
    current_batter_id: Option<i64>,
) -> Vec<Line<'static>> {
    let mut formatted_batting_order = vec![Line::from(format!(""))];

    for batter in batting_order {
        let line = Line::from(format!(
            "{}. {}",
            batter.batting_order,
            batter.player.full_name()
        ));

        if Some(batter.player.id) == current_batter_id {
            formatted_batting_order.push(line.style(Style::default().fg(Color::Yellow)));
        } else {
            formatted_batting_order.push(line);
        }
    }

    formatted_batting_order
}

fn display_runner(has_runner: bool) -> &'static str {
    if has_runner { RUNNER } else { NO_RUNNER }
}

fn display_count_number(number: u8) -> String {
    let mut count_number = "".to_string();
    for _ in 0..number {
        count_number.push_str("●");
    }
    count_number
}
