use super::Component;
use crate::adapters::tui::action::Action;
use crate::adapters::tui::config::Config;
use crate::domain::shared::player::{Player, Position};
use crate::repositories::game_repository::GamePlayerReader;
use crate::{APP_CONTEXT, t};
use color_eyre::eyre::eyre;
use ratatui::{prelude::*, widgets::*};
use tokio::sync::mpsc::UnboundedSender;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TeamPlayerInfoLane {
    Pitcher,
    Catcher,
    Infielder,
    Outfielder,
}

impl Default for TeamPlayerInfoLane {
    fn default() -> Self {
        Self::Pitcher
    }
}

impl TeamPlayerInfoLane {
    const ALL: [Self; 4] = [
        Self::Pitcher,
        Self::Catcher,
        Self::Infielder,
        Self::Outfielder,
    ];

    fn label(self) -> String {
        match self {
            Self::Pitcher => t!("pitcher").to_string(),
            Self::Catcher => t!("catcher").to_string(),
            Self::Infielder => t!("infielder").to_string(),
            Self::Outfielder => t!("outfielder").to_string(),
        }
    }

    fn contains(self, position: Position) -> bool {
        match self {
            Self::Pitcher => position == Position::P,
            Self::Catcher => position == Position::C,
            Self::Infielder => matches!(
                position,
                Position::FB | Position::SB | Position::TB | Position::SS | Position::DH
            ),
            Self::Outfielder => position.is_outfielder(),
        }
    }

    fn selected_index(self) -> usize {
        match self {
            Self::Pitcher => 0,
            Self::Catcher => 1,
            Self::Infielder => 2,
            Self::Outfielder => 3,
        }
    }

    fn from_index(index: usize) -> Self {
        Self::ALL[index % Self::ALL.len()]
    }
}

#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
enum PlayerDetailPane {
    #[default]
    Summary,
    PitchSkills,
}

#[derive(Default)]
pub struct PlayerInfoWidget {
    command_tx: Option<UnboundedSender<Action>>,
    config: Config,
    players: Vec<Player>,
    players_error: Option<String>,
    player_info_lane: TeamPlayerInfoLane,
    selected_player_id: Option<i64>,
    selected_player_detail_pane: PlayerDetailPane,
    pitch_skills_scroll_offset: usize,
    pitcher_state: ListState,
    catcher_state: ListState,
    infielder_state: ListState,
    outfielder_state: ListState,
}

impl PlayerInfoWidget {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn clear(&mut self) {
        self.players.clear();
        self.players_error = None;
        self.reset_selection();
        self.close_detail();
    }

    pub fn reset_detail(&mut self) {
        self.close_detail();
    }

    pub fn load_players(&mut self, team_id: u16) {
        let players_res = APP_CONTEXT
            .get()
            .ok_or_else(|| eyre!("App context is not initialized"))
            .map(|app_context| app_context.game_repository.load_team_players(team_id));

        match players_res {
            Ok(Ok(players)) => {
                self.players = players;
                self.players_error = None;
                self.reset_selection();
                self.close_detail();
            }
            Ok(Err(err)) => {
                self.players.clear();
                self.reset_selection();
                self.close_detail();
                self.players_error = Some(format!(
                    "{}: {}",
                    t!("error", "function" => "load_team_players"),
                    err
                ));
            }
            Err(err) => {
                self.players.clear();
                self.reset_selection();
                self.close_detail();
                self.players_error = Some(format!(
                    "{}: {}",
                    t!("error", "function" => "load_team_players"),
                    err
                ));
            }
        }
    }

