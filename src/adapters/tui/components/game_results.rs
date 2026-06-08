use super::Component;
use crate::adapters::tui::action::Action;
use crate::adapters::tui::config::Config;
use crate::domain::shared::game::{Base, Count, GameHeader};
use crate::domain::shared::game_cursor::{GameCursor, ScoreBoard};
use crate::domain::utils::is_base_occupied;
use crate::repositories::game_repository::GameRepository;
use crate::{APP_CONTEXT, t};
use anyhow::Context;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Flex, Layout};
use ratatui::prelude::*;
use ratatui::style::Color;
use ratatui::symbols::Marker;
use ratatui::widgets::canvas::{Canvas, Rectangle};
use ratatui::widgets::{
    Block, Borders, Cell, List, ListItem, ListState, Padding, Paragraph, Row, Table,
};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info};

const RUNNER: &str = "R";
const NO_RUNNER: &str = "-";
const WALK_OFF: &str = "x";

#[derive(Default, Debug)]
enum GameResultsView {
    #[default]
    SelectSeason,
    SelectGame,
    GameDetail,
}

#[derive(Default, Debug)]
pub struct GameResultsWidget {
    view: GameResultsView,
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    seasons: Vec<u16>,
    season_state: ListState,
    selected_season: Option<u16>,
    games: Vec<GameHeader>,
    game_state: ListState,
    selected_game_id: Option<u32>,
    game_cursor: Option<GameCursor>,
    error: Option<String>,
}

impl GameResultsWidget {
    pub fn new() -> Self {
        info!("Game Result component started.");

        let mut season_state = ListState::default();
        season_state.select(Some(0));

        Self {
            season_state,
            ..Default::default()
        }
    }

