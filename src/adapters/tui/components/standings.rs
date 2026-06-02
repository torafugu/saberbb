use crate::domain::statistics_service::StatService;
use crate::{APP_CONTEXT, t};
use anyhow::{Context, Result};
use ratatui::{prelude::*, widgets::*};

pub struct StandingsWidget;

impl StandingsWidget {
    fn left_cell(content: String) -> Cell<'static> {
        Cell::from(Text::from(content).left_aligned())
    }

    fn right_cell(content: String) -> Cell<'static> {
        Cell::from(Text::from(content).right_aligned())
    }

    fn load_rows() -> Result<Vec<Vec<String>>> {
        let app_context = APP_CONTEXT
            .get()
            .context("App context is not initialized")?;
        let stat_service = StatService {
            repo: app_context.statistics_repository.clone(),
        };

        let standings = stat_service.show_standings()?;
        Ok(standings
            .into_iter()
            .map(|standing| {
                vec![
                    standing.team.name.to_string(),
                    standing.games.to_string(),
                    standing.wins.to_string(),
                    standing.losses.to_string(),
                    standing.draws.to_string(),
                    format!("{:.3}", standing.pct).replace("0.", "."),
                    format!("{:.1}", standing.gb),
                    standing.r.to_string(),
                    standing.ra.to_string(),
                ]
            })
            .collect())
    }
}

impl Widget for StandingsWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::new().title(t!("standings")).borders(Borders::ALL);
        let inner = block.inner(area);
        block.render(area, buf);

        let rows = match Self::load_rows() {
            Ok(rows) => rows,
            Err(err) => {
                Paragraph::new(format!("Error: {err}")).render(inner, buf);
                return;
            }
        };

        let header = Row::new(vec![
            Cell::from(t!("team")),
            Self::left_cell(t!("games")),
            Self::left_cell(t!("wins")),
            Self::left_cell(t!("losses")),
            Self::left_cell(t!("draws")),
            Self::left_cell(t!("pct")),
            Self::left_cell(t!("gb")),
            Self::left_cell(t!("r")),
            Self::left_cell(t!("ra")),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));

        let rows = rows.into_iter().map(|row| {
            Row::new(vec![
                Cell::from(row[0].clone()),
                Self::right_cell(row[1].clone()),
                Self::right_cell(row[2].clone()),
                Self::right_cell(row[3].clone()),
                Self::right_cell(row[4].clone()),
                Self::right_cell(row[5].clone()),
                Self::right_cell(row[6].clone()),
                Self::right_cell(row[7].clone()),
                Self::right_cell(row[8].clone()),
            ])
        });
        // TODO: Should be adjusted by lang.
        let widths = [
            Constraint::Min(8),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(5),
            Constraint::Length(8),
            Constraint::Length(4),
            Constraint::Length(4),
        ];

        Widget::render(
            Table::new(rows, widths).header(header).column_spacing(1),
            inner,
            buf,
        );
    }
}
