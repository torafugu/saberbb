use crate::domain::shared::game::GameHeader;
use ratatui::prelude::*;
use ratatui::style::Color;
use ratatui::widgets::{Block, List, ListItem};

pub(super) fn game_label(game: &GameHeader) -> String {
    format!(
        "[{}] {} {} - {} {} ({})",
        game.actual_date,
        game.away_team.name,
        game.away_points,
        game.home_points,
        game.home_team.name,
        game.game_type
    )
}

pub(super) fn selectable_list(items: Vec<ListItem>, title: String) -> List {
    List::new(items)
        .block(Block::new().title(title))
        .highlight_style(
            Style::default()
                .bg(Color::Blue)
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("> ")
}
