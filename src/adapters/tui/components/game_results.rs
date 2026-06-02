use super::Component;
use crate::adapters::tui::action::Action;
use crate::adapters::tui::config::Config;
use crate::domain::shared::game::{Base, Count, GameHeader};
use crate::domain::shared::game_cursor::{GameCursor, ScoreBoard};
use crate::domain::utils::is_base_occupied;
use crate::i18n::I18nManager;
use crate::repositories::game_repository::GameRepository;
use crate::{APP_CONTEXT, t};
use anyhow::Context;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

const RUNNER: &str = "R";
const NO_RUNNER: &str = "-";
const WALK_OFF: &str = "x";

#[derive(Default)]
enum GameResultsView {
    #[default]
    SelectSeason,
    SelectGame,
    GameDetail,
}

#[derive(Default)]
pub struct GameResultsWidget {
    view: GameResultsView,
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    seasons: Vec<u16>,
    season_state: ListState,
    selected_season: Option<u16>,
    games: Vec<GameHeader>,
    game_state: ListState,
    selected_game: Option<GameHeader>,
    game_cursor: Option<GameCursor>,
    error: Option<String>,
}

impl GameResultsWidget {
    pub fn new() -> Self {
        let mut season_state = ListState::default();
        season_state.select(Some(0));

        Self {
            season_state,
            ..Default::default()
        }
    }

    fn load_processed_seasons(&mut self) {
        let load_processed_seasons_res = APP_CONTEXT
            .get()
            .context("App context is not initialized")
            .map(|app_context| app_context.game_repository.load_processed_seasons());

        match load_processed_seasons_res {
            Ok(Ok(seasons)) => {
                self.seasons = seasons;
                self.error = None;

                if self.seasons.is_empty() {
                    self.season_state.select(None);
                } else if self.season_state.selected().is_none() {
                    self.season_state.select(Some(0));
                }
            }
            Ok(Err(err)) => {
                self.seasons.clear();
                self.season_state.select(None);
                self.error = Some(format!(
                    "{}: {}",
                    t!("error", "function" => "load_processed_seasons"),
                    err
                ));
            }
            Err(err) => {
                self.seasons.clear();
                self.season_state.select(None);
                self.error = Some(err.to_string());
            }
        }
    }

    fn load_games(&mut self, season: u16) {
        let game_headers_res = APP_CONTEXT
            .get()
            .context("App context is not initialized")
            .map(|app_context| {
                app_context
                    .game_repository
                    .load_processed_game_headers(season)
            });

        match game_headers_res {
            Ok(Ok(games)) => {
                self.games = games;
                self.game_state
                    .select(if self.games.is_empty() { None } else { Some(0) });
                self.selected_game = None;
                self.game_cursor = None;
                self.error = None;
                self.view = GameResultsView::SelectGame;
            }
            Ok(Err(err)) => {
                self.games.clear();
                self.game_state.select(None);
                self.error = Some(format!(
                    "{}: {}",
                    t!("error", "function" => "load_processed_game_headers"),
                    err
                ));
            }
            Err(err) => {
                self.games.clear();
                self.game_state.select(None);
                self.error = Some(err.to_string());
            }
        }
    }

    fn load_game_detail(&mut self, game: &GameHeader) {
        let game_row_res = APP_CONTEXT
            .get()
            .context("App context is not initialized")
            .map(|app_context| app_context.game_repository.load_game_row(game));

        match game_row_res {
            Ok(Ok(game_row)) => {
                self.selected_game = Some(game.clone());
                self.game_cursor = Some(GameCursor::new(game_row));
                self.error = None;
                self.view = GameResultsView::GameDetail;
            }
            Ok(Err(err)) => {
                self.game_cursor = None;
                self.error = Some(format!(
                    "{}: {}",
                    t!("error", "function" => "load_game_row"),
                    err
                ));
            }
            Err(err) => {
                self.game_cursor = None;
                self.error = Some(err.to_string());
            }
        }
    }

    fn select_next_season(&mut self) {
        let len = self.seasons.len();
        if len == 0 {
            return;
        }

        let selected = self.season_state.selected().unwrap_or(0);
        self.season_state.select(Some((selected + 1) % len));
    }

    fn select_previous_season(&mut self) {
        let len = self.seasons.len();
        if len == 0 {
            return;
        }

        let selected = self.season_state.selected().unwrap_or(0);
        self.season_state
            .select(Some(selected.checked_sub(1).unwrap_or(len - 1)));
    }

