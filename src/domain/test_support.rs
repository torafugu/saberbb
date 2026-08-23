use crate::domain::random_provider::FixedRng;
use crate::domain::resolver::fielding_physics::FielderRiskTolerance;
use crate::domain::shared::game::GameSeason;
use crate::domain::shared::game::{GameSchedule, GameType};
use crate::domain::shared::game_state::{ActiveFielder, ActiveRunner, GameState};
use crate::domain::shared::player::{
    ArmSlot, BatterInfo, BatterType, CatcherInfo, DefenseSkills, FielderInfo, FielderType,
    OffenseSkills, PitchSkill, PitchType, PitcherInfo, PitcherStyle, Player, PlayerInfo, Position,
    RL, RunningSkills, ZoneAptitude,
};
use crate::domain::shared::stadium::{MOUND_DISTANCE, Stadium};
use crate::domain::shared::team::League;
use crate::domain::shared::team::Team;
use crate::domain::util::PolarPosition;
use crate::error::AppError;
use crate::repositories::schedule_repository::ScheduleRepository;
use anyhow::anyhow;
use chrono::NaiveDate;
use std::cell::{Cell, RefCell};

pub(crate) fn test_date() -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 4, 1).expect("fixed test date must be valid")
}

pub(crate) fn running_skills() -> RunningSkills {
    RunningSkills {
        speed: 7.7,
        lead_distance: 4.0,
        start_reaction: 0.1,
    }
}

pub(crate) fn active_runner(id: i64) -> ActiveRunner {
    ActiveRunner {
        id,
        skills: running_skills(),
    }
}

pub(crate) fn batter_info(batting_side: RL) -> BatterInfo {
    BatterInfo {
        batting_side,
        batter_type: BatterType::ClassicAnalyst,
        zone_aptitude: ZoneAptitude::Balanced,
        hot_zone_scale: 0.1,
        batting_eye: 0.5,
        swing_speed: 150.0,
        swing_power: 1.0,
        attack_angle: 28.0,
        bat_control: 0.8,
        consistency: 0.03,
    }
}

pub(crate) fn fielder_info(fielder_type: FielderType) -> FielderInfo {
    FielderInfo {
        fielder_type,
        throw_speed: 35.0,
        running_speed: 7.0,
        reaction: 0.4,
        prep_time: 0.6,
        catching: 0.8,
        reach_height: 2.5,
        reach_range: 0.0,
    }
}

pub(crate) fn catcher_info() -> CatcherInfo {
    CatcherInfo {
        fielder_info: fielder_info(FielderType::Catcher),
    }
}

pub(crate) fn pitch_skill(pitch_type: PitchType) -> PitchSkill {
    PitchSkill {
        pitch_type,
        velocity: 150.0,
        control: 0.5,
        stamina: 0.5,
        injury_proneness: 0.5,
        spin_rate: 2400.0,
        spin_angle: 180.0,
        spin_efficiency: 0.95,
        usage: 0.7,
    }
}

pub(crate) fn pitcher_info() -> PitcherInfo {
    PitcherInfo {
        height: 1.85,
        extension: 1.8,
        throw_side: RL::Right,
        arm_slot: ArmSlot::ThreeQuarter,
        pitcher_style: PitcherStyle::BalancedPitcher,
        velocity: 150.0,
        spin_rate: 2400.0,
        control: 0.5,
        stamina: 0.5,
        injury_proneness: 0.5,
        clutch: 0.5,
        hpp: 0.5,
        platoon_splitting: 0.5,
        delivery_motion_time: 1.4,
        consistency: 0.03,
        pitch_skills: vec![
            pitch_skill(PitchType::FourSeamFastball),
            PitchSkill {
                pitch_type: PitchType::Slider,
                velocity: 135.0,
                usage: 0.3,
                ..pitch_skill(PitchType::Slider)
            },
        ],
        fielder_info: fielder_info(FielderType::Pitcher),
    }
}

pub(crate) fn active_fielder(position: Position, distance: f64, angle: f64) -> ActiveFielder {
    ActiveFielder {
        position,
        id: 0,
        info: fielder_info(fielder_type_for(position)),
        polar_position: PolarPosition::new(distance, angle),
        risk_tolerance: FielderRiskTolerance::Balanced,
    }
}

pub(crate) fn default_fielders() -> [ActiveFielder; 9] {
    [
        active_fielder(Position::P, MOUND_DISTANCE, 0.0),
        active_fielder(Position::C, 0.0, 0.0),
        active_fielder(Position::FB, 35.0, 33.0),
        active_fielder(Position::SB, 40.0, 18.0),
        active_fielder(Position::TB, 35.0, -33.0),
        active_fielder(Position::SS, 35.0, -33.0),
        active_fielder(Position::RF, 80.0, 26.0),
        active_fielder(Position::CF, 90.0, 0.0),
        active_fielder(Position::LF, 80.0, -26.0),
    ]
}

