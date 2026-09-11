use super::super::app::Mode;
use super::Component;
use super::batting_stats::BattingStatsWidget;
use super::game_results::GameResultsWidget;
use super::pitching_stats::PitchingStatsWidget;
use super::standings::StandingsWidget;
use crate::adapters::tui::action::{Action, MenuOption};
use crate::adapters::tui::config::Config;
use crate::domain::shared::game::TeamGameScheduleView;
use crate::repositories::game_repository::GameScheduleReader;
use crate::repositories::schedule_repository::ScheduleRepository;
use crate::{APP_CONTEXT, t};
use chrono::{Datelike, Local, NaiveDate};
use color_eyre::eyre::{WrapErr, eyre};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use strum::IntoEnumIterator;
use tokio::sync::mpsc::UnboundedSender;
use tracing::info;

#[derive(Clone, Debug, PartialEq, Eq)]
struct LeagueMenuItem {
    id: u16,
    name: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct TeamMenuItem {
    id: u16,
    league_id: u16,
    name: String,
}

#[derive(Default, Debug, Clone, Copy, PartialEq, Eq)]
enum TeamInfoTab {
    #[default]
    Schedule,
    PlayerInfo,
    PitcherStats,
    BatterStats,
}

impl TeamInfoTab {
    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::Schedule),
            1 => Some(Self::PlayerInfo),
            2 => Some(Self::PitcherStats),
            3 => Some(Self::BatterStats),
            _ => None,
        }
    }

    fn selected_index(self) -> usize {
        match self {
            Self::Schedule => 0,
            Self::PlayerInfo => 1,
            Self::PitcherStats => 2,
            Self::BatterStats => 3,
        }
    }
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
    teams: Vec<TeamMenuItem>,
    team_state: ListState,
    selected_team: Option<TeamMenuItem>,
    selected_team_info_tab: TeamInfoTab,
    team_schedules: Vec<TeamGameScheduleView>,
    team_schedule_state: ListState,
    team_schedule_month: NaiveDate,
    team_schedule_error: Option<String>,
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
        let mut team_state = ListState::default();
        team_state.select(Some(0));
        let mut team_schedule_state = ListState::default();
        team_schedule_state.select(Some(0));
        let team_schedule_month = Self::month_start(Local::now().date_naive());

        Self {
            menu_items: MenuOption::iter().collect(),
            menu_state,
            league_state,
            team_state,
            team_schedule_state,
            team_schedule_month,
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

        self.teams = leagues
            .iter()
            .flat_map(|league| {
                league.teams.iter().map(|team| TeamMenuItem {
                    id: team.id,
                    league_id: league.id,
                    name: team.name.to_string(),
                })
            })
            .collect();

        self.leagues = leagues
            .iter()
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
        self.reset_team_selection();

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
            self.reset_team_selection();
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

    fn visible_teams(&self) -> Vec<TeamMenuItem> {
        match self.selected_league.as_ref() {
            Some(league) => self
                .teams
                .iter()
                .filter(|team| team.league_id == league.id)
                .cloned()
                .collect(),
            None => self.teams.clone(),
        }
    }

    fn reset_team_selection(&mut self) {
        let first_team = self.visible_teams().first().cloned();
        self.team_state.select(first_team.as_ref().map(|_| 0));
        self.selected_team = None;
        self.selected_team_info_tab = TeamInfoTab::default();
        self.team_schedules.clear();
        self.team_schedule_state.select(None);
        self.team_schedule_month = Self::month_start(Local::now().date_naive());
        self.team_schedule_error = None;
    }

    fn select_next_team(&mut self) {
        let len = self.visible_teams().len();
        if len == 0 {
            return;
        }

        let selected = self.team_state.selected().unwrap_or(0);
        self.team_state.select(Some((selected + 1) % len));
    }

    fn select_previous_team(&mut self) {
        let len = self.visible_teams().len();
        if len == 0 {
            return;
        }

        let selected = self.team_state.selected().unwrap_or(0);
        self.team_state
            .select(Some(selected.checked_sub(1).unwrap_or(len - 1)));
    }

    fn selected_team_item(&self) -> Option<TeamMenuItem> {
        self.team_state
            .selected()
            .and_then(|selected| self.visible_teams().get(selected).cloned())
    }

    fn confirm_team_selection(&mut self) {
        if let Some(team) = self.selected_team_item() {
            self.team_schedule_month = Self::month_start(Local::now().date_naive());
            self.load_team_schedule(team.id);
            self.selected_team = Some(team);
            self.selected_team_info_tab = TeamInfoTab::default();
        }
    }

    fn select_team_info_tab(&mut self, index: usize) {
        if let Some(tab) = TeamInfoTab::from_index(index) {
            self.selected_team_info_tab = tab;
        }
    }

    fn month_start(date: NaiveDate) -> NaiveDate {
        NaiveDate::from_ymd_opt(date.year(), date.month(), 1).expect("valid first day of month")
    }

    fn next_month_start(month: NaiveDate) -> NaiveDate {
        if month.month() == 12 {
            NaiveDate::from_ymd_opt(month.year() + 1, 1, 1).expect("valid first day of next year")
        } else {
            NaiveDate::from_ymd_opt(month.year(), month.month() + 1, 1)
                .expect("valid first day of next month")
        }
    }

    fn previous_month_start(month: NaiveDate) -> NaiveDate {
        if month.month() == 1 {
            NaiveDate::from_ymd_opt(month.year() - 1, 12, 1)
                .expect("valid first day of previous year")
        } else {
            NaiveDate::from_ymd_opt(month.year(), month.month() - 1, 1)
                .expect("valid first day of previous month")
        }
    }

    fn team_schedule_month_label(&self) -> String {
        self.team_schedule_month.format("%Y-%m").to_string()
    }

    fn team_schedule_title(&self) -> String {
        format!("{}:{}", t!("schedule"), self.team_schedule_month_label())
    }

    fn team_schedule_month_range(month: NaiveDate) -> (NaiveDate, NaiveDate) {
        (month, Self::next_month_start(month))
    }

    fn load_team_schedules_for_month(
        team_id: u16,
        month: NaiveDate,
    ) -> color_eyre::Result<Vec<TeamGameScheduleView>> {
        let (start_date, end_date) = Self::team_schedule_month_range(month);
        let schedule_res = APP_CONTEXT
            .get()
            .ok_or_else(|| eyre!("App context is not initialized"))
            .map(|app_context| {
                app_context
                    .game_repository
                    .load_team_game_schedules(team_id, start_date, end_date)
            })?;

        schedule_res.map_err(Into::into)
    }

    fn load_team_schedule(&mut self, team_id: u16) {
        let schedule_res = Self::load_team_schedules_for_month(team_id, self.team_schedule_month);

        match schedule_res {
            Ok(schedules) => {
                self.team_schedules = schedules;
                self.team_schedule_state
                    .select(if self.team_schedules.is_empty() {
                        None
                    } else {
                        Some(0)
                    });
                self.team_schedule_error = None;
            }
            Err(err) => {
                self.team_schedules.clear();
                self.team_schedule_state.select(None);
                self.team_schedule_error = Some(format!(
                    "{}: {}",
                    t!("error", "function" => "load_team_game_schedules"),
                    err
                ));
            }
        }
    }

    fn select_next_team_schedule_month(&mut self) {
        let Some(team_id) = self.selected_team.as_ref().map(|team| team.id) else {
            return;
        };

        self.try_select_team_schedule_month(
            Self::next_month_start(self.team_schedule_month),
            team_id,
        );
    }

    fn select_previous_team_schedule_month(&mut self) {
        let Some(team_id) = self.selected_team.as_ref().map(|team| team.id) else {
            return;
        };

        self.try_select_team_schedule_month(
            Self::previous_month_start(self.team_schedule_month),
            team_id,
        );
    }

    fn try_select_team_schedule_month(&mut self, month: NaiveDate, team_id: u16) {
        match Self::load_team_schedules_for_month(team_id, month) {
            Ok(schedules) => {
                self.commit_team_schedule_month_if_present(month, schedules);
            }
            Err(err) => {
                self.team_schedule_error = Some(format!(
                    "{}: {}",
                    t!("error", "function" => "load_team_game_schedules"),
                    err
                ));
            }
        }
    }

    fn commit_team_schedule_month_if_present(
        &mut self,
        month: NaiveDate,
        schedules: Vec<TeamGameScheduleView>,
    ) -> bool {
        if schedules.is_empty() {
            return false;
        }

        self.team_schedule_month = month;
        self.team_schedules = schedules;
        self.team_schedule_state.select(Some(0));
        self.team_schedule_error = None;
        true
    }

    fn select_next_team_schedule(&mut self) {
        let len = self.team_schedules.len();
        if len == 0 {
            return;
        }

        let selected = self.team_schedule_state.selected().unwrap_or(0);
        self.team_schedule_state.select(Some((selected + 1) % len));
    }

    fn select_previous_team_schedule(&mut self) {
        let len = self.team_schedules.len();
        if len == 0 {
            return;
        }

        let selected = self.team_schedule_state.selected().unwrap_or(0);
        self.team_schedule_state
            .select(Some(selected.checked_sub(1).unwrap_or(len - 1)));
    }

    fn team_schedule_label(schedule: &TeamGameScheduleView) -> String {
        match (
            schedule.actual_date,
            schedule.away_points,
            schedule.home_points,
        ) {
            (Some(actual_date), Some(away_points), Some(home_points)) => format!(
                "[{}] {} {} - {} {} ({})",
                actual_date,
                schedule.away_team.name,
                away_points,
                home_points,
                schedule.home_team.name,
                schedule.game_type
            ),
            _ => format!(
                "[{}] {} - {} ({})",
                schedule.planned_date,
                schedule.away_team.name,
                schedule.home_team.name,
                schedule.game_type
            ),
        }
    }

    fn draw_team_selector(&mut self, frame: &mut Frame, area: Rect) {
        let title = self.detail_title(t!("select_team"));
        let teams = self.visible_teams();
        if teams.is_empty() {
            frame.render_widget(
                Paragraph::new(t!("no_teams"))
                    .block(Block::new().title(title).borders(Borders::ALL)),
                area,
            );
            return;
        }

        let team_items: Vec<ListItem> = teams
            .iter()
            .map(|team| ListItem::new(team.name.clone()))
            .collect();

        let team_list = List::new(team_items)
            .block(Block::new().title(title).borders(Borders::ALL))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(team_list, area, &mut self.team_state);
    }

    fn draw_team_info(&mut self, frame: &mut Frame, area: Rect) {
        let Some(team) = self.selected_team.as_ref() else {
            self.draw_team_selector(frame, area);
            return;
        };

        let block = Block::new()
            .title(self.detail_title(t!("team_info")))
            .borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(inner);
        let tabs = Tabs::new(vec![
            Line::from(format!("{}(1)", t!("schedule"))),
            Line::from(format!("{}(2)", t!("player_info"))),
            Line::from(format!("{}(3)", t!("pitcher_stats"))),
            Line::from(format!("{}(4)", t!("batter_stats"))),
        ])
        .select(self.selected_team_info_tab.selected_index())
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::new().borders(Borders::BOTTOM));

        frame.render_widget(tabs, layout[0]);

        match self.selected_team_info_tab {
            TeamInfoTab::Schedule => self.draw_team_schedule_tab(frame, layout[1]),
            TeamInfoTab::PlayerInfo | TeamInfoTab::PitcherStats | TeamInfoTab::BatterStats => {
                frame.render_widget(Paragraph::new(team.name.clone()), layout[1]);
            }
        }
    }

    fn draw_team_schedule_tab(&mut self, frame: &mut Frame, area: Rect) {
        let title = self.team_schedule_title();
        if let Some(error) = &self.team_schedule_error {
            frame.render_widget(
                Paragraph::new(error.as_str()).block(Block::new().title(title)),
                area,
            );
            return;
        }

        if self.team_schedules.is_empty() {
            frame.render_widget(
                Paragraph::new(t!("no_schedules")).block(Block::new().title(title)),
                area,
            );
            return;
        }

        let schedules: Vec<ListItem> = self
            .team_schedules
            .iter()
            .map(|schedule| ListItem::new(Self::team_schedule_label(schedule)))
            .collect();

        let schedule_list = List::new(schedules)
            .block(Block::new().title(title))
            .highlight_style(
                Style::default()
                    .bg(Color::Blue)
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(schedule_list, area, &mut self.team_schedule_state);
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

        if matches!(self.selected_item, Some(MenuOption::ViewTeamInfo))
            && self.selected_team.is_some()
        {
            return match key.code {
                KeyCode::Char('1') => Ok(Some(Action::SelectGameDetailTab(0))),
                KeyCode::Char('2') => Ok(Some(Action::SelectGameDetailTab(1))),
                KeyCode::Char('3') => Ok(Some(Action::SelectGameDetailTab(2))),
                KeyCode::Char('4') => Ok(Some(Action::SelectGameDetailTab(3))),
                _ => Ok(None),
            };
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

        if matches!(self.selected_item, Some(MenuOption::ViewTeamInfo)) {
            match action {
                Action::SelectNext
                    if self.selected_team.is_some()
                        && matches!(self.selected_team_info_tab, TeamInfoTab::Schedule) =>
                {
                    self.select_next_team_schedule();
                    return Ok(Some(Action::Render));
                }
                Action::SelectPrevious
                    if self.selected_team.is_some()
                        && matches!(self.selected_team_info_tab, TeamInfoTab::Schedule) =>
                {
                    self.select_previous_team_schedule();
                    return Ok(Some(Action::Render));
                }
                Action::NextCount
                    if self.selected_team.is_some()
                        && matches!(self.selected_team_info_tab, TeamInfoTab::Schedule) =>
                {
                    self.select_next_team_schedule_month();
                    return Ok(Some(Action::Render));
                }
                Action::PreviousCount
                    if self.selected_team.is_some()
                        && matches!(self.selected_team_info_tab, TeamInfoTab::Schedule) =>
                {
                    self.select_previous_team_schedule_month();
                    return Ok(Some(Action::Render));
                }
                Action::SelectNext if self.selected_team.is_none() => {
                    self.select_next_team();
                    return Ok(Some(Action::Render));
                }
                Action::SelectPrevious if self.selected_team.is_none() => {
                    self.select_previous_team();
                    return Ok(Some(Action::Render));
                }
                Action::ConfirmSelection if self.selected_team.is_none() => {
                    self.confirm_team_selection();
                    return Ok(Some(Action::Render));
                }
                Action::SelectGameDetailTab(index) if self.selected_team.is_some() => {
                    self.select_team_info_tab(index);
                    return Ok(Some(Action::Render));
                }
                Action::Back if self.selected_team.is_some() => {
                    self.selected_team = None;
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
            Some(MenuOption::ViewTeamInfo) => self.draw_team_info(frame, layout[1]),
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
    use crate::domain::shared::game::GameType;
    use crate::domain::shared::team::Team;

    fn team_schedule(
        id: u32,
        planned_date: NaiveDate,
        actual_date: Option<NaiveDate>,
        away_points: Option<u8>,
        home_points: Option<u8>,
    ) -> TeamGameScheduleView {
        TeamGameScheduleView {
            id,
            planned_date,
            actual_date,
            away_team: Team::min(1, "Away"),
            home_team: Team::min(2, "Home"),
            game_type: GameType::Regular,
            away_points,
            home_points,
        }
    }

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

    #[test]
    fn view_team_info_navigates_team_selection() {
        let mut home = Home::new();
        home.selected_item = Some(MenuOption::ViewTeamInfo);
        home.selected_league = Some(LeagueMenuItem {
            id: 1,
            name: "Central".to_string(),
        });
        home.teams = vec![
            TeamMenuItem {
                id: 1,
                league_id: 1,
                name: "Lions".to_string(),
            },
            TeamMenuItem {
                id: 2,
                league_id: 1,
                name: "Tigers".to_string(),
            },
        ];
        home.team_state.select(Some(0));

        let action = home.update(Action::SelectNext).unwrap();

        assert_eq!(home.team_state.selected(), Some(1));
        assert_eq!(action, Some(Action::Render));
    }

    #[test]
    fn confirm_team_selection_updates_selected_team() {
        let mut home = Home::new();
        home.selected_item = Some(MenuOption::ViewTeamInfo);
        home.selected_league = Some(LeagueMenuItem {
            id: 1,
            name: "Central".to_string(),
        });
        home.teams = vec![
            TeamMenuItem {
                id: 1,
                league_id: 1,
                name: "Lions".to_string(),
            },
            TeamMenuItem {
                id: 2,
                league_id: 1,
                name: "Tigers".to_string(),
            },
        ];
        home.team_state.select(Some(1));

        let action = home.update(Action::ConfirmSelection).unwrap();

        assert_eq!(home.selected_team.as_ref().map(|team| team.id), Some(2));
        assert_eq!(home.selected_team_info_tab, TeamInfoTab::Schedule);
        assert_eq!(action, Some(Action::Render));
    }

    #[test]
    fn team_selection_reset_does_not_preselect_team_info() {
        let mut home = Home::new();
        home.selected_league = Some(LeagueMenuItem {
            id: 1,
            name: "Central".to_string(),
        });
        home.teams = vec![TeamMenuItem {
            id: 1,
            league_id: 1,
            name: "Lions".to_string(),
        }];
        home.selected_team = Some(TeamMenuItem {
            id: 2,
            league_id: 1,
            name: "Tigers".to_string(),
        });

        home.reset_team_selection();

        assert_eq!(home.team_state.selected(), Some(0));
        assert_eq!(home.selected_team, None);
    }

    #[test]
    fn team_info_tab_selection_updates_selected_tab() {
        let mut home = Home::new();
        home.selected_item = Some(MenuOption::ViewTeamInfo);
        home.selected_team = Some(TeamMenuItem {
            id: 1,
            league_id: 1,
            name: "Lions".to_string(),
        });

        let action = home.update(Action::SelectGameDetailTab(2)).unwrap();

        assert_eq!(home.selected_team_info_tab, TeamInfoTab::PitcherStats);
        assert_eq!(action, Some(Action::Render));
    }

    #[test]
    fn team_schedule_month_helpers_cross_year_boundaries() {
        assert_eq!(
            Home::next_month_start(NaiveDate::from_ymd_opt(2026, 12, 1).unwrap()),
            NaiveDate::from_ymd_opt(2027, 1, 1).unwrap()
        );
        assert_eq!(
            Home::previous_month_start(NaiveDate::from_ymd_opt(2026, 1, 1).unwrap()),
            NaiveDate::from_ymd_opt(2025, 12, 1).unwrap()
        );
    }

    #[test]
    fn empty_schedule_month_does_not_change_selected_month() {
        let mut home = Home::new();
        home.team_schedule_month = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();

        let changed = home.commit_team_schedule_month_if_present(
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            Vec::new(),
        );

        assert!(!changed);
        assert_eq!(
            home.team_schedule_month,
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap()
        );
    }

    #[test]
    fn nonempty_schedule_month_changes_selected_month() {
        let mut home = Home::new();
        home.team_schedule_month = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();

        let changed = home.commit_team_schedule_month_if_present(
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap(),
            vec![team_schedule(
                1,
                NaiveDate::from_ymd_opt(2026, 10, 2).unwrap(),
                None,
                None,
                None,
            )],
        );

        assert!(changed);
        assert_eq!(
            home.team_schedule_month,
            NaiveDate::from_ymd_opt(2026, 10, 1).unwrap()
        );
        assert_eq!(home.team_schedule_state.selected(), Some(0));
    }

    #[test]
    fn schedule_title_includes_selected_month() {
        let mut home = Home::new();
        home.team_schedule_month = NaiveDate::from_ymd_opt(2026, 9, 1).unwrap();

        assert_eq!(home.team_schedule_title(), "Schedule:2026-09");
    }

    #[test]
    fn team_schedule_label_shows_result_for_completed_games() {
        let schedule = team_schedule(
            1,
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            Some(NaiveDate::from_ymd_opt(2026, 9, 2).unwrap()),
            Some(3),
            Some(2),
        );

        assert_eq!(
            Home::team_schedule_label(&schedule),
            "[2026-09-02] Away 3 - 2 Home (Regular)"
        );
    }

    #[test]
    fn team_schedule_label_shows_matchup_for_unplayed_games() {
        let schedule = team_schedule(
            1,
            NaiveDate::from_ymd_opt(2026, 9, 1).unwrap(),
            None,
            None,
            None,
        );

        assert_eq!(
            Home::team_schedule_label(&schedule),
            "[2026-09-01] Away - Home (Regular)"
        );
    }

    #[test]
    fn back_from_team_info_returns_to_team_selection() {
        let mut home = Home::new();
        home.selected_item = Some(MenuOption::ViewTeamInfo);
        home.selected_team = Some(TeamMenuItem {
            id: 1,
            league_id: 1,
            name: "Lions".to_string(),
        });

        let action = home.update(Action::Back).unwrap();

        assert_eq!(home.selected_item, Some(MenuOption::ViewTeamInfo));
        assert_eq!(home.selected_team, None);
        assert_eq!(action, Some(Action::Render));
    }

    #[test]
    fn back_exits_team_selection() {
        let mut home = Home::new();
        home.selected_item = Some(MenuOption::ViewTeamInfo);

        let action = home.update(Action::Back).unwrap();

        assert_eq!(home.selected_item, None);
        assert_eq!(action, Some(Action::Render));
    }
}
