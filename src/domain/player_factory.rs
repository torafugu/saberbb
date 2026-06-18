use super::player_service::PlayerService;
use super::shared::player::{
    DefensiveSkill, PitchSkill, PitcherAttribute, PitcherStyle, Player, Position,
};
use super::shared::prob::{DefensiveSkillProb, PitcherAttributeProb, PlayerProb};
use super::util::{age_random, choose_item_weighted, rl_random, skewed_normal_random};
use crate::domain::shared::prob::ItemProb;
use crate::domain::shared::team::Team;
use crate::error::AppError;
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

    pub fn generate_and_save_players(&mut self, count: u16) -> Result<()> {
        let player_prob = self.service.load_player_probs()?;

        for _ in 0..count {
            let player = self.generate_player(&player_prob)?;
            let team = self.assign_team(&player)?;
            self.service.save_player(team, player)?;
        }
        Ok(())
    }

    pub fn generate_player(&mut self, probs: &PlayerProb) -> Result<Player> {
        let name = self.service.load_random_name()?;
        let age = age_random(
            probs.player_attribute_prob.age_shape,
            probs.player_attribute_prob.age_scale,
            probs.player_attribute_prob.age_offset,
        );
        let throw = rl_random(probs.player_attribute_prob.throw_lefty);
        let bat = rl_random(probs.player_attribute_prob.bat_lefty);
        let mod_ba = skewed_normal_random(probs.batter_skill_prob.ba_skew);
        let mod_slg = skewed_normal_random(probs.batter_skill_prob.slg_skew);

        let mut defensive_skills = Vec::new();
        // TODO: Should be changed to multiple defence skills
        let defensive_skill =
            self.assign_defensive_skills(&probs.position_probs, &probs.defensive_skill_prob)?;

        let mut pitcher_skill = None;
        if defensive_skill.position == Position::P {
            pitcher_skill =
                Some(self.assign_pitcher_skill(
                    &probs.pitcher_style_probs,
                    &probs.pitcher_attribute_prob,
                )?);
        };
        defensive_skills.push(defensive_skill);

        Ok(Player::new_unsaved(
            &name.first,
            &name.last,
            age,
            throw,
            defensive_skills,
            pitcher_skill,
            bat,
            mod_ba,
            mod_slg,
        ))
    }

    fn assign_defensive_skills(
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

    fn assign_pitcher_skill(
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

        let mut pitch_skills: Vec<PitchSkill> = Vec::new();
        for pitch_type_prob in pitch_type_probs {
            let rng: f64 = rand::random();
            if rng < pitch_type_prob.prob {
                let pitch_skill_prob = self.service.pitch_skill_prob(&pitch_type_prob.name)?;
                pitch_skills.push(PitchSkill::from_prob(
                    pitch_type_prob.name.clone(),
                    skewed_normal_random(pitch_skill_prob.velocity_skew),
                    skewed_normal_random(pitch_skill_prob.control_skew),
                    skewed_normal_random(pitch_skill_prob.stamina_skew),
                    skewed_normal_random(pitch_skill_prob.injury_proneness_skew),
                    skewed_normal_random(pitch_skill_prob.stuff_skew),
                    skewed_normal_random(pitch_skill_prob.fb_skew),
                    skewed_normal_random(pitch_skill_prob.gp_skew),
                    skewed_normal_random(pitch_skill_prob.horizontal_movement_skew),
                    skewed_normal_random(pitch_skill_prob.vertical_movement_skew),
                    skewed_normal_random(pitch_skill_prob.spin_rate_skew),
                    skewed_normal_random(pitch_skill_prob.usage_skew),
                ));
            }
        }

        Ok(PitcherAttribute::from_prob(
            pitcher_style,
            skewed_normal_random(pitcher_base_skill_prob.velocity_skew),
            skewed_normal_random(pitcher_base_skill_prob.control_skew),
            skewed_normal_random(pitcher_base_skill_prob.stamina_skew),
            skewed_normal_random(pitcher_base_skill_prob.injury_proneness_skew),
            skewed_normal_random(pitcher_base_skill_prob.clutch_skew),
            skewed_normal_random(pitcher_base_skill_prob.hpp_skew),
            skewed_normal_random(pitcher_base_skill_prob.platoon_splitting_skew),
            pitch_skills,
        ))
    }

    fn assign_team(&self, player: &Player) -> Result<Team, AppError> {
        self.service
            .next_team(player.defensive_skills[0].position.clone())
    }
}
