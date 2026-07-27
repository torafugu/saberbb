use crate::I18nManager;
use crate::domain::resolver::batting_resolver::FieldSector;
use crate::domain::shared::game_state::GameError;
use crate::t;
use serde::{Deserialize, Serialize};
use std::fmt;
use strum_macros::{AsRefStr, EnumIter, EnumString};
use validator::Validate;

#[derive(Clone, Copy, Hash, PartialEq, Eq, EnumString, Serialize, Deserialize, Debug, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum Position {
    P,
    C,
    FB,
    SB,
    TB,
    SS,
    LF,
    CF,
    RF,
    DH,
}
impl Position {
    pub const ALL: [Position; 10] = [
        Position::P,
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

    pub fn is_outfielder(self) -> bool {
        matches!(self, Position::LF | Position::CF | Position::RF)
    }

    pub fn is_infielder(self) -> bool {
        matches!(
            self,
            Position::P | Position::C | Position::FB | Position::SB | Position::TB | Position::SS
        )
    }

    pub fn is_middle_infielder(self) -> bool {
        matches!(self, Position::SB | Position::SS)
    }

    pub fn is_corner_infielder(self) -> bool {
        matches!(self, Position::FB | Position::TB)
    }

    pub fn short(&self) -> LongPositionFormatter<'_> {
        LongPositionFormatter(self)
    }
}

impl std::fmt::Display for Position {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Position::P => write!(f, "{}", t!("p")),
            Position::C => write!(f, "{}", t!("c")),
            Position::FB => write!(f, "{}", t!("fb")),
            Position::SB => write!(f, "{}", t!("sb")),
            Position::TB => write!(f, "{}", t!("tb")),
            Position::SS => write!(f, "{}", t!("ss")),
            Position::LF => write!(f, "{}", t!("lf")),
            Position::CF => write!(f, "{}", t!("cf")),
            Position::RF => write!(f, "{}", t!("rf")),
            Position::DH => write!(f, "{}", t!("dh")),
        }
    }
}

pub struct LongPositionFormatter<'a>(&'a Position);

impl<'a> std::fmt::Display for LongPositionFormatter<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self.0 {
            Position::P => write!(f, "{}", t!("pitcher")),
            Position::C => write!(f, "{}", t!("catcher")),
            Position::FB => write!(f, "{}", t!("first_baseman")),
            Position::SB => write!(f, "{}", t!("second_baseman")),
            Position::TB => write!(f, "{}", t!("third_baseman")),
            Position::SS => write!(f, "{}", t!("shortstop")),
            Position::LF => write!(f, "{}", t!("left_fielder")),
            Position::CF => write!(f, "{}", t!("center_fielder")),
            Position::RF => write!(f, "{}", t!("right_fielder")),
            Position::DH => write!(f, "{}", t!("dh")),
        }
    }
}

