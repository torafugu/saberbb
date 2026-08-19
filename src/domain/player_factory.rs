use super::player_service::PlayerService;
use super::shared::player::{
    CatcherInfo, FielderInfo, FielderType, OffenseSkills, PitchSkill, PitchType, PitcherInfo,
    PitcherStyle, Player, Position, RunningSkills,
};
use super::shared::prob::{
    BatterInfoProbs, FielderInfoProbs, PitchSkillProbs, PitcherInfoProbs, PlayerInfoProbs,
    RunningSkillProbs,
};
use crate::domain::random_provider::{
    RandomProvider, RealRng, choose_item_if_exists, choose_item_weighted,
};
use crate::domain::shared::player::{BatterInfo, DefenseSkills, PlayerInfo};
use crate::domain::shared::prob::ItemWeighted;
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::repositories::player_repository::PlayerRepository;
use anyhow::Result;
use std::collections::HashMap;
use tracing::info;

pub struct PlayerFactory<R: PlayerRepository> {
    service: PlayerService<R>,
    rng: Box<dyn RandomProvider>,
    player_info_probs: PlayerInfoProbs,
    running_skill_probs: RunningSkillProbs,
    batter_info_probs: BatterInfoProbs,
    fielder_info_probs: FielderInfoProbs,
    pitcher_info_probs: PitcherInfoProbs,
    pitch_type_map: HashMap<PitcherStyle, Vec<ItemWeighted<PitchType>>>,
    pitch_skill_map: HashMap<PitchType, PitchSkillProbs>,
}
impl<R: PlayerRepository> PlayerFactory<R> {
    pub fn new(service: PlayerService<R>) -> Self {
        Self {
            service: service,
            rng: Box::new(RealRng::new()),
            player_info_probs: PlayerInfoProbs::default(),
            running_skill_probs: RunningSkillProbs::default(),
            batter_info_probs: BatterInfoProbs::default(),
            fielder_info_probs: FielderInfoProbs::default(),
            pitcher_info_probs: PitcherInfoProbs::default(),
            pitch_type_map: HashMap::new(),
            pitch_skill_map: HashMap::new(),
        }
    }

    pub fn load_player_probs(&mut self) -> Result<(), AppError> {
        info!("load_player_probs() started");

        self.player_info_probs = self.service.load_player_info_probs()?;
        self.running_skill_probs = self.service.load_running_skill_probs()?;
        self.batter_info_probs = self.service.load_batter_info_probs()?;
        self.fielder_info_probs = self.service.load_fielder_info_probs()?;
        self.pitcher_info_probs = self.service.load_pitcher_info_prob()?;
        self.player_info_probs = self.service.load_player_info_probs()?;
        self.pitch_type_map = self.service.load_pitch_type_prob()?;
        self.pitch_skill_map = self.service.load_pitch_skill_probs()?;

        Ok(())
    }

    pub fn generate_and_save_player(&mut self) -> Result<()> {
        info!("generate_and_save_players() started");

        let player = self.generate_player()?;
        let team = self.assign_team(&player)?;

        self.service.save_player(team.id, &player)?;

        Ok(())
    }

    pub fn generate_player(&mut self) -> Result<Player> {
        info!("generate_player() started");

        let player_info = self.assign_player_info()?;

        let fielder_type_1st =
            *choose_item_weighted(self.rng.as_mut(), &self.fielder_info_probs.fielder_type)?;

        // NOTE: DH comes here.
        let primary_position = self.assign_position(&fielder_type_1st)?;

        let multiple_fielder_type_probs = self
            .service
            .load_multiple_fielder_type_prob(&fielder_type_1st)?;
        let mut fielder_types = if multiple_fielder_type_probs.is_empty() {
            Vec::new()
        } else {
            choose_item_if_exists(self.rng.as_mut(), &multiple_fielder_type_probs)?
        };
        fielder_types.push(fielder_type_1st);

        let mut defense_skills = DefenseSkills::new(primary_position);

        for fielder_type in fielder_types {
            let fileder_info = self.assign_fielder_info(&fielder_type)?;
            match fielder_type {
                FielderType::Outfielder => defense_skills.outfielder = Some(fileder_info),
                FielderType::MiddleInfielder => {
                    defense_skills.middle_infielder = Some(fileder_info)
                }
                FielderType::CornerInfielder => {
                    defense_skills.corner_infielder = Some(fileder_info)
                }
                FielderType::Pitcher => {
                    defense_skills.pitcher = Some(self.assign_pitcher_info(&fileder_info)?)
                }
                FielderType::Catcher => {
                    defense_skills.catcher = Some(CatcherInfo {
                        fielder_info: fileder_info,
                    })
                }
            }
        }

        let batter_info = if primary_position == Position::P {
            // TODO: Consider pitcher has second position
            None
        } else {
            Some(self.assign_batter_info()?)
        };

        let offense_skills = OffenseSkills {
            batter: batter_info,
            running: self.assign_running_skills()?,
        };

        Ok(Player {
            info: player_info,
            offense_skills: offense_skills,
            defense_skills: defense_skills,
        })
    }