pub(crate) fn player(id: i64, position: Position, batting_order: Option<u8>) -> Player {
    let mut defense_skills = DefenseSkills::new(position);
    match position {
        Position::P => defense_skills.pitcher = Some(pitcher_info()),
        Position::C => defense_skills.catcher = Some(catcher_info()),
        Position::SB | Position::SS => {
            defense_skills.middle_infielder = Some(fielder_info(FielderType::MiddleInfielder));
        }
        Position::FB | Position::TB => {
            defense_skills.corner_infielder = Some(fielder_info(FielderType::CornerInfielder));
        }
        Position::LF | Position::CF | Position::RF => {
            defense_skills.outfielder = Some(fielder_info(FielderType::Outfielder));
        }
        Position::DH => {}
    }

    let mut batter = (position != Position::P).then(|| batter_info(RL::Right));
    if let Some(order) = batting_order {
        if let Some(batter) = &mut batter {
            batter.swing_speed = 200.0 - f64::from(order);
        }
    }

    Player {
        info: PlayerInfo::new_min(id, format!("First{id}"), format!("Last{id}")),
        offense_skills: OffenseSkills {
            batter,
            running: running_skills(),
        },
        defense_skills,
    }
}

pub(crate) fn team(id: u16, name: &str, first_player_id: i64) -> Team {
    let batter_positions = [
        Position::C,
        Position::FB,
        Position::SB,
        Position::TB,
        Position::SS,
        Position::LF,
        Position::CF,
        Position::RF,
        Position::DH,
    ];
    let mut players = vec![player(first_player_id, Position::P, None)];
    players.extend(
        batter_positions
            .into_iter()
            .enumerate()
            .map(|(index, position)| {
                player(
                    first_player_id + index as i64 + 1,
                    position,
                    Some((index + 1) as u8),
                )
            }),
    );

    Team {
        id,
        name: name.into(),
        players,
    }
}

pub(crate) fn minimal_team(id: u16) -> Team {
    Team::min(id, &format!("Team{id}"))
}

pub(crate) fn league(id: u16, first_team_id: u16) -> League {
    League {
        id,
        name: format!("League{id}").into(),
        teams: (first_team_id..first_team_id + 6)
            .map(minimal_team)
            .collect(),
    }
}

pub(crate) fn game_schedule() -> GameSchedule {
    GameSchedule {
        id: 1,
        season: 2026,
        round_seq: 1,
        seq: 4,
        planned_date: test_date(),
        away_team: team(1, "AAA", 1),
        home_team: team(2, "BBB", 101),
        stadium: Stadium::default(),
        game_type: GameType::Regular,
    }
}

pub(crate) fn game_state() -> GameState {
    GameState::new(Box::new(FixedRng::new(0.5)), game_schedule())
        .expect("test game schedule should produce valid lineups")
}

pub(crate) struct FakeScheduleRepository {
    pub game_season: GameSeason,
    pub leagues: Vec<League>,
    pub load_game_season_error: bool,
    pub load_all_leagues_error: bool,
    pub save_error_at: Option<usize>,
    pub update_error: bool,
    pub load_game_season_calls: Cell<usize>,
    pub load_all_leagues_calls: Cell<usize>,
    pub update_calls: Cell<usize>,
    pub saved_batches: Vec<Vec<GameSchedule>>,
    pub call_log: RefCell<Vec<&'static str>>,
}

impl FakeScheduleRepository {
    pub(crate) fn new(leagues: Vec<League>) -> Self {
        Self {
            game_season: GameSeason {
                start_date: test_date(),
                season: 2026,
            },
            leagues,
            load_game_season_error: false,
            load_all_leagues_error: false,
            save_error_at: None,
            update_error: false,
            load_game_season_calls: Cell::new(0),
            load_all_leagues_calls: Cell::new(0),
            update_calls: Cell::new(0),
            saved_batches: Vec::new(),
            call_log: RefCell::new(Vec::new()),
        }
    }
}

impl ScheduleRepository for FakeScheduleRepository {
    fn load_game_season(&self) -> Result<GameSeason, AppError> {
        self.call_log.borrow_mut().push("load_game_season");
        self.load_game_season_calls
            .set(self.load_game_season_calls.get() + 1);

        if self.load_game_season_error {
            return Err(AppError::Internal(anyhow!("load game season failed")));
        }

        Ok(GameSeason {
            start_date: self.game_season.start_date,
            season: self.game_season.season,
        })
    }

    fn load_all_leagues(&self) -> Result<Vec<League>, AppError> {
        self.call_log.borrow_mut().push("load_all_leagues");
        self.load_all_leagues_calls
            .set(self.load_all_leagues_calls.get() + 1);

        if self.load_all_leagues_error {
            return Err(AppError::Internal(anyhow!("load all leagues failed")));
        }

        Ok(self.leagues.clone())
    }

    fn save_game_schedules(&mut self, game_schedules: Vec<GameSchedule>) -> Result<(), AppError> {
        self.call_log.borrow_mut().push("save_game_schedules");
        let call_index = self.saved_batches.len();

        if self.save_error_at == Some(call_index) {
            return Err(AppError::Internal(anyhow!("save game schedules failed")));
        }

        self.saved_batches.push(game_schedules);
        Ok(())
    }

    fn update_scheduled_season(&self) -> Result<usize, AppError> {
        self.call_log.borrow_mut().push("update_scheduled_season");
        self.update_calls.set(self.update_calls.get() + 1);

        if self.update_error {
            return Err(AppError::Internal(anyhow!(
                "update scheduled season failed"
            )));
        }

        Ok(1)
    }
}

fn fielder_type_for(position: Position) -> FielderType {
    match position {
        Position::P => FielderType::Pitcher,
        Position::C => FielderType::Catcher,
        Position::SB | Position::SS => FielderType::MiddleInfielder,
        Position::FB | Position::TB => FielderType::CornerInfielder,
        Position::LF | Position::CF | Position::RF => FielderType::Outfielder,
        Position::DH => FielderType::Outfielder,
    }
}
