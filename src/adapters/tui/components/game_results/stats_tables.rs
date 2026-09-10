use crate::domain::shared::game_cursor::{BatterGameStatView, PitcherGameStatView};
use crate::t;
use ratatui::layout::{Constraint, Rect};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Cell, Row, Table};

pub(super) fn draw_batting_stats_table(
    frame: &mut Frame,
    area: Rect,
    team_name: String,
    batting_stats: Vec<BatterGameStatView>,
) {
    let header = Row::new([
        right_aligned_cell("#"),
        Cell::from(t!("pos")),
        Cell::from(t!("player")),
        right_aligned_cell(t!("ab")),
        right_aligned_cell(t!("h")),
        right_aligned_cell(t!("double")),
        right_aligned_cell(t!("triple")),
        right_aligned_cell(t!("hr")),
    ]);
    let rows = batting_stats.into_iter().map(|stat| {
        Row::new([
            right_aligned_cell(stat.batting_order),
            Cell::from(stat.position.to_string()),
            Cell::from(stat.player.full_name()),
            right_aligned_cell(stat.at_bats),
            right_aligned_cell(stat.hits),
            right_aligned_cell(stat.doubles),
            right_aligned_cell(stat.triples),
            right_aligned_cell(stat.home_runs),
        ])
    });
    let widths = [
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Min(8),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(6),
        Constraint::Length(6),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(Block::bordered().title(team_name));

    frame.render_widget(table, area);
}

pub(super) fn draw_pitching_stats_table(
    frame: &mut Frame,
    area: Rect,
    team_name: String,
    pitching_stats: Vec<PitcherGameStatView>,
) {
    let header = Row::new([
        Cell::from(t!("pitcher")),
        right_aligned_cell(t!("pitch_count")),
        right_aligned_cell(t!("innings")),
        right_aligned_cell(t!("ab")),
        right_aligned_cell(t!("h")),
        right_aligned_cell(t!("ra")),
        right_aligned_cell(t!("so")),
        right_aligned_cell(t!("bb")),
        right_aligned_cell(t!("era")),
        right_aligned_cell(t!("whip")),
    ]);
    let rows = pitching_stats.into_iter().map(|stat| {
        Row::new([
            Cell::from(stat.player.full_name()),
            right_aligned_cell(stat.pitch_count),
            right_aligned_cell(stat.innings),
            right_aligned_cell(stat.at_bats),
            right_aligned_cell(stat.hits),
            right_aligned_cell(stat.runs),
            right_aligned_cell(stat.strikeouts),
            right_aligned_cell(stat.walks),
            right_aligned_cell(format!("{:.2}", stat.era)),
            right_aligned_cell(format!("{:.2}", stat.whip)),
        ])
    });
    let widths = [
        Constraint::Min(8),
        Constraint::Length(6),
        Constraint::Length(6),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(6),
        Constraint::Length(5),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(1)
        .block(Block::bordered().title(team_name));

    frame.render_widget(table, area);
}

fn right_aligned_cell(value: impl ToString) -> Cell<'static> {
    Cell::from(Line::from(value.to_string()).alignment(Alignment::Right))
}