    pub fn update_player_info(&mut self, action: Action) -> bool {
        match action {
            Action::SelectNext
                if self.selected_player_detail_pane == PlayerDetailPane::PitchSkills =>
            {
                self.scroll_next_pitch_skills();
                true
            }
            Action::SelectPrevious
                if self.selected_player_detail_pane == PlayerDetailPane::PitchSkills =>
            {
                self.scroll_previous_pitch_skills();
                true
            }
            Action::SelectNext if self.selected_player_id.is_none() => {
                self.select_next_player();
                true
            }
            Action::SelectPrevious if self.selected_player_id.is_none() => {
                self.select_previous_player();
                true
            }
            Action::NextCount if self.selected_player_id.is_none() => {
                self.select_next_lane();
                true
            }
            Action::PreviousCount if self.selected_player_id.is_none() => {
                self.select_previous_lane();
                true
            }
            Action::ConfirmSelection
                if self.selected_player_id.is_some()
                    && self.selected_player_detail_pane == PlayerDetailPane::Summary
                    && self.selected_player_has_pitcher_info() =>
            {
                let _ = self.open_selected_pitch_skills();
                true
            }
            Action::ConfirmSelection
                if self.selected_player_id.is_none() && self.selected_player().is_some() =>
            {
                self.open_selected_player_detail();
                true
            }
            Action::Back if self.selected_player_detail_pane == PlayerDetailPane::PitchSkills => {
                self.selected_player_detail_pane = PlayerDetailPane::Summary;
                self.pitch_skills_scroll_offset = 0;
                true
            }
            Action::Back if self.selected_player_id.is_some() => {
                self.close_detail();
                true
            }
            _ => false,
        }
    }

    fn lane_state(&self, lane: TeamPlayerInfoLane) -> &ListState {
        match lane {
            TeamPlayerInfoLane::Pitcher => &self.pitcher_state,
            TeamPlayerInfoLane::Catcher => &self.catcher_state,
            TeamPlayerInfoLane::Infielder => &self.infielder_state,
            TeamPlayerInfoLane::Outfielder => &self.outfielder_state,
        }
    }

    fn lane_state_mut(&mut self, lane: TeamPlayerInfoLane) -> &mut ListState {
        match lane {
            TeamPlayerInfoLane::Pitcher => &mut self.pitcher_state,
            TeamPlayerInfoLane::Catcher => &mut self.catcher_state,
            TeamPlayerInfoLane::Infielder => &mut self.infielder_state,
            TeamPlayerInfoLane::Outfielder => &mut self.outfielder_state,
        }
    }

    fn reset_selection(&mut self) {
        for lane in TeamPlayerInfoLane::ALL {
            let len = self.players_for_lane(lane).len();
            self.lane_state_mut(lane)
                .select(if len == 0 { None } else { Some(0) });
        }

        self.player_info_lane = TeamPlayerInfoLane::ALL
            .into_iter()
            .find(|lane| !self.players_for_lane(*lane).is_empty())
            .unwrap_or_default();
    }

    fn close_detail(&mut self) {
        self.selected_player_id = None;
        self.selected_player_detail_pane = PlayerDetailPane::Summary;
        self.pitch_skills_scroll_offset = 0;
    }

    fn select_next_lane(&mut self) {
        let next_index =
            (self.player_info_lane.selected_index() + 1) % TeamPlayerInfoLane::ALL.len();
        self.player_info_lane = TeamPlayerInfoLane::from_index(next_index);
    }

    fn select_previous_lane(&mut self) {
        let len = TeamPlayerInfoLane::ALL.len();
        let selected = self.player_info_lane.selected_index();
        self.player_info_lane =
            TeamPlayerInfoLane::from_index(selected.checked_sub(1).unwrap_or(len - 1));
    }

    fn select_next_player(&mut self) {
        let lane = self.player_info_lane;
        let len = self.players_for_lane(lane).len();
        if len == 0 {
            return;
        }

        let selected = self.lane_state(lane).selected().unwrap_or(0);
        self.lane_state_mut(lane).select(Some((selected + 1) % len));
    }

