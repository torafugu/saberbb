use super::shared::player::{DefensiveSkill, PitchSkill, PitcherAttribute, PitcherStyle, Player};
use super::shared::probabilities::{DefensiveSkillProb, PitcherAttributeProb};
use super::utils::{age_random, choose_item_weighted, rl_random, skewed_normal_random};
use crate::domain::error::AppError;
use crate::domain::shared::types::Position;
use crate::i18n::I18nManager;
use crate::repositories::player_repository::PlayerRepository;
use crate::t;
use anyhow::{Result, bail};

pub struct PlayerService<R: PlayerRepository> {
    pub repo: R,
}

impl<R: PlayerRepository> PlayerService<R> {
    // TODO: Divide batter generation and pitcher generation
    pub fn generate_players(&mut self, num_of_players: u16) -> Result<()> {
        let player_attribute_prob = self.repo.player_attribute_prob()?;
        let batter_skill_prob = self.repo.batter_skill_prob()?;
        let defensive_skill_prob = self.repo.defensive_skill_prob()?;
        let pitcher_attribute_prob = self.repo.pitcher_attribute_prob()?;

        for _ in 0..num_of_players {
            let name = self.repo.random_name(I18nManager::global().lang_db())?;
            let age = age_random(
                player_attribute_prob.age_shape,
                player_attribute_prob.age_scale,
                player_attribute_prob.age_offset,
            );
            let throw = rl_random(player_attribute_prob.throw_lefty);
            let bat = rl_random(player_attribute_prob.bat_lefty);
            let mod_ba = skewed_normal_random(batter_skill_prob.ba_skew);
            let mod_slg = skewed_normal_random(batter_skill_prob.slg_skew);

            let mut defensive_skills = Vec::new();
            // TODO: Should be changed to multiple skills
            let defensive_skill = self.assign_defensive_skills(&defensive_skill_prob)?;

            let mut pitcher_skill = None;
            if defensive_skill.position == Position::P {
                pitcher_skill = Some(self.assign_pitcher_skill(&pitcher_attribute_prob)?);
            };
            defensive_skills.push(defensive_skill);

            // Assign player to team
            let team = match self
                .repo
                .next_player_dist_team(defensive_skills[0].position.clone())
            {
                Ok(team) => team,
                Err(e)
                    if e.downcast_ref::<AppError>()
                        .map_or(false, |app| matches!(app, AppError::NotFound(_))) =>
                {
                    self.repo.next_random_team()?
                }
                Err(e) => {
                    let error_msg = t!("error", "function" => "next_player_dist_team");
                    return Err(anyhow::anyhow!("{} {}", error_msg, e));
                }
            };

            let player = Player {
                id: 0,
                first_name: name[0].clone().into(),
                last_name: name[1].clone().into(),
                age: age,
                throw: throw,
                // TODO: consider multiple skill holder
                defensive_skills: defensive_skills,
                pitcher_attribute: pitcher_skill,
                bat: bat,
                mod_ba: mod_ba,
                mod_slg: mod_slg,
            };

            if let Err(e) = self.repo.save_player(team, player) {
                let error_msg = t!("error", "function" => "save_player");
                bail!("{}, {}", error_msg, e);
            }
        }
        Ok(())
    }

