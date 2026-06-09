use super::shared::player::{PitchType, PitcherStyle, Player, Position};
use super::shared::prob::{PitchSkillProb, PlayerProb};
use crate::domain::shared::player::FullName;
use crate::domain::shared::prob::ItemProb;
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::i18n::I18nManager;
use crate::repositories::player_repository::PlayerRepository;
use anyhow::Result;

pub struct PlayerService<R: PlayerRepository> {
    pub repo: R,
}

impl<R: PlayerRepository> PlayerService<R> {
    pub fn load_player_probs(&self) -> Result<PlayerProb, AppError> {
        Ok(PlayerProb {
            player_attribute_prob: self.repo.player_attribute_prob()?,
            batter_skill_prob: self.repo.batter_skill_prob()?,
            position_probs: self.repo.position_probs()?,
            defensive_skill_prob: self.repo.defensive_skill_prob()?,
            pitcher_style_probs: self.repo.pitcher_style_probs()?,
            pitcher_attribute_prob: self.repo.pitcher_attribute_prob()?,
        })
    }

    pub fn load_random_name(&self) -> Result<FullName, AppError> {
        self.repo.random_name(I18nManager::global().lang_db())
    }

    pub fn next_team(&self, position: Position) -> Result<Team, AppError> {
        let team = match self.repo.next_player_dist_team(position) {
            Ok(team) => team,
            Err(AppError::NotFound(_)) => self.repo.next_random_team()?,
            Err(e) => {
                return Err(AppError::Internal(
                    anyhow::Error::new(e).context("Failed to fetch next team"),
                ));
            }
        };
        Ok(team)
    }

    pub fn pitch_type_probs(
        &self,
        pitcher_style: &PitcherStyle,
    ) -> Result<Vec<ItemProb<PitchType>>, AppError> {
        self.repo.pitch_type_probs(pitcher_style)
    }

    pub fn pitch_skill_prob(&self, pitch_type: &PitchType) -> Result<PitchSkillProb, AppError> {
        self.repo.pitch_skill_prob(pitch_type)
    }

