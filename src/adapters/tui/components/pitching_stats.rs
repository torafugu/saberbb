use super::Component;
use crate::adapters::tui::action::Action;
use crate::adapters::tui::config::Config;
use crate::domain::shared::stats::PitchingStats;
use crate::domain::statistics_service::StatService;
use crate::{APP_CONTEXT, t};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Default, Debug, Clone, Copy)]
enum PitchingStatsTab {
    #[default]
    Games,
    Innings,
    EarnedRunAverage,
}

impl PitchingStatsTab {
    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Games),
            1 => Some(Self::Innings),
            2 => Some(Self::EarnedRunAverage),
            _ => None,
        }
    }

    fn selected_index(self) -> usize {
        match self {
            Self::Games => 0,
            Self::Innings => 1,
            Self::EarnedRunAverage => 2,
        }
    }
}

#[derive(Default)]
pub struct PitchingStatsWidget {
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    title: String,
    selected_tab: PitchingStatsTab,
    scroll_offset: usize,
}

impl PitchingStatsWidget {
    pub fn new() -> Self {
        Self {
            title: t!("pitching_stats"),
            ..Default::default()
        }
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    fn right_cell(content: String) -> Cell<'static> {
        Cell::from(Text::from(content).right_aligned())
    }

    fn load_pitching_stats() -> Result<Vec<PitchingStats>> {
        let app_context = APP_CONTEXT
            .get()
            .context("App context is not initialized")?;
        let stat_service = StatService {
            repo: app_context.statistics_repository.clone(),
        };

        stat_service.show_pitching_stats()
    }

    fn sorted_pitching_stats(&self) -> Result<Vec<PitchingStats>> {
        let mut pitching_stats = Self::load_pitching_stats()?;
        match self.selected_tab {
            PitchingStatsTab::Games => pitching_stats.sort_by(|a, b| {
                b.games
                    .cmp(&a.games)
                    .then_with(|| b.innings.cmp(&a.innings))
                    .then_with(|| a.era.cmp(&b.era))
                    .then_with(|| a.batter.full_name().cmp(&b.batter.full_name()))
            }),
            PitchingStatsTab::Innings => pitching_stats.sort_by(|a, b| {
                b.innings
                    .cmp(&a.innings)
                    .then_with(|| b.games.cmp(&a.games))
                    .then_with(|| a.era.cmp(&b.era))
                    .then_with(|| a.batter.full_name().cmp(&b.batter.full_name()))
            }),
            PitchingStatsTab::EarnedRunAverage => pitching_stats.sort_by(|a, b| {
                a.era
                    .cmp(&b.era)
                    .then_with(|| b.innings.cmp(&a.innings))
                    .then_with(|| b.games.cmp(&a.games))
                    .then_with(|| a.batter.full_name().cmp(&b.batter.full_name()))
            }),
        }
        Ok(pitching_stats)
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
        pitching_stats: Vec<PitchingStats>,
    ) -> color_eyre::Result<()> {
        if pitching_stats.is_empty() {
            frame.render_widget(Paragraph::new(t!("no_pitching_stats")), area);
            return Ok(());
        }

        let row_count = pitching_stats.len();
        self.clamp_scroll_offset(row_count, area);
        let layout = Layout::horizontal([Constraint::Min(0), Constraint::Length(1)]).split(area);
        let table_area = layout[0];
        let scrollbar_area = layout[1];

        let rows = pitching_stats.into_iter().map(|stat| {
            Row::new(vec![
                Cell::from(stat.batter.full_name()),
                Self::right_cell(stat.games.to_string()),
                Self::right_cell(stat.wins.to_string()),
                Self::right_cell(stat.losses.to_string()),
                Self::right_cell(stat.saves.to_string()),
                Self::right_cell(stat.holds.to_string()),
                Self::right_cell(stat.era.to_string()),
                Self::right_cell(stat.innings.to_string()),
            ])
        });

        let header = Row::new(vec![
            Cell::from(t!("pitcher")),
            Self::right_cell(t!("games")),
            Self::right_cell(t!("wins")),
            Self::right_cell(t!("losses")),
            Self::right_cell(t!("saves")),
            Self::right_cell(t!("holds")),
            Self::right_cell(t!("era")),
            Self::right_cell(t!("innings")),
        ])
        .style(Style::default().add_modifier(Modifier::BOLD));

        let widths = [
            Constraint::Min(9),
            Constraint::Length(6),
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(6),
        ];

        let mut table_state = TableState::new().with_offset(self.scroll_offset);
        frame.render_stateful_widget(
            Table::new(rows, widths).header(header).column_spacing(1),
            table_area,
            &mut table_state,
        );

        let mut scrollbar_state = ScrollbarState::new(row_count)
            .viewport_content_length(usize::from(table_area.height.saturating_sub(1)))
            .position(table_state.offset());
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            scrollbar_area.inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::{Player, PlayerInfo};
    use ratatui::{Terminal, backend::TestBackend};

    fn pitching_stat(id: i64, first_name: &str, last_name: &str, innings: u16) -> PitchingStats {
        PitchingStats {
            batter: Player::from_player_info(PlayerInfo::new_min(
                id,
                first_name.to_string(),
                last_name.to_string(),
            )),
            games: 1,
            innings,
            wins: 0,
            losses: 0,
            saves: 0,
            holds: 0,
            era: 0,
            so: 0,
            bb: 0,
        }
    }

    fn buffer_lines(buffer: &Buffer) -> Vec<String> {
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer.cell((x, y)).map(|cell| cell.symbol()).unwrap_or(" "))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn draw_table_keeps_innings_visible_next_to_scrollbar() {
        let backend = TestBackend::new(72, 6);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut widget = PitchingStatsWidget::new();
        let stats = vec![
            pitching_stat(1, "Two", "Digits", 18),
            pitching_stat(2, "One", "Digit", 9),
            pitching_stat(3, "Other", "Digit", 8),
        ];

        terminal
            .draw(|frame| {
                widget
                    .draw_table(frame, frame.area(), stats)
                    .expect("pitching stats table should render");
            })
            .unwrap();

        let lines = buffer_lines(terminal.backend().buffer());
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Two Digits") && line.contains("18"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("One Digit") && line.contains("9"))
        );
        assert!(
            lines
                .iter()
                .any(|line| line.contains("Other Digit") && line.contains("8"))
        );
    }
}

impl Component for PitchingStatsWidget {
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
                if let Some(tab) = PitchingStatsTab::from_index(index) {
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
        let block = Block::new().title(self.title.clone()).borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(inner);

        let tabs = Tabs::new(vec![
            Line::from(format!("{}(1)", t!("games"))),
            Line::from(format!("{}(2)", t!("innings"))),
            Line::from(format!("{}(3)", t!("era"))),
        ])
        .select(self.selected_tab.selected_index())
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::new().borders(Borders::BOTTOM));

        frame.render_widget(tabs, layout[0]);

        match self.sorted_pitching_stats() {
            Ok(pitching_stats) => self.draw_table(frame, layout[1], pitching_stats)?,
            Err(err) => frame.render_widget(Paragraph::new(format!("Error: {err}")), layout[1]),
        };

        Ok(())
    }
}
