use super::Component;
use crate::adapters::tui::action::Action;
use crate::adapters::tui::config::Config;
use crate::domain::shared::game::GameHeader;
use crate::domain::shared::game_cursor::GameCursor;
use crate::repositories::game_repository::{GameDetailReader, ProcessedGameReader};
use crate::{APP_CONTEXT, I18nManager, t};
use anyhow::Context;
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::layout::{Constraint, Layout};
use ratatui::prelude::*;
use ratatui::style::Color;
use ratatui::widgets::{Block, Borders, ListItem, ListState, Padding, Paragraph, Tabs, Wrap};
use tokio::sync::mpsc::UnboundedSender;
use tracing::{error, info};

mod formatter;
mod list;
mod pitch_zone;
mod scoreboard;
mod stats_tables;

#[derive(Default, Debug, Clone, Copy)]
enum GameDetailTab {
    #[default]
    GameSummary,
    GameProgress,
    BattingStats,
    PitchingStats,
}

impl GameDetailTab {
    fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(Self::GameSummary),
            1 => Some(Self::GameProgress),
            2 => Some(Self::BattingStats),
            3 => Some(Self::PitchingStats),
            _ => None,
        }
    }

    fn selected_index(self) -> usize {
        match self {
            Self::GameSummary => 0,
            Self::GameProgress => 1,
            Self::BattingStats => 2,
            Self::PitchingStats => 3,
        }
    }
}

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
    title: String,
    seasons: Vec<u16>,
    season_state: ListState,
    selected_season: Option<u16>,
    games: Vec<GameHeader>,
    game_state: ListState,
    selected_game_id: Option<u32>,
    game_cursor: Option<GameCursor>,
    selected_tab: GameDetailTab,
    error: Option<String>,
}