    pub fn save_player(&mut self, team: Team, player: Player) -> Result<(), AppError> {
        self.repo.insert_player(team, player)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::{
        DefensiveSkill, FullName, PitchSkill, PitchType, PitcherAttribute, PitcherStyle, Position,
        RL,
    };
    use crate::domain::shared::prob::{
        BatterSkillProb, DefensiveSkillProb, ItemProb, PitchSkillProb, PitcherAttributeProb,
        PlayerAttributeProb,
    };
    use crate::domain::shared::team::Team;
    use crate::error::AppError;
    use crate::repositories::db::FromRow;
    use anyhow::anyhow;
    use rusqlite::Transaction;
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    struct RecordingRepo {
        state: Rc<RepoState>,
    }

    struct RepoState {
        name: FullName,
        team: Team,
        random_team: Team,
        pitch_type_probs: Vec<ItemProb<PitchType>>,
        player_attribute_prob: PlayerAttributeProb,
        batter_skill_prob: BatterSkillProb,
        defensive_skill_prob: DefensiveSkillProb,
        pitcher_attribute_prob: PitcherAttributeProb,
        random_name_error: Cell<bool>,
        next_team_error: Cell<bool>,
        next_team_not_found: Cell<bool>,
        next_random_team_error: Cell<bool>,
        player_attribute_prob_error: Cell<bool>,
        batter_skill_prob_error: Cell<bool>,
        position_probs: Vec<ItemProb<Position>>,
        position_probs_error: Cell<bool>,
        defensive_skill_prob_error: Cell<bool>,
        pitcher_style_probs: Vec<ItemProb<PitcherStyle>>,
        pitcher_style_probs_error: Cell<bool>,
        pitcher_attribute_prob_error: Cell<bool>,
        pitch_type_probs_error: Cell<bool>,
        pitch_skill_prob_error: Cell<bool>,
        save_error_at: Cell<Option<usize>>,
        random_name_languages: RefCell<Vec<String>>,
        next_team_positions: RefCell<Vec<Position>>,
        pitch_type_prob_styles: RefCell<Vec<PitcherStyle>>,
        pitch_skill_prob_types: RefCell<Vec<PitchType>>,
        next_team_calls: Cell<usize>,
        next_random_team_calls: Cell<usize>,
        player_attribute_prob_calls: Cell<usize>,
        batter_skill_prob_calls: Cell<usize>,
        position_probs_calls: Cell<usize>,
        defensive_skill_prob_calls: Cell<usize>,
        pitcher_style_probs_calls: Cell<usize>,
        pitcher_attribute_prob_calls: Cell<usize>,
        pitch_type_probs_calls: Cell<usize>,
        pitch_skill_prob_calls: Cell<usize>,
        save_calls: Cell<usize>,
        item_probs_calls: Cell<usize>,
        saved: RefCell<Vec<(Team, Player)>>,
    }

    impl RecordingRepo {
        fn new() -> (Self, Rc<RepoState>) {
            let state = Rc::new(RepoState {
                name: FullName {
                    first: "Shohei".into(),
                    last: "Ohtani".into(),
                },
                team: Team::min(1, "Lions"),
                random_team: Team::min(99, "Randoms"),
                pitch_type_probs: vec![ItemProb {
                    name: PitchType::Slider,
                    prob: 1.0,
                }],
                player_attribute_prob: PlayerAttributeProb {
                    age_shape: 2.5,
                    age_scale: 2.5,
                    age_offset: 18.0,
                    throw_lefty: 0.2,
                    bat_lefty: 0.4,
                },
                batter_skill_prob: BatterSkillProb {
                    ba_skew: 0.2,
                    slg_skew: 0.3,
                },
                defensive_skill_prob: DefensiveSkillProb { uzr_skew: 0.4 },
                pitcher_attribute_prob: PitcherAttributeProb {
                    velocity_skew: 0.5,
                    control_skew: 0.6,
                    stamina_skew: 0.7,
                    injury_proneness_skew: 0.8,
                    clutch_skew: 0.9,
                    hpp_skew: 1.0,
                    platoon_splitting_skew: 1.1,
                },
                random_name_error: Cell::new(false),
                next_team_error: Cell::new(false),
                next_team_not_found: Cell::new(false),
                next_random_team_error: Cell::new(false),
                player_attribute_prob_error: Cell::new(false),
                batter_skill_prob_error: Cell::new(false),
                position_probs: vec![ItemProb {
                    name: Position::P,
                    prob: 1.0,
                }],
                position_probs_error: Cell::new(false),
                defensive_skill_prob_error: Cell::new(false),
                pitcher_style_probs: vec![ItemProb {
                    name: PitcherStyle::BalancedPitcher,
                    prob: 1.0,
                }],
                pitcher_style_probs_error: Cell::new(false),
                pitcher_attribute_prob_error: Cell::new(false),
                pitch_type_probs_error: Cell::new(false),
                pitch_skill_prob_error: Cell::new(false),
                save_error_at: Cell::new(None),
                random_name_languages: RefCell::new(Vec::new()),
                next_team_positions: RefCell::new(Vec::new()),
                pitch_type_prob_styles: RefCell::new(Vec::new()),
                pitch_skill_prob_types: RefCell::new(Vec::new()),
                next_team_calls: Cell::new(0),
                next_random_team_calls: Cell::new(0),
                player_attribute_prob_calls: Cell::new(0),
                batter_skill_prob_calls: Cell::new(0),
                position_probs_calls: Cell::new(0),
                defensive_skill_prob_calls: Cell::new(0),
                pitcher_style_probs_calls: Cell::new(0),
                pitcher_attribute_prob_calls: Cell::new(0),
                pitch_type_probs_calls: Cell::new(0),
                pitch_skill_prob_calls: Cell::new(0),
                save_calls: Cell::new(0),
                item_probs_calls: Cell::new(0),
                saved: RefCell::new(Vec::new()),
            });

            (
                Self {
                    state: Rc::clone(&state),
                },
                state,
            )
        }
    }

    impl PlayerRepository for RecordingRepo {
        fn insert_player(&mut self, team: Team, player: Player) -> Result<(), AppError> {
            let call_index = self.state.save_calls.get();
            self.state.save_calls.set(call_index + 1);

            if self.state.save_error_at.get() == Some(call_index) {
                return Err(AppError::Internal(anyhow!("save failed")));
            }

            self.state.saved.borrow_mut().push((team, player));
            Ok(())
        }

        fn insert_defensive_skill(
            &self,
            _tx: &Transaction,
            _player_id: u32,
            _defensive_skill: &DefensiveSkill,
        ) -> Result<usize, AppError> {
            Ok(1)
        }

        fn insert_pitcher_attribute(
            &self,
            _tx: &Transaction,
            _player_id: u32,
            _pitcher_attribute: &PitcherAttribute,
        ) -> Result<usize, AppError> {
            Ok(1)
        }

        fn insert_pitch_skill(
            &self,
            _tx: &Transaction,
            _player_id: u32,
            _pitch_skill: &PitchSkill,
        ) -> Result<usize, AppError> {
            Ok(1)
        }

        fn random_name(&self, language: String) -> Result<FullName, AppError> {
            if self.state.random_name_error.get() {
                return Err(AppError::Internal(anyhow!("random name failed")));
            }

            self.state.random_name_languages.borrow_mut().push(language);
            Ok(self.state.name.clone())
        }

        fn next_player_dist_team(&self, position: Position) -> Result<Team, AppError> {
            self.state.next_team_positions.borrow_mut().push(position);
            self.state
                .next_team_calls
                .set(self.state.next_team_calls.get() + 1);

            if self.state.next_team_not_found.get() {
                return Err(AppError::NotFound("position not found".to_string()));
            }

            if self.state.next_team_error.get() {
                return Err(AppError::Internal(anyhow!("next team failed")));
            }

            Ok(self.state.team.clone())
        }

        fn next_random_team(&self) -> Result<Team, AppError> {
            self.state
                .next_random_team_calls
                .set(self.state.next_random_team_calls.get() + 1);

            if self.state.next_random_team_error.get() {
                return Err(AppError::Internal(anyhow!("next random team failed")));
            }

            Ok(self.state.random_team.clone())
        }

        fn position_probs(&self) -> Result<Vec<ItemProb<Position>>, AppError> {
            self.state
                .position_probs_calls
                .set(self.state.position_probs_calls.get() + 1);

            if self.state.position_probs_error.get() {
                return Err(AppError::Internal(anyhow!("position probs failed")));
            }

            Ok(self.state.position_probs.clone())
        }

        fn pitcher_style_probs(&self) -> Result<Vec<ItemProb<PitcherStyle>>, AppError> {
            self.state
                .pitcher_style_probs_calls
                .set(self.state.pitcher_style_probs_calls.get() + 1);

            if self.state.pitcher_style_probs_error.get() {
                return Err(AppError::Internal(anyhow!("pitcher style probs failed")));
            }

            Ok(self.state.pitcher_style_probs.clone())
        }

        fn pitch_type_probs(
            &self,
            pitch_style: &PitcherStyle,
        ) -> Result<Vec<ItemProb<PitchType>>, AppError> {
            self.state
                .pitch_type_probs_calls
                .set(self.state.pitch_type_probs_calls.get() + 1);
            self.state
                .pitch_type_prob_styles
                .borrow_mut()
                .push(pitch_style.clone());

            if self.state.pitch_type_probs_error.get() {
                return Err(AppError::Internal(anyhow!("pitch type probs failed")));
            }

            Ok(self.state.pitch_type_probs.clone())
        }

        fn pitch_skill_prob(&self, pitch_type: &PitchType) -> Result<PitchSkillProb, AppError> {
            self.state
                .pitch_skill_prob_calls
                .set(self.state.pitch_skill_prob_calls.get() + 1);
            self.state
                .pitch_skill_prob_types
                .borrow_mut()
                .push(pitch_type.clone());

            if self.state.pitch_skill_prob_error.get() {
                return Err(AppError::Internal(anyhow!("pitch skill prob failed")));
            }

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

        fn player_attribute_prob(&self) -> Result<PlayerAttributeProb, AppError> {
            self.state
                .player_attribute_prob_calls
                .set(self.state.player_attribute_prob_calls.get() + 1);

            if self.state.player_attribute_prob_error.get() {
                return Err(AppError::Internal(anyhow!("player attribute prob failed")));
            }

            Ok(self.state.player_attribute_prob.clone())
        }

        fn item_probs<T>(&self, category: &str) -> Result<Vec<ItemProb<T>>, AppError>
        where
            ItemProb<T>: FromRow<Error = AppError>,
        {
            self.state
                .item_probs_calls
                .set(self.state.item_probs_calls.get() + 1);

            panic!("item_probs should not be called directly in PlayerService tests: {category}");
        }

        fn batter_skill_prob(&self) -> Result<BatterSkillProb, AppError> {
            self.state
                .batter_skill_prob_calls
                .set(self.state.batter_skill_prob_calls.get() + 1);

            if self.state.batter_skill_prob_error.get() {
                return Err(AppError::Internal(anyhow!("batter skill prob failed")));
            }

            Ok(self.state.batter_skill_prob.clone())
        }

        fn defensive_skill_prob(&self) -> Result<DefensiveSkillProb, AppError> {
            self.state
                .defensive_skill_prob_calls
                .set(self.state.defensive_skill_prob_calls.get() + 1);

            if self.state.defensive_skill_prob_error.get() {
                return Err(AppError::Internal(anyhow!("defensive skill prob failed")));
            }

            Ok(self.state.defensive_skill_prob.clone())
        }

        fn pitcher_attribute_prob(&self) -> Result<PitcherAttributeProb, AppError> {
            self.state
                .pitcher_attribute_prob_calls
                .set(self.state.pitcher_attribute_prob_calls.get() + 1);

            if self.state.pitcher_attribute_prob_error.get() {
                return Err(AppError::Internal(anyhow!("pitcher attribute prob failed")));
            }

            Ok(self.state.pitcher_attribute_prob.clone())
        }
    }

    fn service_with_repo() -> (PlayerService<RecordingRepo>, Rc<RepoState>) {
        let (repo, state) = RecordingRepo::new();
        (PlayerService { repo }, state)
    }

    #[test]
    fn load_player_probs_loads_all_probability_groups() {
        let (service, state) = service_with_repo();

        let player_prob = service.load_player_probs().unwrap();

        assert_eq!(player_prob.player_attribute_prob.age_offset, 18.0);
        assert_eq!(player_prob.batter_skill_prob.slg_skew, 0.3);
        assert_eq!(player_prob.position_probs[0].name, Position::P);
        assert_eq!(player_prob.defensive_skill_prob.uzr_skew, 0.4);
        assert!(matches!(
            player_prob.pitcher_style_probs[0].name,
            PitcherStyle::BalancedPitcher
        ));
        assert_eq!(player_prob.pitcher_attribute_prob.velocity_skew, 0.5);
        assert_eq!(state.player_attribute_prob_calls.get(), 1);
        assert_eq!(state.batter_skill_prob_calls.get(), 1);
        assert_eq!(state.position_probs_calls.get(), 1);
        assert_eq!(state.defensive_skill_prob_calls.get(), 1);
        assert_eq!(state.pitcher_style_probs_calls.get(), 1);
        assert_eq!(state.pitcher_attribute_prob_calls.get(), 1);
    }

    #[test]
    fn load_player_probs_returns_error_from_repository() {
        let (repo, state) = RecordingRepo::new();
        state.position_probs_error.set(true);
        let service = PlayerService { repo };

        assert!(service.load_player_probs().is_err());
        assert_eq!(state.position_probs_calls.get(), 1);
        assert_eq!(state.defensive_skill_prob_calls.get(), 0);
    }

    #[test]
    fn load_random_name_passes_current_i18n_language() {
        let (service, state) = service_with_repo();

        let name = service.load_random_name().unwrap();

        assert_eq!(name.first.as_ref(), "Shohei");
        assert_eq!(name.last.as_ref(), "Ohtani");
        assert_eq!(
            *state.random_name_languages.borrow(),
            vec!["us".to_string()]
        );
    }

    #[test]
    fn next_team_returns_position_distribution_team() {
        let (service, state) = service_with_repo();

        let team = service.next_team(Position::CF).unwrap();

        assert_eq!(team.id, 1);
        assert_eq!(team.name.as_ref(), "Lions");
        assert_eq!(*state.next_team_positions.borrow(), vec![Position::CF]);
        assert_eq!(state.next_team_calls.get(), 1);
        assert_eq!(state.next_random_team_calls.get(), 0);
    }

    #[test]
    fn next_team_falls_back_to_random_team_when_distribution_team_not_found() {
        let (repo, state) = RecordingRepo::new();
        state.next_team_not_found.set(true);
        let service = PlayerService { repo };

        let team = service.next_team(Position::P).unwrap();

        assert_eq!(team.id, 99);
        assert_eq!(team.name.as_ref(), "Randoms");
        assert_eq!(state.next_team_calls.get(), 1);
        assert_eq!(state.next_random_team_calls.get(), 1);
    }

    #[test]
    fn next_team_returns_internal_error_when_distribution_lookup_fails() {
        let (repo, state) = RecordingRepo::new();
        state.next_team_error.set(true);
        let service = PlayerService { repo };

        let result = service.next_team(Position::P);

        assert!(matches!(result, Err(AppError::Internal(_))));
        assert_eq!(state.next_team_calls.get(), 1);
        assert_eq!(state.next_random_team_calls.get(), 0);
    }

    #[test]
    fn next_team_returns_error_when_random_fallback_fails() {
        let (repo, state) = RecordingRepo::new();
        state.next_team_not_found.set(true);
        state.next_random_team_error.set(true);
        let service = PlayerService { repo };

        let result = service.next_team(Position::P);

        assert!(matches!(result, Err(AppError::Internal(_))));
        assert_eq!(state.next_team_calls.get(), 1);
        assert_eq!(state.next_random_team_calls.get(), 1);
    }

    #[test]
    fn pitch_type_probs_delegates_to_repository() {
        let (service, state) = service_with_repo();

        let probs = service
            .pitch_type_probs(&PitcherStyle::PowerPitcher)
            .unwrap();

        assert!(matches!(probs[0].name, PitchType::Slider));
        assert_eq!(state.pitch_type_probs_calls.get(), 1);
        assert!(matches!(
            state.pitch_type_prob_styles.borrow()[0],
            PitcherStyle::PowerPitcher
        ));
    }

    #[test]
    fn pitch_skill_prob_delegates_to_repository() {
        let (service, state) = service_with_repo();

        let prob = service.pitch_skill_prob(&PitchType::Changeup).unwrap();

        assert!(matches!(prob.pitch_type, PitchType::Changeup));
        assert_eq!(state.pitch_skill_prob_calls.get(), 1);
        assert!(matches!(
            state.pitch_skill_prob_types.borrow()[0],
            PitchType::Changeup
        ));
    }

    #[test]
    fn save_player_delegates_to_repository() {
        let (mut service, state) = service_with_repo();
        let mut player = Player::new(7, "First", "Last", 25, RL::Right, RL::Right, 0.0, 0.0);
        player.throw = RL::Left;
        let team = Team::min(3, "Tigers");

        service.save_player(team, player).unwrap();

        let saved = state.saved.borrow();
        assert_eq!(state.save_calls.get(), 1);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].0.id, 3);
        assert_eq!(saved[0].1.id, 7);
        assert_eq!(saved[0].1.first_name.as_ref(), "First");
    }
}
