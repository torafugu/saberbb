use super::super::app::Mode;
use super::Component;
use super::batting_stats::BattingStatsWidget;
use super::game_results::GameResultsWidget;
use super::pitching_stats::PitchingStatsWidget;
use super::standings::StandingsWidget;
use crate::adapters::tui::action::{Action, MenuOption};
use crate::adapters::tui::config::Config;
use crate::repositories::schedule_repository::ScheduleRepository;
use crate::{APP_CONTEXT, t};
use color_eyre::eyre::{WrapErr, eyre};
use crossterm::event::KeyEvent;
use ratatui::{prelude::*, widgets::*};
use strum::IntoEnumIterator;
use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

#[derive(Clone, Debug, PartialEq, Eq)]
struct LeagueMenuItem {
    id: u16,
    name: String,
}

#[derive(Default)]
pub struct Home {
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    menu_items: Vec<MenuOption>,
    menu_state: ListState,
    selected_item: Option<MenuOption>,
    leagues: Vec<LeagueMenuItem>,
    league_state: ListState,
    selected_league: Option<LeagueMenuItem>,
    game_results: GameResultsWidget,
    batting_stats: BattingStatsWidget,
    pitching_stats: PitchingStatsWidget,
}

impl Home {
    pub fn new() -> Self {
        info!("Home component started.");

        let mut menu_state = ListState::default();
        menu_state.select(Some(0));
        let mut league_state = ListState::default();
        league_state.select(Some(0));

        Self {
            menu_items: MenuOption::iter().collect(),
            menu_state,
            league_state,
            game_results: GameResultsWidget::new(),
            batting_stats: BattingStatsWidget::new(),
            pitching_stats: PitchingStatsWidget::new(),
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

    fn load_leagues(&mut self) -> color_eyre::Result<()> {
        let app_context = APP_CONTEXT
            .get()
            .ok_or_else(|| eyre!("App context is not initialized"))?;
        let leagues = app_context
            .schedule_repository
            .load_all_leagues()
            .wrap_err(t!("error", "function" => "load_all_leagues"))?;

        self.leagues = leagues
            .into_iter()
            .map(|league| LeagueMenuItem {
                id: league.id,
                name: league.name.to_string(),
            })
            .collect();

        self.league_state.select(if self.leagues.is_empty() {
            None
        } else {
            Some(0)
        });
        self.selected_league = self.leagues.first().cloned();

        Ok(())
    }

    fn select_next_league(&mut self) {
        let len = self.leagues.len();
        if len == 0 {
            return;
        }

        let selected = self.league_state.selected().unwrap_or(0);
        self.league_state.select(Some((selected + 1) % len));
    }

    fn select_previous_league(&mut self) {
        let len = self.leagues.len();
        if len == 0 {
            return;
        }

        let selected = self.league_state.selected().unwrap_or(0);
        self.league_state
            .select(Some(selected.checked_sub(1).unwrap_or(len - 1)));
    }

    fn selected_league_item(&self) -> Option<LeagueMenuItem> {
        self.league_state
            .selected()
            .and_then(|selected| self.leagues.get(selected))
            .cloned()
    }

    fn confirm_league_selection(&mut self) {
        if let Some(league) = self.selected_league_item() {
            self.selected_league = Some(league);
        }
    }

    fn selected_league_name(&self) -> String {
        self.selected_league
            .as_ref()
            .map(|league| league.name.clone())
            .unwrap_or_else(|| t!("all_leagues"))
    }

    fn detail_title(&self, title: String) -> String {
        format!("{}: {}", title, self.selected_league_name())
    }

    fn detail_text(&self) -> String {
        self.selected_item
            .map(|item| format!("Selected: {item}"))
            .unwrap_or_else(|| t!("select_menu"))
    }

    fn draw_league_selector(&mut self, frame: &mut Frame, area: Rect) {
        if self.leagues.is_empty() {
            frame.render_widget(
                Paragraph::new(t!("no_leagues")).block(
                    Block::new()
                        .title(t!("select_league"))
                        .borders(Borders::ALL),
                ),
                area,
            );
            return;
        }

        let leagues: Vec<ListItem> = self
            .leagues
            .iter()
            .map(|league| ListItem::new(league.name.clone()))
            .collect();

        let league_list = List::new(leagues)
            .block(
                Block::new()
                    .title(t!("select_league"))
                    .borders(Borders::ALL),
            )
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(league_list, area, &mut self.league_state);
    }
}

impl Component for Home {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> color_eyre::Result<()> {
        self.command_tx = Some(tx.clone());
        self.game_results.register_action_handler(tx.clone())?;
        self.batting_stats.register_action_handler(tx.clone())?;
        self.pitching_stats.register_action_handler(tx)?;
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> color_eyre::Result<()> {
        self.config = config.clone();
        self.game_results.register_config_handler(config.clone())?;
        self.batting_stats.register_config_handler(config.clone())?;
        self.pitching_stats.register_config_handler(config)?;
        Ok(())
    }

    fn init(&mut self, area: Size) -> color_eyre::Result<()> {
        self.load_leagues()?;
        self.game_results.init(area)?;
        self.batting_stats.init(area)?;
        self.pitching_stats.init(area)?;
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

        if matches!(self.selected_item, Some(MenuOption::ViewBattingStat)) {
            if self
                .config
                .keybindings
                .0
                .get(&Mode::Home)
                .is_some_and(|keymap| keymap.contains_key(&vec![key]))
            {
                return Ok(None);
            }

            return self.batting_stats.handle_key_event(key);
        }

        if matches!(self.selected_item, Some(MenuOption::ViewPitchingStat)) {
            if self
                .config
                .keybindings
                .0
                .get(&Mode::Home)
                .is_some_and(|keymap| keymap.contains_key(&vec![key]))
            {
                return Ok(None);
            }

            return self.pitching_stats.handle_key_event(key);
        }

        Ok(None)
    }

    fn update(&mut self, action: Action) -> color_eyre::Result<Option<Action>> {
        if matches!(self.selected_item, Some(MenuOption::ViewGameResults)) {
            if matches!(action, Action::Back) && self.game_results.is_at_root() {
                self.selected_item = None;
                return Ok(Some(Action::Render));
            }

            if matches!(
                action,
                Action::SelectNext
                    | Action::SelectPrevious
                    | Action::ConfirmSelection
                    | Action::Back
                    | Action::SelectGameDetailTab(_)
                    | Action::NextCount
                    | Action::PreviousCount
            ) {
                return self.game_results.update(action);
            }
        }

        if matches!(self.selected_item, Some(MenuOption::ViewBattingStat))
            && matches!(
                action,
                Action::SelectNext | Action::SelectPrevious | Action::SelectGameDetailTab(_)
            )
        {
            return self.batting_stats.update(action);
        }

        if matches!(self.selected_item, Some(MenuOption::ViewPitchingStat))
            && matches!(
                action,
                Action::SelectNext | Action::SelectPrevious | Action::SelectGameDetailTab(_)
            )
        {
            return self.pitching_stats.update(action);
        }

        if matches!(self.selected_item, Some(MenuOption::SelectLeague)) {
            match action {
                Action::SelectNext => {
                    self.select_next_league();
                    return Ok(Some(Action::Render));
                }
                Action::SelectPrevious => {
                    self.select_previous_league();
                    return Ok(Some(Action::Render));
                }
                Action::ConfirmSelection => {
                    self.confirm_league_selection();
                    return Ok(Some(Action::Render));
                }
                Action::Back => {
                    self.selected_item = None;
                    return Ok(Some(Action::Render));
                }
                _ => {}
            }
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
            Action::Back if matches!(self.selected_item, Some(MenuOption::ViewBattingStat)) => {
                self.selected_item = None;
                Ok(Some(Action::Render))
            }
            Action::Back if matches!(self.selected_item, Some(MenuOption::ViewPitchingStat)) => {
                self.selected_item = None;
                Ok(Some(Action::Render))
            }
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
            .block(Block::new().title(t!("main_menu")).borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(menu, layout[0], &mut self.menu_state);

        match self.selected_item {
            Some(MenuOption::SelectLeague) => self.draw_league_selector(frame, layout[1]),
            Some(MenuOption::ViewStandings) => frame.render_widget(
                StandingsWidget::new(self.detail_title(t!("standings"))),
                layout[1],
            ),
            Some(MenuOption::ViewGameResults) => {
                self.game_results
                    .set_title(self.detail_title(t!("game_results")));
                self.game_results.draw(frame, layout[1])?
            }
            Some(MenuOption::ViewBattingStat) => {
                self.batting_stats
                    .set_title(self.detail_title(t!("batting_stats")));
                self.batting_stats.draw(frame, layout[1])?
            }
            Some(MenuOption::ViewPitchingStat) => {
                self.pitching_stats
                    .set_title(self.detail_title(t!("pitching_stats")));
                self.pitching_stats.draw(frame, layout[1])?
            }
            _ => {
                let details = self.detail_text();
                frame.render_widget(
                    Paragraph::new(details).block(Block::new().borders(Borders::ALL)),
                    layout[1],
                );
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn back_exits_game_results_when_at_root() {
        let mut home = Home::new();
        home.selected_item = Some(MenuOption::ViewGameResults);

        let action = home.update(Action::Back).unwrap();

        assert_eq!(home.selected_item, None);
        assert_eq!(action, Some(Action::Render));
    }

    #[test]
    fn confirm_selection_updates_selected_league() {
        let mut home = Home::new();
        home.selected_item = Some(MenuOption::SelectLeague);
        home.leagues = vec![
            LeagueMenuItem {
                id: 1,
                name: "Central".to_string(),
            },
            LeagueMenuItem {
                id: 2,
                name: "Pacific".to_string(),
            },
        ];
        home.league_state.select(Some(1));

        let action = home.update(Action::ConfirmSelection).unwrap();

        assert_eq!(
            home.selected_league.as_ref().map(|league| league.id),
            Some(2)
        );
        assert_eq!(home.selected_league_name(), "Pacific");
        assert_eq!(action, Some(Action::Render));
    }

    #[test]
    fn back_exits_league_selection() {
        let mut home = Home::new();
        home.selected_item = Some(MenuOption::SelectLeague);

        let action = home.update(Action::Back).unwrap();

        assert_eq!(home.selected_item, None);
        assert_eq!(action, Some(Action::Render));
    }
}
