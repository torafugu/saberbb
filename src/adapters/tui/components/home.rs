use super::super::app::Mode;
use super::Component;
use super::game_results::GameResultsWidget;
use super::standings::StandingsWidget;
use crate::adapters::tui::action::{Action, MenuOption};
use crate::adapters::tui::config::Config;
use crate::t;
use crossterm::event::KeyEvent;
use ratatui::{prelude::*, widgets::*};
use strum::IntoEnumIterator;
use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

#[derive(Default)]
pub struct Home {
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    menu_items: Vec<MenuOption>,
    menu_state: ListState,
    selected_item: Option<MenuOption>,
    game_results: GameResultsWidget,
}

impl Home {
    pub fn new() -> Self {
        info!("Home component started.");

        let mut menu_state = ListState::default();
        menu_state.select(Some(0));

        Self {
            menu_items: MenuOption::iter().collect(),
            menu_state,
            game_results: GameResultsWidget::new(),
            ..Default::default()
        }
    }

    fn select_next(&mut self) {
        let len = self.menu_items.len();
        let selected = self.menu_state.selected().unwrap_or(0);
        self.menu_state.select(Some((selected + 1) % len));
    }

    fn select_previous(&mut self) {
        let len = self.menu_items.len();
        let selected = self.menu_state.selected().unwrap_or(0);
        self.menu_state
            .select(Some(selected.checked_sub(1).unwrap_or(len - 1)));
    }

    fn selected_menu_item(&self) -> Option<MenuOption> {
        self.menu_state
            .selected()
            .and_then(|selected| self.menu_items.get(selected))
            .copied()
    }

    fn detail_text(&self) -> String {
        self.selected_item
            .map(|item| format!("Selected: {item}"))
            .unwrap_or_else(|| t!("select_menu"))
    }
}

impl Component for Home {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> color_eyre::Result<()> {
        self.command_tx = Some(tx.clone());
        self.game_results.register_action_handler(tx)?;
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> color_eyre::Result<()> {
        self.config = config.clone();
        self.game_results.register_config_handler(config)?;
        Ok(())
    }

    fn init(&mut self, area: Size) -> color_eyre::Result<()> {
        self.game_results.init(area)?;
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> color_eyre::Result<Option<Action>> {
        if matches!(self.selected_item, Some(MenuOption::ViewGameResults)) {
            if self
                .config
                .keybindings
                .0
                .get(&Mode::Home)
                .is_some_and(|keymap| keymap.contains_key(&vec![key]))
            {
                return Ok(None);
            }

            return self.game_results.handle_key_event(key);
        }

        Ok(None)
    }

    fn update(&mut self, action: Action) -> color_eyre::Result<Option<Action>> {
        if matches!(self.selected_item, Some(MenuOption::ViewGameResults))
            && matches!(
                action,
                Action::SelectNext
                    | Action::SelectPrevious
                    | Action::ConfirmSelection
                    | Action::Back
                    | Action::SelectGameDetailTab(_)
                    | Action::NextCount
                    | Action::PreviousCount
            )
        {
            return self.game_results.update(action);
        }

        match action {
            Action::Render => {
                // add any logic here that should run on every render
                Ok(None)
            }
            Action::SelectNext => {
                self.select_next();
                Ok(Some(Action::Render))
            }
            Action::SelectPrevious => {
                self.select_previous();
                Ok(Some(Action::Render))
            }
            Action::ConfirmSelection => Ok(self.selected_menu_item().map(Action::MenuItemSelected)),
            Action::MenuItemSelected(item) => {
                self.selected_item = Some(item);
                Ok(Some(Action::Render))
            }
            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let layout = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(vec![Constraint::Percentage(25), Constraint::Percentage(75)])
            .split(area);

        let menu_items: Vec<ListItem> = self
            .menu_items
            .iter()
            .map(|item| ListItem::new(item.to_string()))
            .collect();

        let menu = List::new(menu_items)
            .block(Block::new().title("Menu").borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(menu, layout[0], &mut self.menu_state);

        match self.selected_item {
            Some(MenuOption::ViewStandings) => frame.render_widget(StandingsWidget, layout[1]),
            Some(MenuOption::ViewGameResults) => self.game_results.draw(frame, layout[1])?,
            _ => {
                let details = self.detail_text();
                frame.render_widget(
                    Paragraph::new(details)
                        .block(Block::new().title("Details").borders(Borders::ALL)),
                    layout[1],
                );
            }
        }
        Ok(())
    }
}
