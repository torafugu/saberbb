use crate::domain::shared::player::{ArmSlot, FielderType, HitterTendency, PitcherStyle, RL};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Validate)]
pub struct NormalParam {
    pub mean: f64,
    pub std_dev: f64,
    pub skew: f64,
    pub coefficient: f64,
    pub offset: f64,
}

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Validate)]
/// shape(k)
/// scale(θ)
pub struct GammaParam {
    #[validate(range(min = 0.0, max = 5.0))]
    pub shape: f64,
    #[validate(range(min = 0.0, max = 5.0))]
    pub scale: f64,
    pub offset: f64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Validate)]
pub struct ItemWeighted<T> {
    pub name: T,
    #[validate(range(min = 0.0, max = 1.0))]
    pub weight: f64,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Validate)]
pub struct PlayerInfoProbs {
    pub age: GammaParam,
}

#[derive(Debug, Default, Clone, Serialize, Deserialize, Validate)]
pub struct RunningSkillProbs {
    pub speed: NormalParam,
    pub lead_distance: NormalParam,
    pub start_reaction: NormalParam,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Validate)]
pub struct BatterInfoProbs {
    pub batting_side: Vec<ItemWeighted<RL>>,
    pub batting_eye: NormalParam,
    pub swing_speed: NormalParam,
    pub swing_power: NormalParam,
    pub attack_angle: NormalParam,
    pub bat_contact: NormalParam,
    pub timing_bias: NormalParam,
    pub consistency_sigma: NormalParam,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Validate)]
pub struct FielderInfoProbs {
    pub fielder_type: Vec<ItemWeighted<FielderType>>,
    pub throw_speed: NormalParam,
    pub running_speed: NormalParam,
    pub reaction: NormalParam,
    pub prep_time: NormalParam,
    pub catching: NormalParam,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Validate)]
pub struct PitcherInfoProbs {
    pub height: NormalParam,
    pub extension: NormalParam,
    pub throw_side: Vec<ItemWeighted<RL>>,
    pub arm_slot: Vec<ItemWeighted<ArmSlot>>,
    pub pitcher_style: Vec<ItemWeighted<PitcherStyle>>,
    pub velocity: NormalParam,
    pub spin_rate: NormalParam,
    pub control: NormalParam,
    pub stamina: NormalParam,
    pub injury_proneness: NormalParam,
    pub clutch: NormalParam,
    pub hpp: NormalParam,
    pub platoon_splitting: NormalParam,
    pub delivery_motion_time: NormalParam,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Validate)]
pub struct PitchSkillProbs {
    pub velocity: NormalParam,
    pub control: NormalParam,
    pub stamina: NormalParam,
    pub injury_proneness: NormalParam,
    pub spin_rate: NormalParam,
    pub spin_angle: NormalParam,
    pub spin_efficiency: NormalParam,
    pub usage: NormalParam,
}
