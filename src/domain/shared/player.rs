use crate::I18nManager;
use crate::domain::resolver::fielding_config::{
    FENCE_BOUNCE_COEFF, FENCE_DISTANCE, FIRST_BOUNCE_TIME, LINER_REACTION_TIME,
};
use crate::domain::resolver::fielding_resolver::BoundedBallResult;
use crate::domain::shared::ball::{BattedBall, FieldedBall, TrajectoryType};
use crate::domain::util::{PolarPosition, calculate_polar_distance, sigmoid};
use crate::t;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::sync::Arc;
use strum_macros::{AsRefStr, EnumString};
use validator::Validate;

const BATTING_MIN_HIT_AVERAGE: f64 = 0.2;
const BATTING_MAX_HIT_AVERAGE: f64 = 0.32;
const BATTING_MIN_SLG: f64 = 0.3;
const BATTING_MAX_SLG: f64 = 0.55;

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
    pub const ALL_NO_DH: [Position; 9] = [
        Position::P,
        Position::C,
        Position::FB,
        Position::SB,
        Position::TB,
        Position::SS,
        Position::LF,
        Position::CF,
        Position::RF,
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

#[derive(Clone, PartialEq, Serialize, Deserialize, EnumString, Debug, AsRefStr)]
#[strum(ascii_case_insensitive)]
pub enum RL {
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
    pub first: Arc<str>,
    pub last: Arc<str>,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct Player {
    pub id: u32, // in case of same first_name and last_name
    pub first_name: Arc<str>,
    pub last_name: Arc<str>,
    pub age: u8,
    pub throw: RL,
    pub defensive_skills: Vec<DefensiveSkill>,
    pub pitcher_attribute: Option<PitcherAttribute>,
    pub bat: RL,
    pub mod_ba: f64,
    pub mod_slg: f64,
}

impl Player {
    pub fn new_unsaved(
        first_name: &str,
        last_name: &str,
        age: u8,
        throw: RL,
        defensive_skills: Vec<DefensiveSkill>,
        pitcher_attribute: Option<PitcherAttribute>,
        bat: RL,
        mod_ba: f64,
        mod_slg: f64,
    ) -> Self {
        Self {
            id: 0,
            first_name: Arc::from(first_name),
            last_name: Arc::from(last_name),
            age: age,
            throw: throw,
            defensive_skills: defensive_skills,
            pitcher_attribute: pitcher_attribute,
            bat: bat,
            mod_ba: mod_ba,
            mod_slg: mod_slg,
        }
    }

    pub fn new(
        id: u32,
        first_name: &str,
        last_name: &str,
        age: u8,
        throw: RL,
        bat: RL,
        mod_ba: f64,
        mod_slg: f64,
    ) -> Self {
        Self {
            id: id,
            first_name: Arc::from(first_name),
            last_name: Arc::from(last_name),
            age: age,
            throw: throw,
            defensive_skills: Vec::new(),
            pitcher_attribute: None,
            bat: bat,
            mod_ba: mod_ba,
            mod_slg: mod_slg,
        }
    }

    pub fn full_name(&self) -> String {
        I18nManager::global().full_name(&self.first_name, &self.last_name)
    }

    pub fn hit_average(&self) -> f64 {
        let ba = (BATTING_MAX_HIT_AVERAGE + BATTING_MIN_HIT_AVERAGE) * 0.5
            + (BATTING_MAX_HIT_AVERAGE - BATTING_MIN_HIT_AVERAGE) * (sigmoid(self.mod_ba) - 0.5);
        ba
    }

    pub fn slg(&self) -> f64 {
        let slg = (BATTING_MAX_SLG + BATTING_MIN_SLG) * 0.5
            + (BATTING_MAX_SLG - BATTING_MIN_SLG) * (sigmoid(self.mod_slg) - 0.5);
        slg
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Runner {
    pub speed: f64,          // NOTE: Base running speed (m/s) e.g. 7.7
    pub lead_distance: f64,  // NOTE: Current lead distance (m), valid when current_base > 0
    pub start_reaction: f64, // TODO: judge mechanism should be implemented.
}

// TODO: merge into Player
#[derive(Debug, Clone)]
pub struct Fielder {
    pub position: Position,
    pub polar_position: PolarPosition, // TODO: Should be moved to game_state struct
    pub throw_speed: f64,              // Throw speed (m/s) e.g. 35.0 – 42.0 m/s
    pub running_speed: f64,            // Running speed (m/s) e.g. 6.5 – 8.0 m/s
    pub reaction: f64,                 // Reaction time (seconds) e.g. 0.3 – 0.7 s (lower is better)
    pub prep_time: f64, // Pitch preparation / transfer time (seconds) e.g. 0.5 – 0.8 s (lower is better)
}

impl Fielder {
    pub fn new(
        position: Position,
        distance: f64,
        angle: f64,
        throw_speed: f64,
        running_speed: f64,
        reaction: f64,
        prep_time: f64,
    ) -> Self {
        Self {
            position: position,
            polar_position: PolarPosition::new(distance, angle),
            throw_speed: throw_speed,
            running_speed: running_speed,
            reaction: reaction,
            prep_time: prep_time,
        }
    }

    pub fn is(&self, position: Position) -> bool {
        self.position == position
    }

    pub fn distance(&self) -> f64 {
        self.polar_position.distance
    }

    pub fn angle(&self) -> f64 {
        self.polar_position.angle
    }

    pub fn x(&self) -> f64 {
        self.polar_position.x
    }

    pub fn y(&self) -> f64 {
        self.polar_position.y
    }

    pub fn try_catch(&self, ball: &BattedBall) -> FieldedBall {
        // $$\text{arrival\_time} = \text{reaction\_time} + \frac{\text{required\_distance}}{\text{fielder\_speed}}$$
        // 1. Calculate straight-line distance from position to landing point
        let required_distance =
            calculate_polar_distance(&self.polar_position, &ball.polar_position);
        let dy = self.y() - ball.y();

        // 3. Adjust initial reaction speed based on hit type (secret ingredient)
        let mut final_reaction = self.reaction;
        if ball.trajectory == TrajectoryType::Liner && dy < 0.0 {
            // Delay reaction when moving forward on a liner (harder to judge)
            final_reaction += LINER_REACTION_TIME;
        }

        // 4. Calculate arrival time (seconds)
        let arrival_time = final_reaction + (required_distance / self.running_speed);

        // 5. Compare arrival time vs hang time
        if ball.trajectory == TrajectoryType::Grounder {
            return FieldedBall {
                ball: ball.clone(),
                fielded_by: self.position,
                time_to_field: arrival_time,
                is_fly_catch: false,
            };
        }

        if arrival_time <= ball.hang_time {
            return FieldedBall {
                ball: ball.clone(),
                fielded_by: self.position,
                time_to_field: ball.hang_time, // Fielder need to wait until catch.
                is_fly_catch: true,
            };
        }

        let bounded_ball = self.process_bounded_ball(ball);

        let mut final_ball = ball.clone();
        final_ball.polar_position.distance = bounded_ball.final_distance;

        FieldedBall {
            ball: final_ball,
            fielded_by: self.position,
            time_to_field: bounded_ball.time_to_fumble,
            is_fly_catch: false,
        }
    }

    // Processing when a fly/liner wasn't caught (became a hit)
    fn process_bounded_ball(&self, ball: &BattedBall) -> BoundedBallResult {
        // 1. Damping coefficient at the moment of the first bounce (liner bounces sharply, fly dies)
        let k_impact = match ball.trajectory {
            TrajectoryType::Liner => 0.60,
            TrajectoryType::Fly => 0.35,
            _ => 0.0,
        };

        // 2. Initial speed as a grounder right after the bounce
        let v_horizontal = ball.launch_speed_ms() * ball.azimuth().cos() * 0.7; // Velocity including in-flight air resistance
        let v_bounce = v_horizontal * k_impact;

        // 3. Additional rolling distance and time until stop
        let roll_distance = v_bounce * 1.8;

        // 4. Provisional final resting position (landing point + roll distance)
        let mut final_distance = ball.distance() + roll_distance;

        // The fence bounce (cushion) logic
        if final_distance > FENCE_DISTANCE {
            let overflow = final_distance - FENCE_DISTANCE;
            final_distance = FENCE_DISTANCE - (overflow * FENCE_BOUNCE_COEFF);
        }

        // 5. Defense: time for the fielder to chase down and pick up the rolling ball
        // The fielder was initially running toward the landing point but didn't make it.
        // Simple calculation of time to loop around toward the direction the ball rolled (final_distance)
        let fielder_distance_to_ball = (final_distance - self.distance()).abs();

        // Time for the fielder to reach the final resting point (or cushion treatment position)
        let fielder_arrival_time = self.reaction + (fielder_distance_to_ball / self.running_speed);

        // Time the fielder picks up the ball (either waiting for it to stop or cutting it off mid-roll)
        let time_to_pick_up = fielder_arrival_time.max(ball.hang_time + FIRST_BOUNCE_TIME);
        BoundedBallResult {
            final_distance,
            time_to_fumble: time_to_pick_up, // ★This becomes the time_to_field for the next throw play!
        }
    }
}

#[derive(Debug)]
pub struct PitcherData {
    pub delivery_motion_time: f64,
}

#[derive(Debug)]
pub struct CatcherData {
    pub prep_time: f64,
    pub throw_speed: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct DefensiveSkill {
    pub position: Position,
    pub mod_uzr: f64,
}

#[derive(Clone, Serialize, Deserialize, EnumString, Debug, AsRefStr)]
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

#[derive(Clone, Serialize, Deserialize, EnumString, Debug, AsRefStr)]
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
pub struct PitcherAttribute {
    pub pitcher_style: PitcherStyle,
    pub mod_velocity: f64,
    pub mod_control: f64,
    pub mod_stamina: f64,
    pub mod_injury_proneness: f64,
    pub mod_clutch: f64,
    pub mod_hpp: f64, // Home-Away Splitting
    pub mod_platoon_splitting: f64,
    pub pitch_skills: Vec<PitchSkill>,
}
impl PitcherAttribute {
    pub fn from_prob(
        pitcher_style: PitcherStyle,
        mod_velocity: f64,
        mod_control: f64,
        mod_stamina: f64,
        mod_injury_proneness: f64,
        mod_clutch: f64,
        mod_hpp: f64,
        mod_platoon_splitting: f64,
        pitch_skills: Vec<PitchSkill>,
    ) -> Self {
        Self {
            pitcher_style: pitcher_style,
            mod_velocity: mod_velocity,
            mod_control: mod_control,
            mod_stamina: mod_stamina,
            mod_injury_proneness: mod_injury_proneness,
            mod_clutch: mod_clutch,
            mod_hpp: mod_hpp,
            mod_platoon_splitting: mod_platoon_splitting,
            pitch_skills: pitch_skills,
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PitchSkill {
    pub pitch_type: PitchType,
    pub mod_velocity: f64,
    pub mod_control: f64,
    pub mod_stamina: f64,
    pub mod_injury_proneness: f64,
    pub mod_stuff: f64,
    pub mod_fb: f64, // Home Run to Fly Ball Rate
    pub mod_gp: f64, // Grounder Percentage
    pub mod_horizontal_movement: f64,
    pub mod_vertical_movement: f64,
    pub mod_spin_rate: f64,
    pub mod_usage: f64, // TODO: Should be over written by strategy
}

impl PitchSkill {
    pub fn from_prob(
        pitch_type: PitchType,
        mod_velocity: f64,
        mod_control: f64,
        mod_stamina: f64,
        mod_injury_proneness: f64,
        mod_stuff: f64,
        mod_fb: f64,
        mod_gp: f64,
        mod_horizontal_movement: f64,
        mod_vertical_movement: f64,
        mod_spin_rate: f64,
        mod_usage: f64,
    ) -> Self {
        Self {
            pitch_type: pitch_type,
            mod_velocity: mod_velocity,
            mod_control: mod_control,
            mod_stamina: mod_stamina,
            mod_injury_proneness: mod_injury_proneness,
            mod_stuff: mod_stuff,
            mod_fb: mod_fb,
            mod_gp: mod_gp,
            mod_horizontal_movement: mod_horizontal_movement,
            mod_vertical_movement: mod_vertical_movement,
            mod_spin_rate: mod_spin_rate,
            mod_usage: mod_usage,
        }
    }
}