// CONSTRAINT: DH is not a FielderType
#[derive(
    Clone, Copy, PartialEq, Serialize, Deserialize, EnumString, EnumIter, Eq, Hash, Debug, AsRefStr,
)]
#[strum(ascii_case_insensitive)]
pub enum FielderType {
    Outfielder,
    MiddleInfielder,
    CornerInfielder,
    Pitcher,
    Catcher,
}
impl std::fmt::Display for FielderType {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            FielderType::Outfielder => write!(f, "{}", t!("outfielder")),
            FielderType::MiddleInfielder => write!(f, "{}", t!("middle_infielder")),
            FielderType::CornerInfielder => write!(f, "{}", t!("corner_infielder")),
            FielderType::Pitcher => write!(f, "{}", t!("pitcher")),
            FielderType::Catcher => write!(f, "{}", t!("catcher")),
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Serialize, Deserialize, EnumString, Debug, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum RL {
    #[default]
    Right,
    Left,
}
impl fmt::Display for RL {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            RL::Right => write!(f, "{}", t!("right")),
            RL::Left => write!(f, "{}", t!("left")),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct FullName {
    pub first: String,
    pub last: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerInfo {
    pub id: i64, // CONSTRAINT: rusqlite does not support u64 and usize
    pub first_name: String,
    pub last_name: String,
    pub age: u8,
    pub uniform_number: u8,
}
impl PlayerInfo {
    pub fn new_unsaved(first_name: String, last_name: String, age: u8, uniform_number: u8) -> Self {
        Self {
            id: 0,
            first_name: first_name,
            last_name: last_name,
            age: age,
            uniform_number: uniform_number,
        }
    }

    pub fn new(
        id: i64,
        first_name: String,
        last_name: String,
        age: u8,
        uniform_number: u8,
    ) -> Self {
        Self {
            id: id,
            first_name: first_name,
            last_name: last_name,
            age: age,
            uniform_number: uniform_number,
        }
    }

    pub fn new_min(id: i64, first_name: String, last_name: String) -> Self {
        Self {
            id: id,
            first_name: first_name,
            last_name: last_name,
            age: 0,
            uniform_number: 0,
        }
    }

    pub fn full_name(&self) -> String {
        I18nManager::global().full_name(&self.first_name, &self.last_name)
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct OffenseSkills {
    pub batter: Option<BatterInfo>,
    pub running: RunningSkills,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct DefenseSkills {
    pub position: Position,
    pub pitcher: Option<PitcherInfo>,
    pub catcher: Option<CatcherInfo>,
    pub middle_infielder: Option<FielderInfo>,
    pub corner_infielder: Option<FielderInfo>,
    pub outfielder: Option<FielderInfo>,
}
impl DefenseSkills {
    pub fn new(position: Position) -> Self {
        Self {
            position,
            pitcher: None,
            catcher: None,
            middle_infielder: None,
            corner_infielder: None,
            outfielder: None,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct Player {
    pub info: PlayerInfo,
    pub offense_skills: OffenseSkills,
    pub defense_skills: DefenseSkills,
}
impl Player {
    pub fn from_player_info(info: PlayerInfo) -> Self {
        Self {
            info: PlayerInfo::new(
                info.id,
                info.first_name,
                info.last_name,
                info.age,
                info.uniform_number,
            ),
            offense_skills: OffenseSkills {
                batter: None,
                running: RunningSkills {
                    speed: 0.0,
                    lead_distance: 0.0,
                    start_reaction: 0.0,
                },
            },
            defense_skills: DefenseSkills::new(Position::DH),
        }
    }

    pub fn is(&self, id: i64) -> bool {
        self.info.id == id
    }

    pub fn full_name(&self) -> String {
        self.info.full_name()
    }

    pub fn batter(&self) -> Result<BatterInfo, GameError> {
        if let Some(batter) = self.offense_skills.batter {
            Ok(batter)
        } else {
            return Err(GameError::BatterInfo);
        }
    }

    pub fn runner(&self) -> RunningSkills {
        self.offense_skills.running
    }

    pub fn pitcher(&self) -> Result<PitcherInfo, GameError> {
        if let Some(pitcher) = &self.defense_skills.pitcher {
            Ok(pitcher.clone())
        } else {
            return Err(GameError::PitcherInfo);
        }
    }

    pub fn catcher(&self) -> Result<CatcherInfo, GameError> {
        if let Some(catcher) = self.defense_skills.catcher {
            Ok(catcher)
        } else {
            return Err(GameError::PitcherInfo);
        }
    }

    pub fn fielder(&self) -> Result<FielderInfo, GameError> {
        if self.defense_skills.position == Position::P {
            if let Some(pitcher) = &self.defense_skills.pitcher {
                Ok(pitcher.fielder_info)
            } else {
                return Err(GameError::FielderInfo);
            }
        } else if self.defense_skills.position == Position::C {
            if let Some(catcher) = self.defense_skills.catcher {
                Ok(catcher.fielder_info)
            } else {
                return Err(GameError::FielderInfo);
            }
        } else if self.defense_skills.position.is_middle_infielder() {
            if let Some(middle_infielder) = self.defense_skills.middle_infielder {
                Ok(middle_infielder)
            } else {
                return Err(GameError::FielderInfo);
            }
        } else if self.defense_skills.position.is_corner_infielder() {
            if let Some(corner_infielder) = self.defense_skills.corner_infielder {
                Ok(corner_infielder)
            } else {
                return Err(GameError::FielderInfo);
            }
        } else if self.defense_skills.position.is_outfielder() {
            if let Some(outfielder) = self.defense_skills.outfielder {
                Ok(outfielder)
            } else {
                return Err(GameError::FielderInfo);
            }
        } else {
            return Err(GameError::FielderInfo);
        }
    }
}
#[derive(
    Clone, Copy, PartialEq, Eq, Serialize, Deserialize, EnumString, EnumIter, Hash, Debug, AsRefStr,
)]
#[strum(ascii_case_insensitive)]
pub enum HitterTendency {
    Normal,
    Pull,
    Spray,
}
impl fmt::Display for HitterTendency {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            HitterTendency::Normal => write!(f, "{}", t!("normal")),
            HitterTendency::Pull => write!(f, "{}", t!("pull")),
            HitterTendency::Spray => write!(f, "{}", t!("spray")),
        }
    }
}

#[derive(Clone, Copy, Default, Serialize, Deserialize, Debug, Validate)]
pub struct BatterInfo {
    pub batting_side: RL,
    pub swing_speed: f64,
    pub base_launch_angle: f64, // Ex. 28.0 deg
    pub consistency_sigma: f64, // Ex. 0.03

    // NOTE: Weight of probability for each sector (set to sum to 1.0)
    // CONSTRAINT: Randomize based on batter type
    pub weight_foul_pull: f64,
    pub weight_pull: f64,
    pub weight_center: f64,
    pub weight_opposite: f64,
    pub weight_foul_opposite: f64,
}
impl BatterInfo {
    // Returns the concrete angle range (min, max) for the selected sector
    pub fn get_angle_range(&self, sector: FieldSector) -> (f32, f32) {
        match self.batting_side {
            RL::Right => match sector {
                FieldSector::FoulPull => (-90.0, -45.0),
                FieldSector::Pull => (-45.0, -15.0), // Right-handed batter's pull → left field (-)
                FieldSector::Center => (-15.0, 15.0),
                FieldSector::Opposite => (15.0, 45.0), // Right-handed batter's opposite → right field (+)
                FieldSector::FoulOpposite => (45.0, 90.0),
            },
            RL::Left => match sector {
                FieldSector::FoulOpposite => (-90.0, -45.0),
                FieldSector::Opposite => (-45.0, -15.0), // Left-handed batter's opposite → left field (-)
                FieldSector::Center => (-15.0, 15.0),
                FieldSector::Pull => (15.0, 45.0), // Left-handed batter's pull → right field (+)
                FieldSector::FoulPull => (45.0, 90.0),
            },
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug, Validate)]
pub struct RunningSkills {
    pub speed: f64,          // NOTE: Base running speed (m/s) e.g. 7.7
    pub lead_distance: f64,  // NOTE: Current lead distance (m), valid when current_base > 0
    pub start_reaction: f64, // TODO: judge mechanism should be implemented.
}

#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug, Validate)]
pub struct FielderInfo {
    pub fielder_type: FielderType,
    pub throw_speed: f64,   // NOTE: Throw speed (m/s) e.g. 35.0 – 42.0 m/s
    pub running_speed: f64, // NOTE: Running speed (m/s) e.g. 6.5 – 8.0 m/s
    pub reaction: f64,      // NOTE: Reaction time (seconds) e.g. 0.3 – 0.7 s (lower is better)
    pub prep_time: f64, // NOTE: Pitch preparation / transfer time (seconds) e.g. 0.5 – 0.8 s (lower is better)
}
impl FielderInfo {
    pub fn new_pitcher() -> Self {
        Self {
            fielder_type: FielderType::Pitcher,
            throw_speed: 0.0,
            running_speed: 0.0,
            reaction: 0.0,
            prep_time: 0.0,
        }
    }
}

// TODO: Consider calling pitches skill
#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct CatcherInfo {
    pub fielder_info: FielderInfo,
}
impl CatcherInfo {
    pub fn from_fielder_info(fielder_info: FielderInfo) -> Self {
        Self {
            fielder_info: fielder_info,
        }
    }
}

#[derive(
    Clone, Copy, Serialize, Deserialize, EnumString, EnumIter, Debug, PartialEq, Eq, Hash, AsRefStr,
)]
#[strum(ascii_case_insensitive)]
pub enum PitchType {
    FourSeamFastball,
    TwoSeamFastball,
    Cutter,
    Curveball,
    Slider,
    Sweeper,
    Changeup,
    Forkball,
    SplitFingerFastball,
    Knuckleball,
}
impl fmt::Display for PitchType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            PitchType::FourSeamFastball => write!(f, "{}", t!("four_seam_fastball")),
            PitchType::TwoSeamFastball => write!(f, "{}", t!("two_seam_fastball")),
            PitchType::Cutter => write!(f, "{}", t!("cutter")),
            PitchType::Curveball => write!(f, "{}", t!("curveball")),
            PitchType::Slider => write!(f, "{}", t!("slider")),
            PitchType::Sweeper => write!(f, "{}", t!("sweeper")),
            PitchType::Changeup => write!(f, "{}", t!("changeup")),
            PitchType::Forkball => write!(f, "{}", t!("forkball")),
            PitchType::SplitFingerFastball => write!(f, "{}", t!("split_finger_fastball")),
            PitchType::Knuckleball => write!(f, "{}", t!("knuckleball")),
        }
    }
}