    fn load_processed_seasons(&mut self) {
        info!("load_processed_seasons started.");
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
        info!("load_games started.");
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
                self.selected_game_id = None;
                self.game_cursor = None;
                self.error = None;
                self.view = GameResultsView::SelectGame;
            }
            Ok(Err(err)) => {
                self.games.clear();
                self.game_state.select(None);
                self.error = Some(format!(
                    "{}: \n{}",
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

    #[tracing::instrument(fields(game_id = %game_id))]
    fn load_game_detail(&mut self, game_id: u32) {
        info!("load_games started.");
        let game_row_res = APP_CONTEXT
            .get()
            .context("App context is not initialized")
            .map(|app_context| app_context.game_repository.load_game_detail(game_id));

        match game_row_res {
            Ok(Ok(game_row)) => {
                self.selected_game_id = Some(game_id);
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
                error!(self.error);
            }
            Err(err) => {
                self.game_cursor = None;
                self.error = Some(err.to_string());
                error!(self.error);
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
                    self.load_game_detail(game.id);
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

    fn draw_game_detail(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let Some(cursor) = &mut self.game_cursor else {
            frame.render_widget(Paragraph::new(t!("select_game")), area);
            return Ok(());
        };

        let layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(5),
                Constraint::Length(1),
                Constraint::Min(8),
            ])
            .split(area);

        let navigator = format!("<- {} {} ->", t!("prev_count"), t!("next_count"),);

        let header = format!(
            "\n\n<{}> {}:{}({}) {}:{}",
            cursor.game_type(),
            t!("current_inning"),
            cursor.inning_seq,
            cursor.inning_tb,
            t!("current_count"),
            cursor.count_seq
        );
        frame.render_widget(Paragraph::new(navigator), layout[0]);
        frame.render_widget(Paragraph::new(header), layout[0]);

        let scoreboard = cursor.current_scoreboard();
        Self::draw_scoreboard(frame, layout[1], &scoreboard);

        let count = cursor.current_count();
        let game_status_areas = Layout::horizontal([
            Constraint::Percentage(13),
            Constraint::Percentage(15),
            Constraint::Percentage(37),
            Constraint::Percentage(35),
        ])
        .split(layout[3]);

        let count_area = game_status_areas[0];
        let runner_area = game_status_areas[1];
        let strike_zone_and_batter_area = game_status_areas[2];
        let lineup_area = game_status_areas[3];

        frame.render_widget(
            Paragraph::new(Self::format_count(&count)).block(Block::new().padding(Padding {
                left: 1,
                right: 0,
                top: 0,
                bottom: 0,
            })),
            count_area,
        );
        frame.render_widget(
            Paragraph::new(Self::format_runner(&count)).block(Block::new().padding(Padding {
                left: 1,
                right: 0,
                top: 0,
                bottom: 0,
            })),
            runner_area,
        );

        let strike_zone_and_batter_areas =
            Layout::vertical([Constraint::Percentage(48), Constraint::Percentage(52)])
                .split(strike_zone_and_batter_area);

        let strike_zone_area = strike_zone_and_batter_areas[0];
        let batter_area = strike_zone_and_batter_areas[1];

        Self::draw_strike_zone(frame, strike_zone_area, cursor);
        frame.render_widget(
            Paragraph::new(Self::format_batter_and_pitcher(cursor)?).block(Block::new().padding(
                Padding {
                    left: 2,
                    right: 0,
                    top: 0,
                    bottom: 0,
                },
            )),
            batter_area,
        );
        frame.render_widget(Paragraph::new(Self::format_lineup(cursor)?), lineup_area);

        Ok(())
    }

    fn draw_scoreboard(frame: &mut Frame, area: Rect, scoreboard: &ScoreBoard) {
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
                home_cells.push(Cell::from(WALK_OFF));
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
                ((team_name_length as usize + 5) + (scoreboard.max_inning_num as usize * 4) + 4)
                    as u16,
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

    fn format_count(count: &Count) -> String {
        let mut formatted_count = format!("{}: {}\n", "B", Self::display_count_number(0));
        formatted_count.push_str(&format!("{}: {}\n", "S", Self::display_count_number(0)));
        formatted_count.push_str(&format!(
            "{}: {}\n",
            "O",
            Self::display_count_number(count.out)
        ));
        formatted_count
    }

    fn format_runner(count: &Count) -> String {
        format!(
            "  <{}>\n<{}> <{}>\n  <H>\n",
            Self::display_runner(count.bases_occupied, Base::Second),
            Self::display_runner(count.bases_occupied, Base::Third),
            Self::display_runner(count.bases_occupied, Base::First)
        )
    }

    fn draw_strike_zone(frame: &mut Frame, area: Rect, game_cursor: &mut GameCursor) {
        // println!("width:{}, height:{}", area.width, area.height);
        let canvas = Canvas::default()
            .marker(Marker::Braille) // これ大事！ 細かい図形ならBraille推奨
            .x_bounds([0.0, area.width as f64])
            .y_bounds([0.0, area.height as f64])
            .paint(|ctx| {
                ctx.draw(&Rectangle {
                    x: 3.0,
                    y: 3.0,
                    width: 15.0,
                    height: 7.0,
                    color: Color::Gray,
                });

                ctx.print(
                    1.0,
                    10.0,
                    Span::styled("[1]", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    10.0,
                    10.0,
                    Span::styled("[2]", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    19.0,
                    10.0,
                    Span::styled("[3]", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    1.0,
                    6.0,
                    Span::styled("[4]", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    19.0,
                    6.0,
                    Span::styled("[5]", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    1.0,
                    2.0,
                    Span::styled("[6]", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    10.0,
                    2.0,
                    Span::styled("[7]", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    19.0,
                    2.0,
                    Span::styled("[8]", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    6.0,
                    8.0,
                    Span::styled("<1>", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    10.0,
                    8.0,
                    Span::styled("<2>", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    14.0,
                    8.0,
                    Span::styled("<3>", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    6.0,
                    6.0,
                    Span::styled("<4>", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    10.0,
                    6.0,
                    Span::styled("<5>", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    14.0,
                    6.0,
                    Span::styled("<6>", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    6.0,
                    4.0,
                    Span::styled("<7>", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    10.0,
                    4.0,
                    Span::styled("<8>", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );

                ctx.print(
                    14.0,
                    4.0,
                    Span::styled("<9>", Color::White).add_modifier(ratatui::style::Modifier::BOLD),
                );
            });

        frame.render_widget(canvas, area);
    }

    fn format_batter_and_pitcher(game_cursor: &mut GameCursor) -> color_eyre::Result<(String)> {
        let pitcher = game_cursor.current_pitcher()?;
        let batter = game_cursor.current_batter()?;
        let mut formatted_batter_and_pitcher =
            format!("{}: {}\n", t!("pitcher"), pitcher.full_name());
        formatted_batter_and_pitcher.push_str(&format!(
            "{}: {}\n",
            t!("batter"),
            batter.full_name()
        ));

        Ok(formatted_batter_and_pitcher)
    }

    fn format_lineup(game_cursor: &mut GameCursor) -> color_eyre::Result<(String)> {
        let pitcher = game_cursor.current_pitcher()?;
        let catcher = game_cursor.current_catcher()?;
        let fb = game_cursor.current_fb()?;
        let sb = game_cursor.current_sb()?;
        let tb = game_cursor.current_tb()?;
        let ss = game_cursor.current_ss()?;
        let rf = game_cursor.current_rf()?;
        let cf = game_cursor.current_cf()?;
        let lf = game_cursor.current_lf()?;

        let mut formatted_lineup = format!("({}) {}\n", t!("p"), pitcher.full_name());
        formatted_lineup.push_str(&format!("({}) {}\n", t!("c"), catcher.full_name()));
        formatted_lineup.push_str(&format!("({}) {}\n", t!("fb"), fb.full_name()));
        formatted_lineup.push_str(&format!("({}) {}\n", t!("sb"), sb.full_name()));
        formatted_lineup.push_str(&format!("({}) {}\n", t!("tb"), tb.full_name()));
        formatted_lineup.push_str(&format!("({}) {}\n", t!("ss"), ss.full_name()));
        formatted_lineup.push_str(&format!("({}) {}\n", t!("rf"), rf.full_name()));
        formatted_lineup.push_str(&format!("({}) {}\n", t!("cf"), cf.full_name()));
        formatted_lineup.push_str(&format!("({}) {}\n", t!("lf"), lf.full_name()));

        Ok(formatted_lineup)
    }

    fn display_runner(bases_occupied: u8, base: Base) -> &'static str {
        if is_base_occupied(bases_occupied, base) {
            RUNNER
        } else {
            NO_RUNNER
        }
    }

    fn display_count_number(number: u8) -> String {
        let mut count_number = "".to_string();
        for _ in 0..number {
            count_number.push_str("●");
        }
        count_number
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
            GameResultsView::GameDetail => self.draw_game_detail(frame, inner)?,
        }

        Ok(())
    }
}