    fn selected_season(&self) -> Option<u16> {
        self.season_state
            .selected()
            .and_then(|selected| self.seasons.get(selected))
            .copied()
    }

    fn select_next_game(&mut self) {
        let len = self.games.len();
        if len == 0 {
            return;
        }

        let selected = self.game_state.selected().unwrap_or(0);
        self.game_state.select(Some((selected + 1) % len));
    }

    fn select_previous_game(&mut self) {
        let len = self.games.len();
        if len == 0 {
            return;
        }

        let selected = self.game_state.selected().unwrap_or(0);
        self.game_state
            .select(Some(selected.checked_sub(1).unwrap_or(len - 1)));
    }

    fn selected_game(&self) -> Option<GameHeader> {
        self.game_state
            .selected()
            .and_then(|selected| self.games.get(selected))
            .cloned()
    }

    fn confirm_selection(&mut self) {
        match self.view {
            GameResultsView::SelectSeason => {
                if let Some(season) = self.selected_season() {
                    self.selected_season = Some(season);
                    self.load_games(season);
                }
            }
            GameResultsView::SelectGame => {
                if let Some(game) = self.selected_game() {
                    self.load_game_detail(&game);
                }
            }
            GameResultsView::GameDetail => {}
        }
    }

    fn select_next(&mut self) {
        match self.view {
            GameResultsView::SelectSeason => self.select_next_season(),
            GameResultsView::SelectGame => self.select_next_game(),
            GameResultsView::GameDetail => {}
        }
    }

    fn select_previous(&mut self) {
        match self.view {
            GameResultsView::SelectSeason => self.select_previous_season(),
            GameResultsView::SelectGame => self.select_previous_game(),
            GameResultsView::GameDetail => {}
        }
    }

    fn next_count(&mut self) {
        if let Some(cursor) = &mut self.game_cursor {
            cursor.next();
        }
    }

    fn previous_count(&mut self) {
        if let Some(cursor) = &mut self.game_cursor {
            cursor.prev();
        }
    }

    fn draw_season_list(&mut self, frame: &mut Frame, area: Rect) {
        let seasons: Vec<ListItem> = self
            .seasons
            .iter()
            .map(|season| ListItem::new(season.to_string()))
            .collect();

        let list = Self::selectable_list(seasons, t!("select_season"));
        frame.render_stateful_widget(list, area, &mut self.season_state);
    }

    fn draw_game_list(&mut self, frame: &mut Frame, area: Rect) {
        let games: Vec<ListItem> = self
            .games
            .iter()
            .map(|game| ListItem::new(Self::game_label(game)))
            .collect();

        let list = Self::selectable_list(games, t!("select_game"));
        frame.render_stateful_widget(list, area, &mut self.game_state);
    }