    fn select_previous_player(&mut self) {
        let lane = self.player_info_lane;
        let len = self.players_for_lane(lane).len();
        if len == 0 {
            return;
        }

        let selected = self.lane_state(lane).selected().unwrap_or(0);
        self.lane_state_mut(lane)
            .select(Some(selected.checked_sub(1).unwrap_or(len - 1)));
    }

    fn selected_player(&self) -> Option<&Player> {
        let lane = self.player_info_lane;
        self.lane_state(lane)
            .selected()
            .and_then(|selected| self.players_for_lane(lane).get(selected).copied())
    }

    fn selected_player_detail(&self) -> Option<&Player> {
        self.selected_player_id.and_then(|player_id| {
            self.players
                .iter()
                .find(|player| player.info.id == player_id)
        })
    }

    fn open_selected_player_detail(&mut self) {
        self.selected_player_id = self.selected_player().map(|player| player.info.id);
        self.selected_player_detail_pane = PlayerDetailPane::Summary;
        self.pitch_skills_scroll_offset = 0;
    }

    fn open_selected_pitch_skills(&mut self) -> bool {
        if self.selected_player_has_pitcher_info() {
            self.selected_player_detail_pane = PlayerDetailPane::PitchSkills;
            self.pitch_skills_scroll_offset = 0;
            true
        } else {
            false
        }
    }

    fn selected_player_has_pitcher_info(&self) -> bool {
        self.selected_player_detail()
            .and_then(|player| player.defense_skills.pitcher.as_ref())
            .is_some()
    }

    fn max_pitch_skills_scroll_offset(line_count: usize, detail_area: Rect) -> usize {
        line_count.saturating_sub(usize::from(detail_area.height))
    }

    fn clamp_pitch_skills_scroll_offset(&mut self, line_count: usize, detail_area: Rect) {
        self.pitch_skills_scroll_offset =
            self.pitch_skills_scroll_offset
                .min(Self::max_pitch_skills_scroll_offset(
                    line_count,
                    detail_area,
                ));
    }

    fn scroll_next_pitch_skills(&mut self) {
        self.pitch_skills_scroll_offset = self.pitch_skills_scroll_offset.saturating_add(1);
    }

    fn scroll_previous_pitch_skills(&mut self) {
        self.pitch_skills_scroll_offset = self.pitch_skills_scroll_offset.saturating_sub(1);
    }

    fn position_sort_key(position: Position) -> u8 {
        match position {
            Position::P => 0,
            Position::C => 1,
            Position::FB => 2,
            Position::SB => 3,
            Position::TB => 4,
            Position::SS => 5,
            Position::DH => 6,
            Position::LF => 7,
            Position::CF => 8,
            Position::RF => 9,
        }
    }

    fn players_for_lane(&self, lane: TeamPlayerInfoLane) -> Vec<&Player> {
        let mut players = self
            .players
            .iter()
            .filter(|player| lane.contains(player.defense_skills.position))
            .collect::<Vec<_>>();

        players.sort_by_key(|player| {
            (
                Self::position_sort_key(player.defense_skills.position),
                player.info.uniform_number,
                player.full_name(),
            )
        });

        players
    }

    fn format_f64(label: &str, value: f64) -> String {
        format!("{label}: {value:.2}")
    }

    fn format_fielder_info(fielder: &crate::domain::shared::player::FielderInfo) -> Vec<String> {
        vec![
            format!("{}: {}", t!("fielder_position"), fielder.fielder_type),
            Self::format_f64("Throw Speed", fielder.throw_speed),
            Self::format_f64("Running Speed", fielder.running_speed),
            Self::format_f64("Reaction", fielder.reaction),
            Self::format_f64("Prep Time", fielder.prep_time),
            Self::format_f64("Catching", fielder.catching),
            Self::format_f64("Reach Height", fielder.reach_height),
            Self::format_f64("Reach Range", fielder.reach_range),
        ]
    }

