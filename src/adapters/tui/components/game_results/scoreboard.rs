use crate::domain::shared::game_cursor::ScoreBoard;
use crate::t;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Row, Table};

const WALK_OFF: &str = "x";

pub(super) fn draw_scoreboard(frame: &mut Frame, area: Rect, scoreboard: &ScoreBoard) {
    let mut header_cells = vec![Cell::from(t!("team"))];
    for inning_seq in 1..=scoreboard.max_inning_num {
        header_cells.push(Cell::from(
            Line::from(inning_seq.to_string()).alignment(Alignment::Center),
        ));
    }
    header_cells.push(Cell::from(
        Line::from(t!("total_score")).alignment(Alignment::Center),
    ));

    let mut team_name_length = scoreboard.away_team_name.len();
    if scoreboard.home_team_name.len() > team_name_length {
        team_name_length = scoreboard.home_team_name.len()
    }

    let mut away_cells = vec![Cell::from(scoreboard.away_team_name.clone())];
    let mut home_cells = vec![Cell::from(scoreboard.home_team_name.clone())];

    for inning_index in 0..scoreboard.max_inning_num as usize {
        away_cells.push(Cell::from(
            Line::from(
                scoreboard
                    .away_innning_points
                    .get(inning_index)
                    .map(u8::to_string)
                    .unwrap_or_default(),
            )
            .alignment(Alignment::Center),
        ));

        if scoreboard.is_last_bottom_inning_skiped
            && inning_index + 1 == scoreboard.max_inning_num as usize
        {
            home_cells.push(Cell::from(
                Line::from(WALK_OFF).alignment(Alignment::Center),
            ));
        } else {
            home_cells.push(Cell::from(
                Line::from(
                    scoreboard
                        .home_innning_points
                        .get(inning_index)
                        .map(u8::to_string)
                        .unwrap_or_default(),
                )
                .alignment(Alignment::Center),
            ));
        }
    }

    away_cells.push(Cell::from(
        Line::from(scoreboard.away_total_point.to_string()).alignment(Alignment::Center),
    ));
    home_cells.push(Cell::from(
        Line::from(scoreboard.home_total_point.to_string()).alignment(Alignment::Center),
    ));

    let mut widths = vec![Constraint::Min(8)];
    widths.extend(std::iter::repeat_n(
        Constraint::Length(3),
        scoreboard.max_inning_num as usize + 1,
    ));

    let [table_area, _remaining_area] = Layout::horizontal([
        Constraint::Length(
            ((team_name_length as usize + 5) + (scoreboard.max_inning_num as usize * 4) + 4) as u16,
        ),
        Constraint::Min(0),
    ])
    .flex(Flex::Start)
    .areas(area);

    let table = Table::new([Row::new(away_cells), Row::new(home_cells)], widths)
        .header(Row::new(header_cells).style(Style::default().add_modifier(Modifier::BOLD)))
        .block(Block::default().borders(Borders::ALL));

    frame.render_widget(table, table_area);
}