    pub fn default_fielder_info(fielder_type: FielderType) -> FielderInfo {
        FielderInfo {
            fielder_type,
            throw_speed: 40.0,
            running_speed: 7.0,
            reaction: 0.5,
            prep_time: 0.65,
            catching: 0.8,
        }
    }

    fn assign_player_info(&mut self) -> Result<PlayerInfo, AppError> {
        let name = self.service.load_random_name()?;

        // TODO: Consider uniqueness and ability for uniform number
        // TODO: Consider correlation　of throw and batting side
        Ok(PlayerInfo::new_unsaved(
            name.first,
            name.last,
            self.rng.gamma(self.player_info_probs.age).round() as u8,
            self.rng.gen_range(0, 100) as u8,
        ))
    }

    pub fn assign_batter_info(&mut self) -> Result<BatterInfo, AppError> {
        let batting_side =
            choose_item_weighted(self.rng.as_mut(), &self.batter_info_probs.batting_side)?.clone();

        let batting_eye = self.rng.normal(self.batter_info_probs.batting_eye);
        let swing_speed = self.rng.normal(self.batter_info_probs.swing_speed);
        let swing_power = self.rng.normal(self.batter_info_probs.swing_power);
        let attack_angle = self.rng.normal(self.batter_info_probs.attack_angle);
        let bat_contact = self.rng.normal(self.batter_info_probs.bat_contact);
        let timing_bias = self.rng.normal(self.batter_info_probs.timing_bias);
        let consistency_sigma = self.rng.normal(self.batter_info_probs.consistency_sigma);

        Ok(BatterInfo {
            batting_side: batting_side,
            batting_eye: batting_eye,
            swing_speed: swing_speed,
            swing_power: swing_power,
            attack_angle: attack_angle,
            bat_contact: bat_contact,
            timing_bias: timing_bias,
            consistency_sigma: consistency_sigma,
        })
    }

    fn assign_running_skills(&mut self) -> Result<RunningSkills, AppError> {
        Ok(RunningSkills {
            speed: self.rng.normal(self.running_skill_probs.speed),
            lead_distance: self.rng.normal(self.running_skill_probs.lead_distance),
            start_reaction: self.rng.normal(self.running_skill_probs.start_reaction),
        })
    }

    fn assign_position(&mut self, fielder_type: &FielderType) -> Result<Position, AppError> {
        let mut items = Vec::new();
        match fielder_type {
            FielderType::Outfielder => {
                items.push(ItemWeighted {
                    name: Position::RF,
                    weight: 0.32,
                });
                items.push(ItemWeighted {
                    name: Position::CF,
                    weight: 0.32,
                });
                items.push(ItemWeighted {
                    name: Position::LF,
                    weight: 0.32,
                });
                items.push(ItemWeighted {
                    name: Position::DH,
                    weight: 0.04,
                });
                Ok(choose_item_weighted(self.rng.as_mut(), &items)?.clone())
            }
            FielderType::MiddleInfielder => {
                items.push(ItemWeighted {
                    name: Position::SS,
                    weight: 0.48,
                });
                items.push(ItemWeighted {
                    name: Position::SB,
                    weight: 0.52,
                });
                Ok(choose_item_weighted(self.rng.as_mut(), &items)?.clone())
            }
            FielderType::CornerInfielder => {
                items.push(ItemWeighted {
                    name: Position::FB,
                    weight: 0.5,
                });
                items.push(ItemWeighted {
                    name: Position::TB,
                    weight: 0.4,
                });
                items.push(ItemWeighted {
                    name: Position::DH,
                    weight: 0.1,
                });
                Ok(choose_item_weighted(self.rng.as_mut(), &items)?.clone())
            }
            FielderType::Pitcher => Ok(Position::P),
            FielderType::Catcher => Ok(Position::C),
        }
    }