    fn draw_game_detail(&mut self, frame: &mut Frame, area: Rect) {
        let Some(cursor) = &mut self.game_cursor else {
            frame.render_widget(Paragraph::new(t!("select_game")), area);
            return;
        };

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(5),
                Constraint::Min(8),
            ])
            .split(area);

        let header = format!(
            "<- {} {} ->\n<{}> {}:{}({}) {}:{}",
            t!("prev_count"),
            t!("next_count"),
            cursor.game_type(),
            t!("current_inning"),
            cursor.inning_seq,
            cursor.inning_tb,
            t!("current_count"),
            cursor.count_seq
        );
        frame.render_widget(Paragraph::new(header), layout[0]);

        let scoreboard = cursor.current_scoreboard();
        Self::draw_scoreboard(frame, layout[1], &scoreboard);

        let count = cursor.current_count();
        frame.render_widget(Paragraph::new(Self::format_count(&count)), layout[2]);
    }

    fn draw_scoreboard(frame: &mut Frame, area: Rect, scoreboard: &ScoreBoard) {
        let mut header_cells = vec![Cell::from(t!("team"))];
        for inning_seq in 1..=scoreboard.max_inning_num {
            header_cells.push(Cell::from(inning_seq.to_string()));
        }
        header_cells.push(Cell::from(t!("total_score")));

        let mut away_cells = vec![Cell::from(scoreboard.away_team_name.clone())];
        let mut home_cells = vec![Cell::from(scoreboard.home_team_name.clone())];

        for inning_index in 0..scoreboard.max_inning_num as usize {
            away_cells.push(Cell::from(
                scoreboard
                    .away_innning_points
                    .get(inning_index)
                    .map(u8::to_string)
                    .unwrap_or_default(),
            ));

            if scoreboard.is_last_bottom_inning_skiped
                && inning_index + 1 == scoreboard.max_inning_num as usize
            {
                home_cells.push(Cell::from(WALK_OFF));
            } else {
                home_cells.push(Cell::from(
                    scoreboard
                        .home_innning_points
                        .get(inning_index)
                        .map(u8::to_string)
                        .unwrap_or_default(),
                ));
            }
        }

        away_cells.push(Cell::from(scoreboard.away_total_point.to_string()));
        home_cells.push(Cell::from(scoreboard.home_total_point.to_string()));

        let mut widths = vec![Constraint::Min(8)];
        widths.extend(std::iter::repeat_n(
            Constraint::Length(3),
            scoreboard.max_inning_num as usize + 1,
        ));

        let table = Table::new([Row::new(away_cells), Row::new(home_cells)], widths)
            .header(Row::new(header_cells).style(Style::default().add_modifier(Modifier::BOLD)))
            .column_spacing(1);

        frame.render_widget(table, area);
    }

    fn format_count(count: &Count) -> String {
        let mut formatted_count = format!(
            "  <{}>\n<{}> <{}>\n  <H>\n",
            Self::display_runner(count.bases_occupied, Base::Second),
            Self::display_runner(count.bases_occupied, Base::Third),
            Self::display_runner(count.bases_occupied, Base::First)
        );
        formatted_count.push_str(&format!("{}: {}\n", t!("out_count"), count.out));
        formatted_count.push_str(&format!(
            "{}: {}\n",
            t!("batter"),
            I18nManager::global().full_name(&count.batter.first_name, &count.batter.last_name)
        ));
        formatted_count.push_str(&format!(
            "{}: .{}\n",
            t!("ba"),
            (count.batter.hit_average() * 1000.0).round()
        ));
        formatted_count.push_str(&format!(
            "{}: .{}\n",
            t!("slg"),
            (count.batter.slg() * 1000.0).round()
        ));
        formatted_count.push_str(&format!("{}: {}\n", t!("batting_result"), count.result));
        if count.point > 0 {
            formatted_count.push_str(&format!("{}: +{}\n", t!("score"), count.point));
        }

        formatted_count
    }

    fn display_runner(bases_occupied: u8, base: Base) -> &'static str {
        if is_base_occupied(bases_occupied, base) {
            RUNNER
        } else {
            NO_RUNNER
        }
    }

    fn game_label(game: &GameHeader) -> String {
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

    fn selectable_list(items: Vec<ListItem>, title: String) -> List {
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
}

impl Component for GameResultsWidget {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> color_eyre::Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> color_eyre::Result<()> {
        self.config = config;
        Ok(())
    }

    fn init(&mut self, _area: Size) -> color_eyre::Result<()> {
        self.load_processed_seasons();
        Ok(())
    }

    fn handle_key_event(&mut self, key: KeyEvent) -> color_eyre::Result<Option<Action>> {
        match key.code {
            KeyCode::Left if matches!(self.view, GameResultsView::GameDetail) => {
                Ok(Some(Action::PreviousCount))
            }
            KeyCode::Right if matches!(self.view, GameResultsView::GameDetail) => {
                Ok(Some(Action::NextCount))
            }
            _ => Ok(None),
        }
    }

    fn update(&mut self, action: Action) -> color_eyre::Result<Option<Action>> {
        match action {
            Action::SelectNext => {
                self.select_next();
                Ok(Some(Action::Render))
            }
            Action::SelectPrevious => {
                self.select_previous();
                Ok(Some(Action::Render))
            }
            Action::ConfirmSelection => {
                self.confirm_selection();
                Ok(Some(Action::Render))
            }
            Action::NextCount => {
                self.next_count();
                Ok(Some(Action::Render))
            }
            Action::PreviousCount => {
                self.previous_count();
                Ok(Some(Action::Render))
            }
            _ => Ok(None),
        }
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let block = Block::new().title(t!("game_results")).borders(Borders::ALL);
        let inner = block.inner(area);
        frame.render_widget(block, area);

        if let Some(error) = &self.error {
            frame.render_widget(Paragraph::new(error.as_str()), inner);
            return Ok(());
        }

        match self.view {
            GameResultsView::SelectSeason => self.draw_season_list(frame, inner),
            GameResultsView::SelectGame => self.draw_game_list(frame, inner),
            GameResultsView::GameDetail => self.draw_game_detail(frame, inner),
        }

        Ok(())
    }
}
