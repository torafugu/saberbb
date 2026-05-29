use super::player_service::PlayerService;
use super::shared::player::{
    DefensiveSkill, PitchSkill, PitcherAttribute, PitcherStyle, Player, Position,
};
use super::shared::prob::{DefensiveSkillProb, PitcherAttributeProb};
use super::utils::{age_random, choose_item_weighted, rl_random, skewed_normal_random};
use crate::domain::shared::prob::ItemProb;
use crate::repositories::player_repository::PlayerRepository;
use crate::t;
use anyhow::{Result, bail};

pub struct PlayerFactory<R: PlayerRepository> {
    service: PlayerService<R>,
}
impl<R: PlayerRepository> PlayerFactory<R> {
    pub fn new(service: PlayerService<R>) -> Self {
        Self { service }
    }

    pub fn generate_players(&mut self, num_of_players: u16) -> Result<()> {
        let player_prob = self.service.load_player_probs()?;

        for _ in 0..num_of_players {
            let name = self.service.load_random_name()?;
            let age = age_random(
                player_prob.player_attribute_prob.age_shape,
                player_prob.player_attribute_prob.age_scale,
                player_prob.player_attribute_prob.age_offset,
            );
            let throw = rl_random(player_prob.player_attribute_prob.throw_lefty);
            let bat = rl_random(player_prob.player_attribute_prob.bat_lefty);
            let mod_ba = skewed_normal_random(player_prob.batter_skill_prob.ba_skew);
            let mod_slg = skewed_normal_random(player_prob.batter_skill_prob.slg_skew);

            let mut defensive_skills = Vec::new();
            // TODO: Should be changed to multiple skills
            let defensive_skill = self.assign_defensive_skills(
                &player_prob.position_probs,
                &player_prob.defensive_skill_prob,
            )?;

            let mut pitcher_skill = None;
            if defensive_skill.position == Position::P {
                pitcher_skill = Some(self.assign_pitcher_skill(
                    &player_prob.pitcher_style_probs,
                    &player_prob.pitcher_attribute_prob,
                )?);
            };
            defensive_skills.push(defensive_skill);

            // Assign player to team
            let team = self
                .service
                .next_team(defensive_skills[0].position.clone())?;

            let player = Player {
                id: 0,
                first_name: name.first,
                last_name: name.last,
                age: age,
                throw: throw,
                // TODO: consider multiple skill holder
                defensive_skills: defensive_skills,
                pitcher_attribute: pitcher_skill,
                bat: bat,
                mod_ba: mod_ba,
                mod_slg: mod_slg,
            };

            self.service.save_player(team, player)?;
        }

        Ok(())
    }

    pub fn assign_defensive_skills(
        &self,
        position_probs: &Vec<ItemProb<Position>>,
        defensive_skill_prob: &DefensiveSkillProb,
    ) -> Result<DefensiveSkill> {
        let position = match choose_item_weighted(&position_probs) {
            Some(chosen) => chosen.clone(),
            None => {
                bail!(t!("error", "function" => "choose_item_weighted"));
            }
        };

        let defensive_skill = DefensiveSkill {
            position: position,
            mod_uzr: skewed_normal_random(defensive_skill_prob.uzr_skew),
        };

        Ok(defensive_skill)
    }

    pub fn assign_pitcher_skill(
        &self,
        pitcher_style_probs: &Vec<ItemProb<PitcherStyle>>,
        pitcher_base_skill_prob: &PitcherAttributeProb,
    ) -> Result<PitcherAttribute> {
        let pitcher_style = match choose_item_weighted(&pitcher_style_probs) {
            Some(chosen) => chosen.clone(),
            None => {
                bail!(t!("error", "function" => "choose_item_weighted"));
            }
        };

        let pitch_type_probs = self.service.pitch_type_probs(&pitcher_style)?;

        let mut pitcher_skill = PitcherAttribute {
            pitcher_style: pitcher_style,
            mod_velocity: skewed_normal_random(pitcher_base_skill_prob.velocity_skew),
            mod_control: skewed_normal_random(pitcher_base_skill_prob.control_skew),
            mod_stamina: skewed_normal_random(pitcher_base_skill_prob.control_skew),
            mod_injury_proneness: skewed_normal_random(
                pitcher_base_skill_prob.injury_proneness_skew,
            ),
            mod_clutch: skewed_normal_random(pitcher_base_skill_prob.clutch_skew),
            mod_hpp: skewed_normal_random(pitcher_base_skill_prob.hpp_skew),
            mod_platoon_splitting: skewed_normal_random(
                pitcher_base_skill_prob.platoon_splitting_skew,
            ),
            pitch_skills: Vec::new(),
        };

        let mut pitch_skills: Vec<PitchSkill> = Vec::new();
        for pitch_type_prob in pitch_type_probs {
            let rng: f64 = rand::random();
            if rng < pitch_type_prob.prob {
                let pitch_skill_prob = self.service.pitch_skill_prob(&pitch_type_prob.name)?;
                let pitch_skill = PitchSkill {
                    pitch_type: pitch_type_prob.name.clone(),
                    mod_velocity: skewed_normal_random(pitch_skill_prob.velocity_skew),
                    mod_control: skewed_normal_random(pitch_skill_prob.control_skew),
                    mod_stamina: skewed_normal_random(pitch_skill_prob.stamina_skew),
                    mod_injury_proneness: skewed_normal_random(
                        pitch_skill_prob.injury_proneness_skew,
                    ),
                    mod_stuff: skewed_normal_random(pitch_skill_prob.stuff_skew),
                    mod_fb: skewed_normal_random(pitch_skill_prob.fb_skew),
                    mod_gp: skewed_normal_random(pitch_skill_prob.gp_skew),
                    mod_horizontal_movement: skewed_normal_random(
                        pitch_skill_prob.horizontal_movement_skew,
                    ),
                    mod_vertical_movement: skewed_normal_random(
                        pitch_skill_prob.vertical_movement_skew,
                    ),
                    mod_spin_rate: skewed_normal_random(pitch_skill_prob.spin_rate_skew),
                    mod_usage: skewed_normal_random(pitch_skill_prob.usage_skew),
                };
                pitch_skills.push(pitch_skill);
            }
        }

        pitcher_skill.pitch_skills = pitch_skills;

        Ok(pitcher_skill)
    }
}
