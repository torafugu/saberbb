use crate::domain::shared::player::{FielderType, HitterTendency, PitcherStyle, RL};
use serde::{Deserialize, Serialize};
use validator::Validate;

#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, Validate)]
pub struct NormalParam {
    pub mean: f64,
    #[validate(range(min = -5.0, max = 5.0))]
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
    #[validate(range(min = 0.0, max = 1.0))]
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
    pub swing_speed: NormalParam,
    pub hitter_tendency: Vec<ItemWeighted<HitterTendency>>,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Validate)]
pub struct FielderInfoProbs {
    pub fielder_type: Vec<ItemWeighted<FielderType>>,
    pub throw_speed: NormalParam,
    pub running_speed: NormalParam,
    pub reaction: NormalParam,
    pub prep_time: NormalParam,
}

#[derive(Clone, Default, Serialize, Deserialize, Debug, Validate)]
pub struct PitcherInfoProbs {
    pub pitcher_style: Vec<ItemWeighted<PitcherStyle>>,
    pub velocity: NormalParam,
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
    pub stuff: NormalParam,
    pub fb: NormalParam,
    pub gp: NormalParam,
    pub horizontal_movement: NormalParam,
    pub vertical_movement: NormalParam,
    pub spin_rate: NormalParam,
    pub usage: NormalParam,
}