    pub fn assign_defensive_skills(
        &self,
        defensive_skill_prob: &DefensiveSkillProb,
    ) -> Result<DefensiveSkill> {
        let position_probs = self.repo.position_probs()?;

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
        pitcher_base_skill_prob: &PitcherAttributeProb,
    ) -> Result<PitcherAttribute> {
        let pitcher_style_probs = self.repo.pitcher_style_probs()?;

        let pitcher_style = match choose_item_weighted(&pitcher_style_probs) {
            Some(chosen) => chosen.clone(),
            None => {
                bail!(t!("error", "function" => "choose_item_weighted"));
            }
        };

        let pitch_type_probs = self.repo.pitch_type_probs(&pitcher_style);

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
        for pitch_type_prob in pitch_type_probs? {
            let rng: f64 = rand::random();
            if rng < pitch_type_prob.prob {
                let pitch_skill_prob = self.repo.pitch_skill_prob(&pitch_type_prob.name)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::PitchType;
    use crate::domain::shared::probabilities::{
        BatterSkillProb, PitchSkillProb, PlayerAttributeProb,
    };
    use crate::domain::shared::team::Team;
    use crate::domain::shared::types::Position;
    use crate::domain::utils::ItemProb;
    use anyhow::anyhow;
    use std::cell::{Cell, RefCell};

    struct RecordingRepo {
        name: [String; 2],
        team: Team,
        random_team: Team,
        random_name_error: bool,
        next_team_error: bool,
        next_team_not_found: bool,
        next_random_team_error: bool,
        position_probs: Vec<ItemProb<Position>>,
        position_probs_error: bool,
        pitcher_style_probs: Vec<ItemProb<PitcherStyle>>,
        pitcher_style_probs_error: bool,
        item_prob_categories: RefCell<Vec<String>>,
        save_error_at: Option<usize>,
        random_name_languages: RefCell<Vec<String>>,
        next_team_positions: RefCell<Vec<Position>>,
        next_team_calls: Cell<usize>,
        next_random_team_calls: Cell<usize>,
        position_probs_calls: Cell<usize>,
        pitcher_style_probs_calls: Cell<usize>,
        save_calls: usize,
        saved: Vec<(Team, Player)>,
    }

    impl RecordingRepo {
        fn new() -> Self {
            Self {
                name: ["翔平".to_string(), "大谷".to_string()],
                team: Team::min(1, "ライオンズ"),
                random_team: Team::min(99, "ランダムズ"),
                random_name_error: false,
                next_team_error: false,
                next_team_not_found: false,
                next_random_team_error: false,
                position_probs: vec![ItemProb {
                    name: Position::P,
                    prob: 1.0,
                }],
                position_probs_error: false,
                pitcher_style_probs: vec![ItemProb {
                    name: PitcherStyle::BalancedPitcher,
                    prob: 1.0,
                }],
                pitcher_style_probs_error: false,
                item_prob_categories: RefCell::new(Vec::new()),
                save_error_at: None,
                random_name_languages: RefCell::new(Vec::new()),
                next_team_positions: RefCell::new(Vec::new()),
                next_team_calls: Cell::new(0),
                next_random_team_calls: Cell::new(0),
                position_probs_calls: Cell::new(0),
                pitcher_style_probs_calls: Cell::new(0),
                save_calls: 0,
                saved: Vec::new(),
            }
        }
    }

    impl PlayerRepository for RecordingRepo {
        fn save_player(&mut self, team: Team, player: Player) -> Result<()> {
            let call_index = self.save_calls;
            self.save_calls += 1;

            if self.save_error_at == Some(call_index) {
                return Err(anyhow!("save failed"));
            }

            self.saved.push((team, player));
            Ok(())
        }

        fn random_name(&self, _language: String) -> Result<[String; 2]> {
            if self.random_name_error {
                return Err(anyhow!("random name failed"));
            }

            self.random_name_languages.borrow_mut().push(_language);
            Ok(self.name.clone())
        }

        fn next_player_dist_team(&self, position: Position) -> Result<Team> {
            self.next_team_positions.borrow_mut().push(position);
            self.next_team_calls.set(self.next_team_calls.get() + 1);

            if self.next_team_not_found {
                return Err(AppError::NotFound("position not found".to_string()).into());
            }

            if self.next_team_error {
                return Err(anyhow!("next team failed"));
            }

            Ok(self.team.clone())
        }

        fn next_random_team(&self) -> Result<Team> {
            self.next_random_team_calls
                .set(self.next_random_team_calls.get() + 1);

            if self.next_random_team_error {
                return Err(anyhow!("next random team failed"));
            }

            Ok(self.random_team.clone())
        }

        fn position_probs(&self) -> Result<Vec<ItemProb<Position>>> {
            self.position_probs_calls
                .set(self.position_probs_calls.get() + 1);
            self.item_prob_categories
                .borrow_mut()
                .push("position".to_string());

            if self.position_probs_error {
                return Err(anyhow!("position probs failed"));
            }

            Ok(self.position_probs.clone())
        }

        fn pitcher_style_probs(&self) -> Result<Vec<ItemProb<PitcherStyle>>> {
            self.pitcher_style_probs_calls
                .set(self.pitcher_style_probs_calls.get() + 1);
            self.item_prob_categories
                .borrow_mut()
                .push("pitcher_style".to_string());

            if self.pitcher_style_probs_error {
                return Err(anyhow!("pitcher style probs failed"));
            }

            Ok(self.pitcher_style_probs.clone())
        }

        fn pitch_type_probs(
            &self,
            _pitch_style: &PitcherStyle,
        ) -> Result<Vec<ItemProb<PitchType>>> {
            Ok(Vec::new())
        }

        fn pitch_skill_prob(&self, pitch_type: &PitchType) -> Result<PitchSkillProb> {
            Ok(PitchSkillProb {
                pitch_type: pitch_type.clone(),
                velocity_skew: 0.0,
                control_skew: 0.0,
                stamina_skew: 0.0,
                injury_proneness_skew: 0.0,
                stuff_skew: 0.0,
                fb_skew: 0.0,
                gp_skew: 0.0,
                horizontal_movement_skew: 0.0,
                vertical_movement_skew: 0.0,
                spin_rate_skew: 0.0,
                usage_skew: 0.0,
            })
        }

        fn player_attribute_prob(&self) -> Result<PlayerAttributeProb> {
            Ok(PlayerAttributeProb {
                age_shape: 2.5,
                age_scale: 2.5,
                age_offset: 18.0,
                throw_lefty: 0.2,
                bat_lefty: 0.4,
            })
        }

        fn batter_skill_prob(&self) -> Result<BatterSkillProb> {
            Ok(BatterSkillProb {
                ba_skew: 0.2,
                slg_skew: 0.2,
            })
        }

        fn defensive_skill_prob(&self) -> Result<DefensiveSkillProb> {
            Ok(DefensiveSkillProb { uzr_skew: 0.2 })
        }

        fn pitcher_attribute_prob(&self) -> Result<PitcherAttributeProb> {
            Ok(PitcherAttributeProb {
                velocity_skew: 0.2,
                control_skew: 0.2,
                stamina_skew: 0.2,
                injury_proneness_skew: 0.0,
                clutch_skew: 0.0,
                hpp_skew: 0.0,
                platoon_splitting_skew: 0.0,
            })
        }

        fn query_item_probs<T, F, C>(&self, _category: C, _mapper: F) -> Result<Vec<ItemProb<T>>>
        where
            C: AsRef<str>,
            F: for<'row> FnMut(&'row rusqlite::Row) -> rusqlite::Result<ItemProb<T>>,
        {
            unreachable!("RecordingRepo returns concrete probability fixtures directly")
        }
    }

    fn assert_pitcher_skill_is_finite(pitcher_skill: &PitcherAttribute) {
        assert!(matches!(
            pitcher_skill.pitcher_style,
            PitcherStyle::BalancedPitcher
        ));
        assert!(pitcher_skill.mod_velocity.is_finite());
        assert!(pitcher_skill.mod_control.is_finite());
        assert!(pitcher_skill.mod_stamina.is_finite());
        assert!(pitcher_skill.mod_injury_proneness.is_finite());
        assert!(pitcher_skill.mod_clutch.is_finite());
        assert!(pitcher_skill.mod_hpp.is_finite());
        assert!(pitcher_skill.mod_platoon_splitting.is_finite());
        assert!(pitcher_skill.pitch_skills.is_empty());
    }

    #[test]
    fn generate_players_saves_requested_number_of_players() {
        let mut service = PlayerService {
            repo: RecordingRepo::new(),
        };

        let result = service.generate_players(3);

        assert!(result.is_ok());
        assert_eq!(service.repo.random_name_languages.borrow().len(), 3);
        assert_eq!(service.repo.position_probs_calls.get(), 3);
        assert_eq!(
            *service.repo.item_prob_categories.borrow(),
            vec![
                "position".to_string(),
                "pitcher_style".to_string(),
                "position".to_string(),
                "pitcher_style".to_string(),
                "position".to_string(),
                "pitcher_style".to_string()
            ]
        );
        assert_eq!(service.repo.pitcher_style_probs_calls.get(), 3);
        assert_eq!(service.repo.next_team_calls.get(), 3);
        assert_eq!(service.repo.next_random_team_calls.get(), 0);
        assert_eq!(service.repo.save_calls, 3);
        assert_eq!(service.repo.saved.len(), 3);
    }

    #[test]
    fn generate_players_saves_players_with_names_from_repository() {
        let mut service = PlayerService {
            repo: RecordingRepo::new(),
        };

        service.generate_players(1).unwrap();

        let (_, player) = &service.repo.saved[0];
        assert_eq!(player.first_name.as_ref(), "翔平");
        assert_eq!(player.last_name.as_ref(), "大谷");
    }

    #[test]
    fn generate_players_assigns_team_from_repository() {
        let mut repo = RecordingRepo::new();
        repo.team = Team::min(7, "ライオンズ");
        let mut service = PlayerService { repo };

        service.generate_players(1).unwrap();

        let (team, _) = &service.repo.saved[0];
        assert_eq!(team.id, 7);
        assert_eq!(team.name.as_ref(), "ライオンズ");
    }

    #[test]
    fn generate_players_sets_generated_player_defaults() {
        let mut service = PlayerService {
            repo: RecordingRepo::new(),
        };

        service.generate_players(1).unwrap();

        let (_, player) = &service.repo.saved[0];
        assert_eq!(player.id, 0);
        assert_eq!(player.defensive_skills.len(), 1);
        assert!(player.age >= 18);
        assert!(player.mod_ba.is_finite());
        assert!(player.mod_slg.is_finite());
        assert!(player.defensive_skills[0].mod_uzr.is_finite());
    }

    #[test]
    fn generate_players_with_zero_players_does_nothing() {
        let mut service = PlayerService {
            repo: RecordingRepo::new(),
        };

        let result = service.generate_players(0);

        assert!(result.is_ok());
        assert!(service.repo.random_name_languages.borrow().is_empty());
        assert_eq!(service.repo.position_probs_calls.get(), 0);
        assert_eq!(service.repo.next_team_calls.get(), 0);
        assert_eq!(service.repo.next_random_team_calls.get(), 0);
        assert_eq!(service.repo.save_calls, 0);
        assert!(service.repo.saved.is_empty());
    }

    #[test]
    fn generate_players_returns_error_when_random_name_fails() {
        let mut repo = RecordingRepo::new();
        repo.random_name_error = true;
        let mut service = PlayerService { repo };

        let result = service.generate_players(3);

        assert!(result.is_err());
        assert_eq!(service.repo.position_probs_calls.get(), 0);
        assert_eq!(service.repo.next_team_calls.get(), 0);
        assert_eq!(service.repo.next_random_team_calls.get(), 0);
        assert_eq!(service.repo.save_calls, 0);
        assert!(service.repo.saved.is_empty());
    }

    #[test]
    fn generate_players_returns_error_when_position_probs_fails() {
        let mut repo = RecordingRepo::new();
        repo.position_probs_error = true;
        let mut service = PlayerService { repo };

        let result = service.generate_players(3);

        assert!(result.is_err());
        assert_eq!(service.repo.random_name_languages.borrow().len(), 1);
        assert_eq!(service.repo.position_probs_calls.get(), 1);
        assert_eq!(service.repo.next_team_calls.get(), 0);
        assert_eq!(service.repo.next_random_team_calls.get(), 0);
        assert_eq!(service.repo.save_calls, 0);
        assert!(service.repo.saved.is_empty());
    }

    #[test]
    fn generate_players_returns_error_when_next_player_dist_team_fails() {
        let mut repo = RecordingRepo::new();
        repo.next_team_error = true;
        let mut service = PlayerService { repo };

        let result = service.generate_players(3);

        assert!(result.is_err());
        assert_eq!(service.repo.random_name_languages.borrow().len(), 1);
        assert_eq!(service.repo.position_probs_calls.get(), 1);
        assert_eq!(service.repo.next_team_calls.get(), 1);
        assert_eq!(service.repo.next_random_team_calls.get(), 0);
        assert_eq!(service.repo.save_calls, 0);
        assert!(service.repo.saved.is_empty());
    }

    #[test]
    fn generate_players_falls_back_to_random_team_when_position_team_not_found() {
        let mut repo = RecordingRepo::new();
        repo.next_team_not_found = true;
        repo.random_team = Team::min(42, "フォールバックズ");
        let mut service = PlayerService { repo };

        let result = service.generate_players(1);

        assert!(result.is_ok());
        assert_eq!(service.repo.position_probs_calls.get(), 1);
        assert_eq!(service.repo.next_team_calls.get(), 1);
        assert_eq!(service.repo.next_random_team_calls.get(), 1);
        assert_eq!(service.repo.save_calls, 1);
        assert_eq!(service.repo.saved[0].0.id, 42);
        assert_eq!(service.repo.saved[0].0.name.as_ref(), "フォールバックズ");
    }

    #[test]
    fn generate_players_returns_error_when_random_team_fallback_fails() {
        let mut repo = RecordingRepo::new();
        repo.next_team_not_found = true;
        repo.next_random_team_error = true;
        let mut service = PlayerService { repo };

        let result = service.generate_players(1);

        assert!(result.is_err());
        assert_eq!(service.repo.position_probs_calls.get(), 1);
        assert_eq!(service.repo.next_team_calls.get(), 1);
        assert_eq!(service.repo.next_random_team_calls.get(), 1);
        assert_eq!(service.repo.save_calls, 0);
        assert!(service.repo.saved.is_empty());
    }

    #[test]
    fn generate_players_returns_error_when_save_player_fails() {
        let mut repo = RecordingRepo::new();
        repo.save_error_at = Some(1);
        let mut service = PlayerService { repo };

        let result = service.generate_players(3);

        assert!(result.is_err());
        assert_eq!(service.repo.random_name_languages.borrow().len(), 2);
        assert_eq!(service.repo.position_probs_calls.get(), 2);
        assert_eq!(service.repo.next_team_calls.get(), 2);
        assert_eq!(service.repo.next_random_team_calls.get(), 0);
        assert_eq!(service.repo.save_calls, 2);
        assert_eq!(service.repo.saved.len(), 1);
    }

    #[test]
    fn generate_players_uses_current_i18n_language_for_random_name() {
        let mut service = PlayerService {
            repo: RecordingRepo::new(),
        };

        service.generate_players(1).unwrap();

        assert_eq!(
            *service.repo.random_name_languages.borrow(),
            vec!["us".to_string()]
        );
    }

    #[test]
    fn generate_players_passes_assigned_position_to_next_team_lookup() {
        let mut service = PlayerService {
            repo: RecordingRepo::new(),
        };

        service.generate_players(1).unwrap();

        let saved_position = service.repo.saved[0].1.defensive_skills[0].position.clone();
        assert_eq!(
            *service.repo.next_team_positions.borrow(),
            vec![saved_position]
        );
    }

    #[test]
    fn generate_players_assigns_pitcher_skill_for_pitchers() {
        let mut repo = RecordingRepo::new();
        repo.position_probs = vec![ItemProb {
            name: Position::P,
            prob: 1.0,
        }];
        let mut service = PlayerService { repo };

        service.generate_players(1).unwrap();

        let (_, player) = &service.repo.saved[0];
        assert_eq!(player.defensive_skills[0].position, Position::P);
        let pitcher_skill = player.pitcher_attribute.as_ref().unwrap();
        assert_pitcher_skill_is_finite(pitcher_skill);
    }

    #[test]
    fn generate_players_does_not_assign_pitcher_skill_for_non_pitchers() {
        let mut repo = RecordingRepo::new();
        repo.position_probs = vec![ItemProb {
            name: Position::CF,
            prob: 1.0,
        }];
        let mut service = PlayerService { repo };

        service.generate_players(1).unwrap();

        let (_, player) = &service.repo.saved[0];
        assert_eq!(player.defensive_skills[0].position, Position::CF);
        assert!(player.pitcher_attribute.is_none());
    }

    #[test]
    fn assign_defensive_skills_uses_position_probs_with_finite_uzr() {
        let mut repo = RecordingRepo::new();
        repo.position_probs = vec![ItemProb {
            name: Position::CF,
            prob: 1.0,
        }];
        let service = PlayerService { repo };
        let defensive_skill_prob = DefensiveSkillProb { uzr_skew: 0.2 };

        let defensive_skill = service
            .assign_defensive_skills(&defensive_skill_prob)
            .unwrap();

        assert_eq!(defensive_skill.position, Position::CF);
        assert!(defensive_skill.mod_uzr.is_finite());
        assert_eq!(service.repo.position_probs_calls.get(), 1);
        assert_eq!(
            *service.repo.item_prob_categories.borrow(),
            vec!["position".to_string()]
        );
    }

    #[test]
    fn assign_defensive_skills_returns_error_when_position_probs_are_empty() {
        let mut repo = RecordingRepo::new();
        repo.position_probs = Vec::new();
        let service = PlayerService { repo };
        let defensive_skill_prob = DefensiveSkillProb { uzr_skew: 0.2 };

        let result = service.assign_defensive_skills(&defensive_skill_prob);

        assert!(result.is_err());
        assert_eq!(service.repo.position_probs_calls.get(), 1);
    }

    #[test]
    fn assign_pitcher_skill_returns_finite_base_skill() {
        let service = PlayerService {
            repo: RecordingRepo::new(),
        };
        let pitcher_base_skill_prob = PitcherAttributeProb {
            velocity_skew: 0.11,
            control_skew: 0.12,
            stamina_skew: 0.13,
            injury_proneness_skew: 0.14,
            clutch_skew: 0.15,
            hpp_skew: 0.16,
            platoon_splitting_skew: 0.17,
        };

        let pitcher_skill = service
            .assign_pitcher_skill(&pitcher_base_skill_prob)
            .unwrap();

        assert_pitcher_skill_is_finite(&pitcher_skill);
    }
}
