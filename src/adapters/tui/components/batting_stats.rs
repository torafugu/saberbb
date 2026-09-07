use super::Component;
use crate::adapters::tui::action::Action;
use crate::adapters::tui::config::Config;
use crate::domain::shared::stat::BattingStats;
use crate::domain::statistics_service::StatService;
use crate::{APP_CONTEXT, t};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Default, Debug, Clone, Copy)]
enum BattingStatsTab {
    #[default]
    AtBats,
    HomeRuns,
    RunsBattedIn,
}

impl BattingStatsTab {
    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::AtBats),
            1 => Some(Self::HomeRuns),
            2 => Some(Self::RunsBattedIn),
            _ => None,
        }
    }

    fn selected_index(self) -> usize {
        match self {
            Self::AtBats => 0,
            Self::HomeRuns => 1,
            Self::RunsBattedIn => 2,
        }
    }
}

#[derive(Default)]
pub struct BattingStatsWidget {
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    selected_tab: BattingStatsTab,
    scroll_offset: usize,
}

impl BattingStatsWidget {
    pub fn new() -> Self {
        Self::default()
    }

    fn right_cell(content: String) -> Cell<'static> {
        Cell::from(Text::from(content).right_aligned())
    }

    fn load_batting_stats() -> Result<Vec<BattingStats>> {
        let app_context = APP_CONTEXT
            .get()
            .context("App context is not initialized")?;
        let stat_service = StatService {
            repo: app_context.statistics_repository.clone(),
        };

        stat_service.show_batting_stats()
    }

    fn sorted_batting_stats(&self) -> Result<Vec<BattingStats>> {
        let mut batting_stats = Self::load_batting_stats()?;
        match self.selected_tab {
            BattingStatsTab::AtBats => batting_stats.sort_by(|a, b| {
                b.ab.cmp(&a.ab)
                    .then_with(|| b.homerun.cmp(&a.homerun))
                    .then_with(|| b.rbi.total_cmp(&a.rbi))
                    .then_with(|| a.batter.full_name().cmp(&b.batter.full_name()))
            }),
            BattingStatsTab::HomeRuns => batting_stats.sort_by(|a, b| {
                b.homerun
                    .cmp(&a.homerun)
                    .then_with(|| b.rbi.total_cmp(&a.rbi))
                    .then_with(|| b.ab.cmp(&a.ab))
                    .then_with(|| a.batter.full_name().cmp(&b.batter.full_name()))
            }),
            BattingStatsTab::RunsBattedIn => batting_stats.sort_by(|a, b| {
                b.rbi
                    .total_cmp(&a.rbi)
                    .then_with(|| b.homerun.cmp(&a.homerun))
                    .then_with(|| b.ab.cmp(&a.ab))
                    .then_with(|| a.batter.full_name().cmp(&b.batter.full_name()))
            }),
        }
        Ok(batting_stats)
    }

    fn max_scroll_offset(&self, row_count: usize, table_area: Rect) -> usize {
        let visible_rows = usize::from(table_area.height.saturating_sub(1));
        row_count.saturating_sub(visible_rows)
    }

    fn clamp_scroll_offset(&mut self, row_count: usize, table_area: Rect) {
        self.scroll_offset = self
            .scroll_offset
            .min(self.max_scroll_offset(row_count, table_area));
    }

    fn scroll_next(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    fn scroll_previous(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    fn draw_table(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        batting_stats: Vec<BattingStats>,
    ) -> color_eyre::Result<()> {
        if batting_stats.is_empty() {
            frame.render_widget(Paragraph::new(t!("no_batting_stats")), area);
            return Ok(());
        }

        let row_count = batting_stats.len();
        self.clamp_scroll_offset(row_count, area);

        let rows = batting_stats.into_iter().map(|stat| {
            let hits = stat.single + stat.double + stat.triple + stat.homerun;
            Row::new(vec![
                Cell::from(stat.batter.full_name()),
                Self::right_cell(stat.ab.to_string()),
                Self::right_cell(hits.to_string()),
                Self::right_cell(stat.single.to_string()),
                Self::right_cell(stat.double.to_string()),
                Self::right_cell(stat.triple.to_string()),
                Self::right_cell(stat.homerun.to_string()),
                Self::right_cell(format!("{:.3}", stat.ba).replace("0.", ".")),
                Self::right_cell(format!("{:.0}", stat.rbi)),
            ])
        });

        let header = Row::new(vec![
            Cell::from(t!("player")),
            Self::right_cell(t!("ab")),
            Self::right_cell(t!("h")),
            Self::right_cell(t!("single")),
            Self::right_cell(t!("double")),
            Self::right_cell(t!("triple")),
            Self::right_cell(t!("hr")),
            Self::right_cell(t!("ba")),
            Self::right_cell(t!("rbi")),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));

        let widths = [
            Constraint::Min(10),
            Constraint::Length(4),
            Constraint::Length(4),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(6),
            Constraint::Length(6),
        ];

        let mut table_state = TableState::new().with_offset(self.scroll_offset);
        frame.render_stateful_widget(
            Table::new(rows, widths).header(header).column_spacing(1),
            area,
            &mut table_state,
        );

        let mut scrollbar_state = ScrollbarState::new(row_count)
            .viewport_content_length(usize::from(area.height.saturating_sub(1)))
            .position(table_state.offset());
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );

        Ok(())
    }
}

impl Component for BattingStatsWidget {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> color_eyre::Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> color_eyre::Result<()> {
        self.config = config;
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> color_eyre::Result<Option<Action>> {
        match key.code {
            KeyCode::Char('1') => Ok(Some(Action::SelectGameDetailTab(0))),
            KeyCode::Char('2') => Ok(Some(Action::SelectGameDetailTab(1))),
            KeyCode::Char('3') => Ok(Some(Action::SelectGameDetailTab(2))),
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action) -> color_eyre::Result<Option<Action>> {
        match action {
            Action::SelectGameDetailTab(index) => {
                if let Some(tab) = BattingStatsTab::from_index(index) {
                    self.selected_tab = tab;
                    self.scroll_offset = 0;
                }
                Ok(Some(Action::Render))
            }
            Action::SelectNext => {
                self.scroll_next();
                Ok(Some(Action::Render))
            }
            Action::SelectPrevious => {
                self.scroll_previous();
                Ok(Some(Action::Render))
            }
            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let block = Block::new()
            .title(t!("batting_stats"))
            .borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(inner);

        let tabs = Tabs::new(vec![
            Line::from(format!("{}(1)", t!("ab"))),
            Line::from(format!("{}(2)", t!("hr"))),
            Line::from(format!("{}(3)", t!("rbi"))),
        ])
        .select(self.selected_tab.selected_index())
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::new().borders(Borders::BOTTOM));

        frame.render_widget(tabs, layout[0]);

        match self.sorted_batting_stats() {
            Ok(batting_stats) => self.draw_table(frame, layout[1], batting_stats)?,
            Err(err) => frame.render_widget(Paragraph::new(format!("Error: {err}")), layout[1]),
        };

        Ok(())
    }
}