    fn player_detail_primary_title(player: &Player) -> &'static str {
        if player.defense_skills.position == Position::P {
            "Player / Pitcher Info"
        } else {
            "Player / Batter Info"
        }
    }

    fn player_detail_primary_footer(player: &Player) -> Option<&'static str> {
        (player.defense_skills.position == Position::P).then_some("Hit return key for Pitch Skills")
    }

    fn format_player_detail_sections(player: &Player) -> (String, String) {
        let mut primary_lines = vec![
            format!("{}: {}", t!("player"), player.full_name()),
            format!("ID: {}", player.info.id),
            format!("{}: {}", t!("pos"), player.defense_skills.position.short()),
            format!("Uniform Number: {}", player.info.uniform_number),
            format!("Age: {}", player.info.age),
        ];
        let mut skill_lines = Vec::new();

        if let Some(pitcher) = player.defense_skills.pitcher.as_ref() {
            primary_lines.extend([
                String::new(),
                t!("pitcher").to_string(),
                Self::format_f64("Height", pitcher.height),
                Self::format_f64("Extension", pitcher.extension),
                format!("Throw Side: {}", pitcher.throw_side),
                format!("Arm Slot: {:?}", pitcher.arm_slot),
                format!("Style: {}", pitcher.pitcher_style),
                Self::format_f64("Velocity", pitcher.velocity),
                Self::format_f64("Spin Rate", pitcher.spin_rate),
                Self::format_f64("Control", pitcher.control),
                Self::format_f64("Stamina", pitcher.stamina),
                Self::format_f64("Injury Proneness", pitcher.injury_proneness),
                Self::format_f64("Clutch", pitcher.clutch),
                Self::format_f64("HPP", pitcher.hpp),
                Self::format_f64("Platoon Splitting", pitcher.platoon_splitting),
                Self::format_f64("Delivery Motion Time", pitcher.delivery_motion_time),
                Self::format_f64("Consistency", pitcher.consistency),
            ]);

            skill_lines.push(t!("fielders_choice").to_string());
            skill_lines.extend(Self::format_fielder_info(&pitcher.fielder_info));
        }

        if let Some(batter) = player.offense_skills.batter.as_ref() {
            primary_lines.extend([
                String::new(),
                t!("batter").to_string(),
                format!("Batting Side: {}", batter.batting_side),
                format!("Batter Type: {:?}", batter.batter_type),
                format!("Zone Aptitude: {:?}", batter.zone_aptitude),
                Self::format_f64("Hot Zone Scale", batter.hot_zone_scale),
                Self::format_f64("Batting Eye", batter.batting_eye),
                Self::format_f64("Swing Speed", batter.swing_speed),
                Self::format_f64("Swing Power", batter.swing_power),
                Self::format_f64("Attack Angle", batter.attack_angle),
                Self::format_f64("Bat Control", batter.bat_control),
                Self::format_f64("Consistency", batter.consistency),
            ]);

            if !skill_lines.is_empty() {
                skill_lines.push(String::new());
            }
            skill_lines.extend([
                t!("runner").to_string(),
                Self::format_f64("Speed", player.offense_skills.running.speed),
                Self::format_f64("Lead Distance", player.offense_skills.running.lead_distance),
                Self::format_f64(
                    "Start Reaction",
                    player.offense_skills.running.start_reaction,
                ),
            ]);
        }

        if let Some(catcher) = player.defense_skills.catcher.as_ref() {
            if !skill_lines.is_empty() {
                skill_lines.push(String::new());
            }
            skill_lines.push(t!("catcher").to_string());
            skill_lines.extend(Self::format_fielder_info(&catcher.fielder_info));
        }
        if let Some(fielder) = player.defense_skills.middle_infielder.as_ref() {
            if !skill_lines.is_empty() {
                skill_lines.push(String::new());
            }
            skill_lines.push(t!("middle_infielder").to_string());
            skill_lines.extend(Self::format_fielder_info(fielder));
        }
        if let Some(fielder) = player.defense_skills.corner_infielder.as_ref() {
            if !skill_lines.is_empty() {
                skill_lines.push(String::new());
            }
            skill_lines.push(t!("corner_infielder").to_string());
            skill_lines.extend(Self::format_fielder_info(fielder));
        }
        if let Some(fielder) = player.defense_skills.outfielder.as_ref() {
            if !skill_lines.is_empty() {
                skill_lines.push(String::new());
            }
            skill_lines.push(t!("outfielder").to_string());
            skill_lines.extend(Self::format_fielder_info(fielder));
        }

        (
            primary_lines.join("\n"),
            skill_lines
                .into_iter()
                .skip_while(String::is_empty)
                .collect::<Vec<_>>()
                .join("\n"),
        )
    }

    fn format_pitch_skills_detail(player: &Player) -> String {
        let Some(pitcher) = player.defense_skills.pitcher.as_ref() else {
            return String::new();
        };

        if pitcher.pitch_skills.is_empty() {
            return "No Pitch Skills".to_string();
        }

        pitcher
            .pitch_skills
            .iter()
            .map(|skill| {
                format!(
                    "{}\n  Velocity: {:.2}\n  Control: {:.2}\n  Stamina: {:.2}\n  Injury Proneness: {:.2}\n  Spin Rate: {:.2}\n  Spin Angle: {:.2}\n  Spin Efficiency: {:.2}\n  Usage: {:.2}",
                    skill.pitch_type,
                    skill.velocity,
                    skill.control,
                    skill.stamina,
                    skill.injury_proneness,
                    skill.spin_rate,
                    skill.spin_angle,
                    skill.spin_efficiency,
                    skill.usage
                )
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }
}

impl Component for PlayerInfoWidget {
    fn register_action_handler(&mut self, tx: UnboundedSender<Action>) -> color_eyre::Result<()> {
        self.command_tx = Some(tx);
        Ok(())
    }

    fn register_config_handler(&mut self, config: Config) -> color_eyre::Result<()> {
        self.config = config;
        Ok(())
    }

    fn draw(&mut self, frame: &mut Frame, area: Rect) -> color_eyre::Result<()> {
        if let Some(error) = &self.players_error {
            frame.render_widget(Paragraph::new(error.as_str()), area);
            return Ok(());
        }

        if let Some(player) = self.selected_player_detail() {
            if self.selected_player_detail_pane == PlayerDetailPane::PitchSkills {
                let detail = Self::format_pitch_skills_detail(player);
                let title = format!("Pitch Skills: {}", player.full_name());
                let line_count = detail.lines().count().max(1);
                let block = Block::new().title(title).borders(Borders::ALL);
                let inner = block.inner(area);
                let layout =
                    Layout::horizontal([Constraint::Min(0), Constraint::Length(1)]).split(inner);
                let detail_area = layout[0];
                let scrollbar_area = layout[1];

                self.clamp_pitch_skills_scroll_offset(line_count, detail_area);

                frame.render_widget(block, area);
                frame.render_widget(
                    Paragraph::new(detail)
                        .scroll((self.pitch_skills_scroll_offset as u16, 0))
                        .wrap(Wrap { trim: false }),
                    detail_area,
                );

                let mut scrollbar_state = ScrollbarState::new(line_count)
                    .viewport_content_length(usize::from(detail_area.height))
                    .position(self.pitch_skills_scroll_offset);
                frame.render_stateful_widget(
                    Scrollbar::new(ScrollbarOrientation::VerticalRight),
                    scrollbar_area,
                    &mut scrollbar_state,
                );
                return Ok(());
            }

            let detail_layout =
                Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)])
                    .split(area);
            let (primary_detail, skill_detail) = Self::format_player_detail_sections(player);
            let primary_block = Block::new()
                .title(Self::player_detail_primary_title(player))
                .borders(Borders::ALL);
            let primary_inner = primary_block.inner(detail_layout[0]);
            frame.render_widget(primary_block, detail_layout[0]);

            if let Some(footer) = Self::player_detail_primary_footer(player) {
                let primary_layout = Layout::vertical([Constraint::Min(0), Constraint::Length(1)])
                    .split(primary_inner);
                frame.render_widget(
                    Paragraph::new(primary_detail).wrap(Wrap { trim: false }),
                    primary_layout[0],
                );
                frame.render_widget(
                    Paragraph::new(footer)
                        .style(Style::default().fg(Color::Yellow))
                        .alignment(Alignment::Center),
                    primary_layout[1],
                );
            } else {
                frame.render_widget(
                    Paragraph::new(primary_detail).wrap(Wrap { trim: false }),
                    primary_inner,
                );
            }

            frame.render_widget(
                Paragraph::new(skill_detail)
                    .block(
                        Block::new()
                            .title("Running / Defense Info")
                            .borders(Borders::ALL),
                    )
                    .wrap(Wrap { trim: false }),
                detail_layout[1],
            );
            return Ok(());
        }

        let lanes = Layout::horizontal([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(area);

        for (area, lane) in lanes.iter().zip(TeamPlayerInfoLane::ALL) {
            let players = self.players_for_lane(lane);
            let items = if players.is_empty() {
                vec![ListItem::new(t!("na").to_string())]
            } else {
                players
                    .into_iter()
                    .map(|player| ListItem::new(player.full_name()))
                    .collect()
            };
            let border_style = if lane == self.player_info_lane {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };

            let list = List::new(items)
                .block(
                    Block::new()
                        .title(lane.label())
                        .borders(Borders::ALL)
                        .border_style(border_style),
                )
                .highlight_style(
                    Style::default()
                        .bg(Color::Blue)
                        .fg(Color::White)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("> ");

            frame.render_stateful_widget(list, *area, self.lane_state_mut(lane));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::{
        ArmSlot, BatterInfo, BatterType, DefenseSkills, FielderInfo, PitchSkill, PitchType,
        PitcherInfo, PitcherStyle, PlayerInfo, RL, RunningSkills, ZoneAptitude,
    };

    fn player(id: i64, first_name: &str, position: Position) -> Player {
        let mut player = Player::from_player_info(PlayerInfo::new(
            id,
            first_name.to_string(),
            "Player".to_string(),
            20,
            id as u8,
        ));
        player.defense_skills = DefenseSkills::new(position);
        player
    }

    fn pitcher_player() -> Player {
        let mut player = player(1, "Pitcher", Position::P);
        player.defense_skills.pitcher = Some(PitcherInfo::from_prob(
            1.85,
            1.8,
            RL::Right,
            ArmSlot::ThreeQuarter,
            PitcherStyle::BalancedPitcher,
            145.0,
            2200.0,
            0.7,
            90.0,
            0.1,
            0.6,
            0.5,
            0.2,
            1.4,
            0.03,
            vec![PitchSkill::from_prob(
                PitchType::FourSeamFastball,
                148.0,
                0.75,
                0.8,
                0.1,
                2300.0,
                180.0,
                0.95,
                0.6,
            )],
            FielderInfo::new_pitcher(),
        ));
        player
    }

    fn batter_player() -> Player {
        let mut player = player(2, "Batter", Position::SS);
        player.offense_skills.batter = Some(BatterInfo {
            batting_side: RL::Left,
            batter_type: BatterType::ClassicAnalyst,
            zone_aptitude: ZoneAptitude::Balanced,
            hot_zone_scale: 0.1,
            batting_eye: 0.5,
            swing_speed: 30.0,
            swing_power: 1.0,
            attack_angle: 28.0,
            bat_control: 0.8,
            consistency: 0.03,
        });
        player.offense_skills.running = RunningSkills {
            speed: 7.5,
            lead_distance: 2.0,
            start_reaction: 0.3,
        };
        player.defense_skills.middle_infielder = Some(FielderInfo {
            fielder_type: crate::domain::shared::player::FielderType::MiddleInfielder,
            throw_speed: 38.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.6,
            catching: 0.8,
            reach_height: 2.5,
            reach_range: 1.0,
        });
        player
    }

    #[test]
    fn player_info_lane_labels_match_requested_list() {
        let labels = TeamPlayerInfoLane::ALL
            .iter()
            .map(|lane| lane.label())
            .collect::<Vec<_>>();

        assert_eq!(
            labels,
            vec!["Pitcher", "Catcher", "Infielder", "Outfielder"]
        );
    }

    #[test]
    fn player_info_lanes_group_actual_players_by_position() {
        let mut widget = PlayerInfoWidget::new();
        widget.players = vec![
            player(1, "Pitcher", Position::P),
            player(2, "Catcher", Position::C),
            player(3, "First", Position::FB),
            player(4, "Short", Position::SS),
            player(5, "Designated", Position::DH),
            player(6, "Left", Position::LF),
        ];

        assert_eq!(
            widget
                .players_for_lane(TeamPlayerInfoLane::Pitcher)
                .into_iter()
                .map(Player::full_name)
                .collect::<Vec<_>>(),
            vec!["Pitcher Player"]
        );
        assert_eq!(
            widget
                .players_for_lane(TeamPlayerInfoLane::Catcher)
                .into_iter()
                .map(Player::full_name)
                .collect::<Vec<_>>(),
            vec!["Catcher Player"]
        );
        assert_eq!(
            widget
                .players_for_lane(TeamPlayerInfoLane::Infielder)
                .into_iter()
                .map(Player::full_name)
                .collect::<Vec<_>>(),
            vec!["First Player", "Short Player", "Designated Player"]
        );
        assert_eq!(
            widget
                .players_for_lane(TeamPlayerInfoLane::Outfielder)
                .into_iter()
                .map(Player::full_name)
                .collect::<Vec<_>>(),
            vec!["Left Player"]
        );
    }

    #[test]
    fn player_info_selection_starts_on_first_nonempty_lane() {
        let mut widget = PlayerInfoWidget::new();
        widget.players = vec![player(2, "Catcher", Position::C)];

        widget.reset_selection();

        assert_eq!(widget.player_info_lane, TeamPlayerInfoLane::Catcher);
        assert_eq!(widget.catcher_state.selected(), Some(0));
        assert_eq!(
            widget.selected_player().map(Player::full_name),
            Some("Catcher Player".to_string())
        );
    }

    #[test]
    fn player_info_tab_navigates_players_and_lanes() {
        let mut widget = PlayerInfoWidget::new();
        widget.players = vec![
            player(1, "Pitcher", Position::P),
            player(3, "First", Position::FB),
            player(4, "Short", Position::SS),
        ];
        widget.reset_selection();

        assert!(widget.update_player_info(Action::NextCount));
        assert_eq!(widget.player_info_lane, TeamPlayerInfoLane::Catcher);

        assert!(widget.update_player_info(Action::NextCount));
        assert_eq!(widget.player_info_lane, TeamPlayerInfoLane::Infielder);
        assert_eq!(
            widget.selected_player().map(Player::full_name),
            Some("First Player".to_string())
        );

        assert!(widget.update_player_info(Action::SelectNext));
        assert_eq!(
            widget.selected_player().map(Player::full_name),
            Some("Short Player".to_string())
        );
    }

    #[test]
    fn player_info_confirm_opens_and_back_closes_player_detail() {
        let mut widget = PlayerInfoWidget::new();
        widget.players = vec![pitcher_player()];
        widget.reset_selection();

        assert!(widget.update_player_info(Action::ConfirmSelection));
        assert_eq!(widget.selected_player_id, Some(1));

        assert!(widget.update_player_info(Action::Back));
        assert_eq!(widget.selected_player_id, None);
    }

    #[test]
    fn pitcher_detail_confirm_opens_pitch_skills_and_back_returns_to_summary() {
        let mut widget = PlayerInfoWidget::new();
        widget.players = vec![pitcher_player()];
        widget.reset_selection();
        widget.open_selected_player_detail();

        assert!(widget.update_player_info(Action::ConfirmSelection));
        assert_eq!(
            widget.selected_player_detail_pane,
            PlayerDetailPane::PitchSkills
        );

        assert!(widget.update_player_info(Action::Back));
        assert_eq!(
            widget.selected_player_detail_pane,
            PlayerDetailPane::Summary
        );
        assert_eq!(widget.selected_player_id, Some(1));
    }

    #[test]
    fn player_detail_primary_title_matches_player_role() {
        assert_eq!(
            PlayerInfoWidget::player_detail_primary_title(&pitcher_player()),
            "Player / Pitcher Info"
        );
        assert_eq!(
            PlayerInfoWidget::player_detail_primary_title(&batter_player()),
            "Player / Batter Info"
        );
        assert_eq!(
            PlayerInfoWidget::player_detail_primary_footer(&pitcher_player()),
            Some("Hit return key for Pitch Skills")
        );
        assert_eq!(
            PlayerInfoWidget::player_detail_primary_footer(&batter_player()),
            None
        );
    }

    #[test]
    fn pitcher_detail_omits_batter_and_running_sections() {
        let (primary_detail, skill_detail) =
            PlayerInfoWidget::format_player_detail_sections(&pitcher_player());

        assert!(primary_detail.contains("Pitcher"));
        assert!(primary_detail.contains("Velocity"));
        assert!(!primary_detail.contains("Batter"));
        assert!(!primary_detail.contains("Pitch Skills"));
        assert!(!primary_detail.contains("Four Seam Fastball"));
        assert!(skill_detail.contains("Fielder"));
        assert!(!skill_detail.contains("Runner"));
        assert!(!skill_detail.contains("Speed: 7.50"));
        assert!(!skill_detail.contains("Lead Distance"));
    }

    #[test]
    fn non_pitcher_detail_includes_present_batter_running_and_fielding_sections() {
        let (primary_detail, skill_detail) =
            PlayerInfoWidget::format_player_detail_sections(&batter_player());

        assert!(primary_detail.contains("Batter"));
        assert!(primary_detail.contains("Swing Speed"));
        assert!(!primary_detail.contains("Runner"));
        assert!(!primary_detail.contains("Middle Infielder"));
        assert!(skill_detail.contains("Runner"));
        assert!(skill_detail.contains("Middle Infielder"));
        assert!(skill_detail.contains("Lead Distance"));
        assert!(!primary_detail.contains("Pitch Skills"));
        assert!(!skill_detail.contains("Pitch Skills"));
    }

    #[test]
    fn pitch_skills_detail_lists_pitch_skill_parameters() {
        let detail = PlayerInfoWidget::format_pitch_skills_detail(&pitcher_player());

        assert!(detail.contains("Four Seam Fastball"));
        assert!(detail.contains("Velocity: 148.00"));
        assert!(detail.contains("Spin Efficiency: 0.95"));
        assert!(detail.contains("Usage: 0.60"));
    }

    #[test]
    fn pitch_skills_detail_scrolls_with_select_actions() {
        let mut widget = PlayerInfoWidget::new();
        widget.players = vec![pitcher_player()];
        widget.reset_selection();
        widget.open_selected_player_detail();
        widget.open_selected_pitch_skills();

        assert!(widget.update_player_info(Action::SelectNext));
        assert_eq!(widget.pitch_skills_scroll_offset, 1);

        assert!(widget.update_player_info(Action::SelectPrevious));
        assert_eq!(widget.pitch_skills_scroll_offset, 0);
    }
}
