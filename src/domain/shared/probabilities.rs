use super::player::PitchType;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PlayerAttributeProb {
    pub age_shape: f64,
    pub age_scale: f64,
    pub age_offset: f64,
    pub throw_lefty: f64,
    pub bat_lefty: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct BatterSkillProb {
    pub ba_skew: f64,
    pub slg_skew: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct DefensiveSkillProb {
    pub uzr_skew: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PitcherAttributeProb {
    pub velocity_skew: f64,
    pub control_skew: f64,
    pub stamina_skew: f64,
    pub injury_proneness_skew: f64,
    pub clutch_skew: f64,
    pub hpp_skew: f64,
    pub platoon_splitting_skew: f64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct PitchSkillProb {
    pub pitch_type: PitchType,
    pub velocity_skew: f64,
    pub control_skew: f64,
    pub stamina_skew: f64,
    pub injury_proneness_skew: f64,
    pub stuff_skew: f64,
    pub fb_skew: f64,
    pub gp_skew: f64,
    pub horizontal_movement_skew: f64,
    pub vertical_movement_skew: f64,
    pub spin_rate_skew: f64,
    pub usage_skew: f64,
}