#[derive(
    Clone, Copy, Serialize, Deserialize, EnumString, EnumIter, Debug, PartialEq, Eq, Hash, AsRefStr,
)]
#[strum(ascii_case_insensitive)]
pub enum PitcherStyle {
    PowerPitcher,
    FinessePitcher,
    BalancedPitcher,
}
impl fmt::Display for PitcherStyle {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            PitcherStyle::PowerPitcher => write!(f, "{}", t!("power_pitcher")),
            PitcherStyle::FinessePitcher => write!(f, "{}", t!("finesse_pitcher")),
            PitcherStyle::BalancedPitcher => write!(f, "{}", t!("balanced_pitcher")),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PitcherInfo {
    pub pitcher_style: PitcherStyle,
    pub velocity: f64,
    pub control: f64,
    pub stamina: f64,
    pub injury_proneness: f64,
    pub clutch: f64,
    pub hpp: f64, // NOTE: Home-Away Splitting
    pub platoon_splitting: f64,
    pub delivery_motion_time: f64,
    pub pitch_skills: Vec<PitchSkill>,
    pub fielder_info: FielderInfo,
}
impl PitcherInfo {
    pub fn from_prob(
        pitcher_style: PitcherStyle,
        velocity: f64,
        control: f64,
        stamina: f64,
        injury_proneness: f64,
        clutch: f64,
        hpp: f64,
        platoon_splitting: f64,
        delivery_motion_time: f64,
        pitch_skills: Vec<PitchSkill>,
        fielder_info: FielderInfo,
    ) -> Self {
        Self {
            pitcher_style: pitcher_style,
            velocity,
            control,
            stamina,
            injury_proneness,
            clutch,
            hpp,
            platoon_splitting,
            delivery_motion_time: delivery_motion_time,
            pitch_skills: pitch_skills,
            fielder_info: fielder_info,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PitchSkill {
    pub pitch_type: PitchType,
    pub velocity: f64,
    pub control: f64,
    pub stamina: f64,
    pub injury_proneness: f64,
    pub stuff: f64,
    pub fb: f64, // NOTE: Home Run to Fly Ball Rate
    pub gp: f64, // NOTE: Grounder Percentage
    pub horizontal_movement: f64,
    pub vertical_movement: f64,
    pub spin_rate: f64,
    pub usage: f64, // TODO: Should be over written by strategy
}

impl PitchSkill {
    pub fn from_prob(
        pitch_type: PitchType,
        velocity: f64,
        control: f64,
        stamina: f64,
        injury_proneness: f64,
        stuff: f64,
        fb: f64,
        gp: f64,
        horizontal_movement: f64,
        vertical_movement: f64,
        spin_rate: f64,
        usage: f64,
    ) -> Self {
        Self {
            pitch_type: pitch_type,
            velocity,
            control,
            stamina,
            injury_proneness,
            stuff,
            fb,
            gp,
            horizontal_movement,
            vertical_movement,
            spin_rate,
            usage,
        }
    }
}
