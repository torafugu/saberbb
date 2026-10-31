use super::game::BattingResult;
use crate::domain::resolver::batting_resolver::{SwingContactResult, SwingContactType};
use crate::domain::resolver::fielding_physics::{FielderCandidateDiagnostics, FieldingCatchDiagnostics};
use crate::domain::resolver::fielding_resolver::PlayType;
use crate::domain::resolver::pitching_resolver::{LocationBias, PitchDisplacement};
use crate::domain::resolver::running_resolver::RunningEvent;
use crate::domain::shared::ball::{BallMovement, BattedBall, PitchedBall};
use crate::domain::shared::game_state::{GameError, Ruling};
use crate::domain::shared::player::{PlayerInfo, Position};
use crate::domain::shared::stadium::Base;
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameEntryView {
    pub start_count_seq: u16,
    pub end_count_seq: u16,
    pub team_id: u16,
    pub position: Position,
    pub batting_order: u8,
    pub player: PlayerInfo,
}
impl PlayerGameEntryView {
    pub fn is_position(&self, team_id: u16, position: Position, count_seq: u16) -> bool {
        self.team_id == team_id
            && self.position == position
            && self.start_count_seq <= count_seq
            && self.end_count_seq >= count_seq
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameBattingView {
    pub count_seq: u16,
    pub pitcher_id: i64,
    pub batter: PlayerInfo,
    pub ball: BattedBall,
    pub fielder_position: Option<Position>,
    pub result: BattingResult,
}
impl PlayerGameBattingView {
    pub fn is(&self, count_seq: u16) -> bool {
        self.count_seq == count_seq
    }

    pub fn outcome(&self) -> Result<String, GameError> {
        match self.result {
            BattingResult::Out => {
                if let Some(position) = self.fielder_position {
                    let mut outcome = position.long().to_string();
                    outcome.push_str(" ");
                    outcome.push_str(&self.ball.trajectory().to_string());
                    Ok(outcome)
                } else {
                    return Err(GameError::FielderInfo);
                }
            }
            _ => Ok(self.result.to_string()),
        }
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameHomeRunView {
    pub count_seq: u16,
    pub batter: PlayerInfo,
    pub season_home_runs: u16,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGamePitchingView {
    pub count_seq: u16,
    pub pitcher_id: i64,
    pub ball: PitchedBall,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGamePitchingDecisionView {
    pub pitcher: PlayerInfo,
    pub decision: String,
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameRunningView {
    pub count_seq: u16,
    pub seq: u8,
    pub defense_time: f64,
    pub runner_time: f64,
    pub throw_target_base: Base,
    pub target_runner: Option<PlayerInfo>,
    pub event: RunningEvent,
    pub play_type: PlayType,
    pub ruling: Ruling,
    pub runs_scored: u8,
    pub runner_1st: Option<PlayerInfo>,
    pub runner_2nd: Option<PlayerInfo>,
    pub runner_3rd: Option<PlayerInfo>,
}
impl PlayerGameRunningView {
    pub fn is(&self, count_seq: u16, seq: u8) -> bool {
        self.count_seq == count_seq && self.seq == seq
    }
}

#[derive(Clone, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameEntry {
    pub start_count_seq: u16,
    pub end_count_seq: Option<u16>,
    pub position: Position,
    pub batting_order: u8,
    pub player_id: i64,
}
impl PlayerGameEntry {
    pub fn new(
        start_count_seq: u16,
        end_count_seq: Option<u16>,
        position: Position,
        batting_order: u8,
        player_id: i64,
    ) -> Self {
        Self {
            start_count_seq: start_count_seq,
            end_count_seq: end_count_seq,
            position: position,
            batting_order: batting_order,
            player_id: player_id,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGamePitching {
    pub count_seq: u16,
    pub pitcher_id: i64,
    pub ball: PitchedBall,
    pub ball_movement: BallMovement,
    pub location_bias: LocationBias,
    pub pitch_displacement: PitchDisplacement,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameBattingMetrics {
    #[serde(default)]
    pub timing_position_scale: Option<f64>,
    #[serde(default)]
    pub actual_bat_angle_deg: Option<f64>,
    pub adapted_horizontal_offset_m: Option<f64>,
    pub adapted_vertical_offset_m: Option<f64>,
    pub adapted_timing_offset_sec: Option<f64>,
    pub timing_impact_x_m: Option<f64>,
    pub swing_error_x_m: Option<f64>,
    pub swing_error_z_m: Option<f64>,
    pub contact_offset_x_m: Option<f64>,
    pub contact_offset_z_m: Option<f64>,
    pub thickness_offset_m: Option<f64>,
    pub length_offset_m: f64,
    pub original_contact_type: SwingContactType,
    pub adjusted_contact_type: SwingContactType,
    pub timing_error: f64,
    pub bat_speed: f64,
    pub angular_velocity: f64,
    pub approach_variation_deg: f64,
    pub pull_bias_deg: f64,
    pub timing_angle_error: f64,
    pub final_hla_deg: f64,
}
impl PlayerGameBattingMetrics {
    /// Capture the contact calculation for every swing, including misses and fouls.
    pub fn from_contact(contact: &SwingContactResult) -> Self {
        Self {
            timing_position_scale: contact.timing_position_scale,
            actual_bat_angle_deg: contact.actual_bat_angle_deg,
            adapted_horizontal_offset_m: Some(contact.adapted_horizontal_offset_m),
            adapted_vertical_offset_m: Some(contact.adapted_vertical_offset_m),
            adapted_timing_offset_sec: Some(contact.timing_offset_sec),
            timing_impact_x_m: Some(contact.timing_impact_x_m),
            swing_error_x_m: Some(contact.swing_error_x_m),
            swing_error_z_m: Some(contact.swing_error_z_m),
            contact_offset_x_m: Some(contact.offset_x_m),
            contact_offset_z_m: Some(contact.offset_z_m),
            thickness_offset_m: Some(contact.thickness_offset_m),
            length_offset_m: contact.length_offset_m,
            original_contact_type: contact.original_contact_type,
            adjusted_contact_type: contact.adjusted_contact_type,
            timing_error: contact.timing_offset_sec,
            ..Default::default()
        }
    }
}

impl Default for PlayerGameBattingMetrics {
    fn default() -> Self {
        Self {
            timing_position_scale: None,
            actual_bat_angle_deg: None,
            adapted_horizontal_offset_m: None,
            adapted_vertical_offset_m: None,
            adapted_timing_offset_sec: None,
            timing_impact_x_m: None,
            swing_error_x_m: None,
            swing_error_z_m: None,
            contact_offset_x_m: None,
            contact_offset_z_m: None,
            thickness_offset_m: None,
            length_offset_m: 0.0,
            original_contact_type: SwingContactType::Take,
            adjusted_contact_type: SwingContactType::Take,
            timing_error: 0.0,
            bat_speed: 0.0,
            angular_velocity: 0.0,
            approach_variation_deg: 0.0,
            pull_bias_deg: 0.0,
            timing_angle_error: 0.0,
            final_hla_deg: 0.0,
        }
    }
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameBatting {
    #[serde(default)]
    pub timing_position_scale: Option<f64>,
    #[serde(default)]
    pub actual_bat_angle_deg: Option<f64>,
    #[serde(default)]
    pub swing_decision: Option<crate::domain::resolver::batting_resolver::SwingDecisionDiagnostics>,
    pub count_seq: u16,
    pub pitcher_id: i64,
    pub batter_id: i64,
    pub timing_error: f64,
    pub bat_speed: f64,
    pub angular_velocity: f64,
    pub approach_variation_deg: f64,
    pub pull_bias_deg: f64,
    pub timing_angle_error: f64,
    pub final_hla_deg: f64,
    pub adapted_horizontal_offset_m: Option<f64>,
    pub adapted_vertical_offset_m: Option<f64>,
    pub adapted_timing_offset_sec: Option<f64>,
    pub timing_impact_x_m: Option<f64>,
    pub swing_error_x_m: Option<f64>,
    pub swing_error_z_m: Option<f64>,
    pub contact_offset_x_m: Option<f64>,
    pub contact_offset_z_m: Option<f64>,
    pub thickness_offset_m: Option<f64>,
    pub length_offset_m: f64,
    pub original_contact_type: SwingContactType,
    pub adjusted_contact_type: SwingContactType,
    pub ball: BattedBall,
    pub fielder_position: Option<Position>,
    pub result: BattingResult,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameFieldingCandidate {
    pub count_seq: u16,
    pub diagnostics: FielderCandidateDiagnostics,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameFielding {
    /// None for legacy records and subsequent double-play throws.
    #[serde(default)]
    pub catch_diagnostics: Option<FieldingCatchDiagnostics>,
    pub count_seq: u16,
    pub seq: u8,
    pub catch_fielder_id: i64,
    pub catch_fielder_position: Position,
    pub cutoff_fielder_id: Option<i64>,
    pub cutoff_fielder_position: Option<Position>,
    pub final_fielder_id: Option<i64>,
    pub final_fielder_position: Option<Position>,
    pub time_to_field: f64,
    pub play_type: PlayType,
}

#[derive(Clone, Copy, Serialize, Deserialize, Debug, Validate)]
pub struct PlayerGameRunning {
    pub count_seq: u16,
    pub seq: u8,
    pub defense_time: f64,
    pub runner_time: f64,
    pub throw_target_base: Base,
    pub target_runner_id: Option<i64>,
    pub event: RunningEvent,
    pub play_type: PlayType,
    pub ruling: Ruling,
    pub runs_scored: u8,
    pub runner_1st_id: Option<i64>,
    pub runner_2nd_id: Option<i64>,
    pub runner_3rd_id: Option<i64>,
}