impl GameResultsWidget {
    pub fn new() -> Self {
        info!("Game Result component started.");

        let mut season_state = ListState::default();
        season_state.select(Some(0));

        Self {
            title: t!("game_progress"),
            season_state,
            ..Default::default()
        }
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn is_at_root(&self) -> bool {
        matches!(self.view, GameResultsView::SelectSeason)
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

    #[tracing::instrument(skip(self), fields(game_id = %game_id))]
    fn load_game_detail(&mut self, game_id: u32) {
        info!("load_game_detail started.");
        let game_row_res = APP_CONTEXT
            .get()
            .context("App context is not initialized")
            .map(|app_context| app_context.game_repository.load_game_detail(game_id));

        match game_row_res {
            Ok(Ok(game_row)) => {
                self.selected_game_id = Some(game_id);
                self.game_cursor = Some(GameCursor::new(game_row));
                self.error = None;
                self.selected_tab = GameDetailTab::GameSummary;
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

    fn back(&mut self) {
        match self.view {
            GameResultsView::SelectSeason => {}
            GameResultsView::SelectGame => {
                self.games.clear();
                self.game_state.select(None);
                self.selected_game_id = None;
                self.game_cursor = None;
                self.view = GameResultsView::SelectSeason;
            }
            GameResultsView::GameDetail => {
                self.selected_game_id = None;
                self.game_cursor = None;
                self.selected_tab = GameDetailTab::GameSummary;
                self.view = GameResultsView::SelectGame;
            }
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
        if self.seasons.is_empty() {
            frame.render_widget(Paragraph::new(t!("no_game_progress")), area);
            return;
        }

        let seasons: Vec<ListItem> = self
            .seasons
            .iter()
            .map(|season| ListItem::new(season.to_string()))
            .collect();

        let list = list::selectable_list(seasons, t!("select_season"));
        frame.render_stateful_widget(list, area, &mut self.season_state);
    }

    fn draw_game_list(&mut self, frame: &mut Frame, area: Rect) {
        if self.games.is_empty() {
            frame.render_widget(Paragraph::new(t!("no_game_result")), area);
            return;
        }

        let games: Vec<ListItem> = self
            .games
            .iter()
            .map(|game| ListItem::new(list::game_label(game)))
            .collect();

        let list = list::selectable_list(games, t!("select_game"));
        frame.render_stateful_widget(list, area, &mut self.game_state);
    }

    fn draw_game_detail(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let layout = Layout::vertical([Constraint::Length(3), Constraint::Min(0)]).split(area);

        let game_summary_tab_name = format!("{}(1)", t!("game_summary"));
        let game_progress_tab_name = format!("{}(2)", t!("game_progress"));
        let batting_stats_tab_name = format!("{}(3)", t!("batting_stats"));
        let pitching_stats_tab_name = format!("{}(4)", t!("pitching_stats"));
        let tabs = Tabs::new(vec![
            Line::from(game_summary_tab_name),
            Line::from(game_progress_tab_name),
            Line::from(batting_stats_tab_name),
            Line::from(pitching_stats_tab_name),
        ])
        .select(self.selected_tab.selected_index())
        .highlight_style(
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )
        .block(Block::new().borders(Borders::BOTTOM));

        frame.render_widget(tabs, layout[0]);

        match self.selected_tab {
            GameDetailTab::GameSummary => self.draw_game_summary_tab(frame, layout[1])?,
            GameDetailTab::GameProgress => self.draw_game_progress_tab(frame, layout[1])?,
            GameDetailTab::BattingStats => self.draw_batting_stats_tab(frame, layout[1])?,
            GameDetailTab::PitchingStats => self.draw_pitching_stats_tab(frame, layout[1])?,
        }

        Ok(())
    }

    fn draw_game_summary_tab(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let Some(cursor) = &self.game_cursor else {
            frame.render_widget(Paragraph::new(t!("select_game")), area);
            return Ok(());
        };

        let layout = Layout::vertical([
            Constraint::Length(5),
            Constraint::Length(5),
            Constraint::Length(7),
            Constraint::Min(6),
        ])
        .split(area);

        let scoreboard = cursor.final_scoreboard();
        scoreboard::draw_scoreboard(frame, layout[0], &scoreboard);

        frame.render_widget(
            Paragraph::new(self.format_pitching_decisions())
                .block(Block::bordered().title(t!("pitching_decisions")))
                .wrap(Wrap { trim: true }),
            layout[1],
        );

        frame.render_widget(
            Paragraph::new(self.format_home_runs())
                .block(Block::bordered().title(t!("home_runs")))
                .wrap(Wrap { trim: true }),
            layout[2],
        );

        frame.render_widget(
            Paragraph::new(self.format_batteries())
                .block(Block::bordered().title(t!("batteries")))
                .wrap(Wrap { trim: true }),
            layout[3],
        );

        Ok(())
    }

    fn format_pitching_decisions(&self) -> String {
        let Some(cursor) = &self.game_cursor else {
            return String::new();
        };

        let decisions = cursor.pitching_decisions();
        if decisions.is_empty() {
            return t!("na").to_string();
        }

        decisions
            .into_iter()
            .map(|decision| {
                format!(
                    "{}: {}\n",
                    Self::pitching_decision_label(&decision.decision),
                    decision.pitcher.full_name()
                )
            })
            .collect::<Vec<_>>()
            .join("  ")
    }

    fn pitching_decision_label(decision: &str) -> String {
        match decision {
            "Win" => t!("win").to_string(),
            "Loss" => t!("loss").to_string(),
            "Hold" => t!("hold").to_string(),
            "Save" => t!("save").to_string(),
            _ => decision.to_string(),
        }
    }

    fn format_home_runs(&self) -> String {
        let Some(cursor) = &self.game_cursor else {
            return String::new();
        };

        let home_runs = cursor.home_run_summaries();
        if home_runs.is_empty() {
            return t!("na").to_string();
        }

        home_runs
            .into_iter()
            .map(|home_run| {
                format!(
                    "{}({}) - {}",
                    home_run.batter.full_name(),
                    I18nManager::global().homerun(home_run.season_home_runs),
                    I18nManager::global().inning(home_run.inning_seq, home_run.inning_tb)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_batteries(&self) -> String {
        let Some(cursor) = &self.game_cursor else {
            return String::new();
        };

        cursor
            .battery_summaries()
            .into_iter()
            .map(|battery| {
                format!(
                    "{}:\n {}: {}\n{}: {}\n",
                    battery.team_name,
                    t!("pitcher"),
                    Self::format_player_list(&battery.pitchers),
                    t!("catcher"),
                    Self::format_player_list(&battery.catchers)
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn format_player_list(players: &[crate::domain::shared::player::PlayerInfo]) -> String {
        if players.is_empty() {
            return t!("na").to_string();
        }

        players
            .iter()
            .map(|player| player.full_name())
            .collect::<Vec<_>>()
            .join(", ")
    }

    fn draw_game_progress_tab(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let Some(cursor) = &mut self.game_cursor else {
            frame.render_widget(Paragraph::new(t!("select_game")), area);
            return Ok(());
        };

        if !cursor.has_counts() {
            frame.render_widget(Paragraph::new(t!("no_game_progress")), area);
            return Ok(());
        }

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
        scoreboard::draw_scoreboard(frame, layout[1], &scoreboard);

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
            Paragraph::new(formatter::format_count(&count)).block(Block::new().padding(Padding {
                left: 1,
                right: 0,
                top: 0,
                bottom: 0,
            })),
            count_area,
        );
        frame.render_widget(
            Paragraph::new(formatter::format_runner(cursor)).block(Block::new().padding(Padding {
                left: 1,
                right: 0,
                top: 0,
                bottom: 0,
            })),
            runner_area,
        );

        let strike_zone_and_batter_areas =
            Layout::vertical([Constraint::Percentage(60), Constraint::Percentage(40)])
                .split(strike_zone_and_batter_area);

        let strike_zone_area = strike_zone_and_batter_areas[0];
        let batter_area = strike_zone_and_batter_areas[1];

        let actual_location = cursor.current_pitching_view()?.ball.actual_location;
        pitch_zone::draw_strike_zone(frame, strike_zone_area, actual_location);
        frame.render_widget(
            Paragraph::new(formatter::format_batter_and_pitcher(cursor)?).block(
                Block::new().padding(Padding {
                    left: 2,
                    right: 0,
                    top: 0,
                    bottom: 0,
                }),
            ),
            batter_area,
        );
        frame.render_widget(
            Paragraph::new(formatter::format_lineup(cursor)?),
            lineup_area,
        );

        Ok(())
    }

    fn draw_batting_stats_tab(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let Some(cursor) = &mut self.game_cursor else {
            frame.render_widget(Paragraph::new(t!("select_game")), area);
            return Ok(());
        };

        let table_areas =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

        stats_tables::draw_batting_stats_table(
            frame,
            table_areas[0],
            cursor.away_team_name(),
            cursor.current_batting_stats_for_team(cursor.away_team_id()),
        );
        stats_tables::draw_batting_stats_table(
            frame,
            table_areas[1],
            cursor.home_team_name(),
            cursor.current_batting_stats_for_team(cursor.home_team_id()),
        );

        Ok(())
    }

    fn draw_pitching_stats_tab(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        let Some(cursor) = &mut self.game_cursor else {
            frame.render_widget(Paragraph::new(t!("select_game")), area);
            return Ok(());
        };

        let table_areas =
            Layout::vertical([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);

        stats_tables::draw_pitching_stats_table(
            frame,
            table_areas[0],
            cursor.away_team_name(),
            cursor.current_pitching_stats_for_team(cursor.away_team_id()),
        );
        stats_tables::draw_pitching_stats_table(
            frame,
            table_areas[1],
            cursor.home_team_name(),
            cursor.current_pitching_stats_for_team(cursor.home_team_id()),
        );

        Ok(())
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
            KeyCode::Esc | KeyCode::Backspace => Ok(Some(Action::Back)),
            KeyCode::Char('1') if matches!(self.view, GameResultsView::GameDetail) => {
                Ok(Some(Action::SelectGameDetailTab(0)))
            }
            KeyCode::Char('2') if matches!(self.view, GameResultsView::GameDetail) => {
                Ok(Some(Action::SelectGameDetailTab(1)))
            }
            KeyCode::Char('3') if matches!(self.view, GameResultsView::GameDetail) => {
                Ok(Some(Action::SelectGameDetailTab(2)))
            }
            KeyCode::Char('4') if matches!(self.view, GameResultsView::GameDetail) => {
                Ok(Some(Action::SelectGameDetailTab(3)))
            }
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
            Action::SelectGameDetailTab(index) => {
                if let Some(tab) = GameDetailTab::from_index(index) {
                    self.selected_tab = tab;
                }
                Ok(Some(Action::Render))
            }
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
            Action::Back => {
                self.back();
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
        let block = Block::new().title(self.title.clone()).borders(Borders::ALL);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::ball::BallLocation;
    use crate::domain::shared::game_cursor::BatterGameStatView;
    use crate::domain::shared::player::{PlayerInfo, Position};

    #[test]
    fn ball_location_section_maps_strike_zone_to_nine_sections() {
        assert_eq!(
            pitch_zone::ball_location_section(BallLocation { x: -0.8, y: 0.8 }),
            pitch_zone::PitchZoneSection::Strike(1)
        );
        assert_eq!(
            pitch_zone::ball_location_section(BallLocation { x: 0.0, y: 0.0 }),
            pitch_zone::PitchZoneSection::Strike(5)
        );
        assert_eq!(
            pitch_zone::ball_location_section(BallLocation { x: 0.8, y: -0.8 }),
            pitch_zone::PitchZoneSection::Strike(9)
        );
    }

    #[test]
    fn ball_location_section_maps_ball_zone_to_eight_sections() {
        assert_eq!(
            pitch_zone::ball_location_section(BallLocation { x: -1.2, y: 1.2 }),
            pitch_zone::PitchZoneSection::Ball(1)
        );
        assert_eq!(
            pitch_zone::ball_location_section(BallLocation { x: 0.0, y: 1.2 }),
            pitch_zone::PitchZoneSection::Ball(2)
        );
        assert_eq!(
            pitch_zone::ball_location_section(BallLocation { x: 1.2, y: 0.0 }),
            pitch_zone::PitchZoneSection::Ball(5)
        );
        assert_eq!(
            pitch_zone::ball_location_section(BallLocation { x: 0.0, y: -1.2 }),
            pitch_zone::PitchZoneSection::Ball(7)
        );
    }

    #[test]
    fn format_batting_order_lists_order_position_and_player() {
        let formatted_batting_order = formatter::format_batting_order(
            vec![
                BatterGameStatView {
                    team_id: 1,
                    batting_order: 1,
                    position: Position::CF,
                    player: PlayerInfo::new_min(10, "First10".to_string(), "Last10".to_string()),
                    plate_appearances: 0,
                    at_bats: 0,
                    hits: 0,
                    doubles: 0,
                    triples: 0,
                    home_runs: 0,
                },
                BatterGameStatView {
                    team_id: 1,
                    batting_order: 2,
                    position: Position::DH,
                    player: PlayerInfo::new_min(11, "First11".to_string(), "Last11".to_string()),
                    plate_appearances: 0,
                    at_bats: 0,
                    hits: 0,
                    doubles: 0,
                    triples: 0,
                    home_runs: 0,
                },
            ],
            Some(11),
        );

        assert_eq!(
            formatted_batting_order[0].to_string(),
            format!("{}: away", t!("batting_order"))
        );
        assert_eq!(
            formatted_batting_order[1].to_string(),
            "1. (CF) First10 Last10"
        );
        assert_eq!(formatted_batting_order[1].style.fg, None);
        assert_eq!(
            formatted_batting_order[2].to_string(),
            "2. (DH) First11 Last11"
        );
        assert_eq!(formatted_batting_order[2].style.fg, Some(Color::Yellow));
    }
}