    fn assign_fielder_info(&mut self, fielder_type: &FielderType) -> Result<FielderInfo> {
        Ok(FielderInfo {
            fielder_type: fielder_type.clone(),
            throw_speed: self.rng.normal(self.fielder_info_probs.throw_speed),
            running_speed: self.rng.normal(self.fielder_info_probs.running_speed),
            reaction: self.rng.normal(self.fielder_info_probs.reaction),
            prep_time: self.rng.normal(self.fielder_info_probs.prep_time),
            catching: self.rng.normal(self.fielder_info_probs.catching),
        })
    }

    fn assign_pitcher_info(&mut self, fielder_info: &FielderInfo) -> Result<PitcherInfo> {
        let throw_side =
            choose_item_weighted(self.rng.as_mut(), &self.pitcher_info_probs.throw_side)?.clone();
        let arm_slot =
            choose_item_weighted(self.rng.as_mut(), &self.pitcher_info_probs.arm_slot)?.clone();
        let pitcher_style =
            choose_item_weighted(self.rng.as_mut(), &self.pitcher_info_probs.pitcher_style)?
                .clone();
        let pitch_skills = self.assign_pitch_skill(&pitcher_style)?;

        Ok(PitcherInfo {
            height: self.rng.normal(self.pitcher_info_probs.height),
            extension: self.rng.normal(self.pitcher_info_probs.extension),
            throw_side: throw_side,
            arm_slot: arm_slot,
            pitcher_style: pitcher_style,
            velocity: self.rng.normal(self.pitcher_info_probs.velocity),
            spin_rate: self.rng.normal(self.pitcher_info_probs.spin_rate),
            control: self.rng.normal(self.pitcher_info_probs.control),
            stamina: self.rng.normal(self.pitcher_info_probs.stamina),
            injury_proneness: self.rng.normal(self.pitcher_info_probs.injury_proneness),
            clutch: self.rng.normal(self.pitcher_info_probs.injury_proneness),
            hpp: self.rng.normal(self.pitcher_info_probs.hpp),
            platoon_splitting: self.rng.normal(self.pitcher_info_probs.platoon_splitting),
            delivery_motion_time: self
                .rng
                .normal(self.pitcher_info_probs.delivery_motion_time),
            pitch_skills: pitch_skills,
            fielder_info: fielder_info.clone(),
        })
    }

    fn assign_pitch_skill(
        &mut self,
        pitcher_style: &PitcherStyle,
    ) -> Result<Vec<PitchSkill>, AppError> {
        let pitch_types = if let Some(pitch_type_all) = &self.pitch_type_map.get(&pitcher_style) {
            choose_item_if_exists(self.rng.as_mut(), &pitch_type_all)?
        } else {
            return Err(AppError::NotFound("pitch_types_all".to_string()));
        };

        let mut pitch_skills: Vec<PitchSkill> = Vec::new();

        for pitch_type in pitch_types {
            if let Some(pitch_skill_prob) = &self.pitch_skill_map.get(&pitch_type) {
                let pitch_skill = PitchSkill {
                    pitch_type: pitch_type,
                    velocity: self.rng.normal(pitch_skill_prob.velocity),
                    control: self.rng.normal(pitch_skill_prob.control),
                    stamina: self.rng.normal(pitch_skill_prob.stamina),
                    injury_proneness: self.rng.normal(pitch_skill_prob.injury_proneness),
                    spin_rate: self.rng.normal(pitch_skill_prob.spin_rate),
                    spin_angle: self.rng.normal(pitch_skill_prob.spin_angle),
                    spin_efficiency: self.rng.normal(pitch_skill_prob.spin_efficiency),
                    usage: self.rng.normal(pitch_skill_prob.usage),
                };
                pitch_skills.push(pitch_skill);
            }
        }
        Ok(pitch_skills)
    }

    fn assign_team(&self, player: &Player) -> Result<Team, AppError> {
        self.service.next_team(player.defense_skills.position)
    }
}
