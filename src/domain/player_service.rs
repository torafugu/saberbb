use super::shared::player::{
    FielderType, HitterTendency, PitchType, PitcherStyle, Player, Position,
};
use super::shared::prob::{
    BatterInfoProbs, FielderInfoProbs, PitchSkillProbs, PitcherInfoProbs, PlayerInfoProbs,
    RunningSkillProbs,
};
use crate::domain::resolver::batting_resolver::FieldSector;
use crate::domain::shared::player::FullName;
use crate::domain::shared::prob::ItemWeighted;
use crate::domain::shared::team::Team;
use crate::error::AppError;
use crate::i18n::I18nManager;
use crate::repositories::player_repository::PlayerRepository;
use anyhow::Result;
use std::collections::HashMap;
use strum::IntoEnumIterator;
use tracing::info;

const PLY: &str = "player";
const PIF: &str = "player_info";
const RS: &str = "running_skills";
const FI: &str = "fielder_info";
const PTI: &str = "pitcher_info";
const PT: &str = "pitch_type";

pub struct PlayerService<R: PlayerRepository> {
    pub repo: R,
}

impl<R: PlayerRepository> PlayerService<R> {
    pub fn load_player_info_probs(&self) -> Result<PlayerInfoProbs, AppError> {
        info!("load_player_info_probs() started");

        Ok(PlayerInfoProbs {
            age: self.repo.gamma_params(PLY, PIF, "age")?,
        })
    }

    pub fn load_running_skill_probs(&self) -> Result<RunningSkillProbs, AppError> {
        info!("load_running_skill_probs() started");

        let speed = self.repo.normal_params(PLY, RS, "running_speed")?;
        let lead_distance = self.repo.normal_params(PLY, RS, "lead_distance")?;
        let start_reaction = self.repo.normal_params(PLY, RS, "start_reaction")?;

        Ok(RunningSkillProbs {
            speed: speed,
            lead_distance: lead_distance,
            start_reaction: start_reaction,
        })
    }

    pub fn load_multiple_fielder_type_prob(
        &self,
        fielder_type: &FielderType,
    ) -> Result<Vec<ItemWeighted<FielderType>>, AppError> {
        info!("load_multiple_fielder_type_prob() started");

        let multiple_fielder_type = self
            .repo
            .item_probs("multiple_fielder_type", fielder_type.as_ref())?;

        Ok(multiple_fielder_type)
    }

    pub fn load_fielder_info_probs(&self) -> Result<FielderInfoProbs, AppError> {
        info!("load_fielder_info_probs() started");

        let fielder_type = self.repo.item_probs(PLY, "fielder_type")?;
        let throw_speed = self.repo.normal_params(PLY, FI, "throw_speed")?;
        let running_speed = self.repo.normal_params(PLY, FI, "running_speed")?;
        let reaction = self.repo.normal_params(PLY, FI, "reaction")?;
        let prep_time = self.repo.normal_params(PLY, FI, "prep_time")?;

        Ok(FielderInfoProbs {
            fielder_type: fielder_type,
            throw_speed: throw_speed,
            running_speed: running_speed,
            reaction: reaction,
            prep_time: prep_time,
        })
    }

    pub fn load_batter_info_probs(&self) -> Result<BatterInfoProbs, AppError> {
        info!("load_batter_info_probs() started");

        let batting_side = self.repo.item_probs(PLY, "batting_side")?;
        let swing_speed = self.repo.normal_params(PLY, "batter_info", "swing_speed")?;
        let hitter_tendency = self.repo.item_probs(PLY, "hitter_tendency")?;

        Ok(BatterInfoProbs {
            batting_side: batting_side,
            swing_speed: swing_speed,
            hitter_tendency: hitter_tendency,
        })
    }

    pub fn load_field_sector_weights_prob(
        &self,
    ) -> Result<HashMap<HitterTendency, Vec<ItemWeighted<FieldSector>>>, AppError> {
        info!("load_field_sector_weights_prob() started");

        let mut field_sector_weights_map: HashMap<HitterTendency, Vec<ItemWeighted<FieldSector>>> =
            HashMap::new();

        for hitter_tendency in HitterTendency::iter() {
            field_sector_weights_map.entry(hitter_tendency).or_insert(
                self.repo
                    .item_probs("hitter_tendency", hitter_tendency.as_ref())?,
            );
        }

        Ok(field_sector_weights_map)
    }

    pub fn load_pitcher_info_prob(&self) -> Result<PitcherInfoProbs, AppError> {
        info!("load_pitcher_info_prob() started");

        let pitcher_style = self.repo.item_probs(PTI, "pitcher_style")?;
        let velocity = self.repo.normal_params(PLY, PTI, "velocity")?;
        let control = self.repo.normal_params(PLY, PTI, "control")?;
        let stamina = self.repo.normal_params(PLY, PTI, "stamina")?;
        let injury_proneness = self.repo.normal_params(PLY, PTI, "injury_proneness")?;
        let clutch = self.repo.normal_params(PLY, PTI, "clutch")?;
        let hpp = self.repo.normal_params(PLY, PTI, "hpp")?;
        let platoon_splitting = self.repo.normal_params(PLY, PTI, "platoon_splitting")?;
        let delivery_motion_time = self.repo.normal_params(PLY, PTI, "delivery_motion_time")?;

        Ok(PitcherInfoProbs {
            pitcher_style: pitcher_style,
            velocity: velocity,
            control: control,
            stamina: stamina,
            injury_proneness: injury_proneness,
            clutch: clutch,
            hpp: hpp,
            platoon_splitting: platoon_splitting,
            delivery_motion_time: delivery_motion_time,
        })
    }

    pub fn load_pitch_type_prob(
        &self,
    ) -> Result<HashMap<PitcherStyle, Vec<ItemWeighted<PitchType>>>, AppError> {
        info!("load_pitch_type_prob() started");

        let mut pitch_type_map: HashMap<PitcherStyle, Vec<ItemWeighted<PitchType>>> =
            HashMap::new();

        for pitcher_style in PitcherStyle::iter() {
            pitch_type_map.entry(pitcher_style).or_insert(
                self.repo
                    .item_probs("pitcher_style", pitcher_style.as_ref())?,
            );
        }

        Ok(pitch_type_map)
    }

    fn load_pitch_skill_prob(&self, pitch_type: PitchType) -> Result<PitchSkillProbs, AppError> {
        info!(
            "load_pitch_skill_prob() started for {}",
            pitch_type.to_string()
        );

        let velocity = self
            .repo
            .normal_params(PT, pitch_type.as_ref(), "velocity")?;
        let control = self
            .repo
            .normal_params(PT, pitch_type.as_ref(), "control")?;
        let stamina = self
            .repo
            .normal_params(PT, pitch_type.as_ref(), "stamina")?;
        let injury_proneness =
            self.repo
                .normal_params(PT, pitch_type.as_ref(), "injury_proneness")?;
        let stuff = self.repo.normal_params(PT, pitch_type.as_ref(), "stuff")?;
        let fb = self.repo.normal_params(PT, pitch_type.as_ref(), "fb")?;
        let gp = self.repo.normal_params(PT, pitch_type.as_ref(), "gp")?;
        let horizontal_movement =
            self.repo
                .normal_params(PT, pitch_type.as_ref(), "horizontal_movement")?;
        let vertical_movement =
            self.repo
                .normal_params(PT, pitch_type.as_ref(), "vertical_movement")?;
        let spin_rate = self
            .repo
            .normal_params(PT, pitch_type.as_ref(), "spin_rate")?;
        let usage = self.repo.normal_params(PT, pitch_type.as_ref(), "usage")?;

        Ok(PitchSkillProbs {
            velocity: velocity,
            control: control,
            stamina: stamina,
            injury_proneness: injury_proneness,
            stuff: stuff,
            fb: fb,
            gp: gp,
            horizontal_movement: horizontal_movement,
            vertical_movement: vertical_movement,
            spin_rate: spin_rate,
            usage: usage,
        })
    }

    pub fn load_pitch_skill_probs(&self) -> Result<HashMap<PitchType, PitchSkillProbs>, AppError> {
        info!("load_pitch_skill_probs() started");

        let mut pitch_skill_map: HashMap<PitchType, PitchSkillProbs> = HashMap::new();

        for pitch_type in PitchType::iter() {
            pitch_skill_map
                .entry(pitch_type)
                .or_insert(self.load_pitch_skill_prob(pitch_type)?);
        }

        Ok(pitch_skill_map)
    }

    pub fn load_random_name(&self) -> Result<FullName, AppError> {
        info!("load_random_name() started");

        self.repo.random_name(I18nManager::global().lang_db())
    }

    pub fn next_team(&self, position: Position) -> Result<Team, AppError> {
        info!("next_team() started");

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

    pub fn save_player(&mut self, team_id: u16, player: &Player) -> Result<(), AppError> {
        info!("save_players() started");

        self.repo.insert_player(team_id, &player)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::{
        BatterInfo, DefenseSkills, FielderInfo, FullName, OffenseSkills, PitchSkill, PitchType,
        PitcherInfo, PitcherStyle, PlayerInfo, Position, RunningSkills,
    };
    use crate::domain::shared::prob::{
        GammaParam, ItemWeighted, NormalParam, PitcherInfoProbs, PlayerInfoProbs,
    };
    use crate::domain::shared::team::Team;
    use crate::error::AppError;
    use crate::repositories::db::FromRow;
    use anyhow::anyhow;
    use rusqlite::Transaction;
    use std::cell::{Cell, RefCell};
    use std::mem::ManuallyDrop;
    use std::rc::Rc;
    use std::str::FromStr;

    struct RecordingRepo {
        state: Rc<RepoState>,
    }

    struct RepoState {
        name: FullName,
        team: Team,
        random_team: Team,
        pitch_type_probs: Vec<ItemWeighted<PitchType>>,
        player_attribute_prob: PlayerInfoProbs,
        pitcher_attribute_prob: PitcherInfoProbs,
        random_name_error: Cell<bool>,
        next_team_error: Cell<bool>,
        next_team_not_found: Cell<bool>,
        next_random_team_error: Cell<bool>,
        player_attribute_prob_error: Cell<bool>,
        batter_skill_prob_error: Cell<bool>,
        position_probs: Vec<ItemWeighted<Position>>,
        position_probs_error: Cell<bool>,
        defensive_skill_prob_error: Cell<bool>,
        pitcher_style_probs: Vec<ItemWeighted<PitcherStyle>>,
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
        saved: RefCell<Vec<(u16, Player)>>,
    }

    impl RecordingRepo {
        fn new() -> (Self, Rc<RepoState>) {
            let normal = NormalParam {
                mean: 1.0,
                std_dev: 0.1,
                skew: 0.0,
                coefficient: 1.0,
                offset: 0.0,
            };
            let state = Rc::new(RepoState {
                name: FullName {
                    first: "Shohei".into(),
                    last: "Ohtani".into(),
                },
                team: Team::min(1, "Lions"),
                random_team: Team::min(99, "Randoms"),
                pitch_type_probs: vec![ItemWeighted {
                    name: PitchType::Slider,
                    weight: 1.0,
                }],
                player_attribute_prob: PlayerInfoProbs {
                    age: GammaParam {
                        shape: 2.5,
                        scale: 0.5,
                        offset: 18.0,
                    },
                },
                pitcher_attribute_prob: PitcherInfoProbs {
                    pitcher_style: vec![ItemWeighted {
                        name: PitcherStyle::BalancedPitcher,
                        weight: 1.0,
                    }],
                    velocity: normal,
                    control: normal,
                    stamina: normal,
                    injury_proneness: normal,
                    clutch: normal,
                    hpp: normal,
                    platoon_splitting: normal,
                    delivery_motion_time: normal,
                },
                random_name_error: Cell::new(false),
                next_team_error: Cell::new(false),
                next_team_not_found: Cell::new(false),
                next_random_team_error: Cell::new(false),
                player_attribute_prob_error: Cell::new(false),
                batter_skill_prob_error: Cell::new(false),
                position_probs: vec![ItemWeighted {
                    name: Position::P,
                    weight: 1.0,
                }],
                position_probs_error: Cell::new(false),
                defensive_skill_prob_error: Cell::new(false),
                pitcher_style_probs: vec![ItemWeighted {
                    name: PitcherStyle::BalancedPitcher,
                    weight: 1.0,
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

        fn cast_item_probs<T>(items: Vec<ItemWeighted<T>>) -> Vec<ItemWeighted<T>> {
            items
        }

        unsafe fn cast_pitch_type_probs<T>(
            items: Vec<ItemWeighted<PitchType>>,
        ) -> Vec<ItemWeighted<T>> {
            let mut items = ManuallyDrop::new(items);
            unsafe {
                Vec::from_raw_parts(
                    items.as_mut_ptr() as *mut ItemWeighted<T>,
                    items.len(),
                    items.capacity(),
                )
            }
        }

        unsafe fn cast_pitcher_style_probs<T>(
            items: Vec<ItemWeighted<PitcherStyle>>,
        ) -> Vec<ItemWeighted<T>> {
            let mut items = ManuallyDrop::new(items);
            unsafe {
                Vec::from_raw_parts(
                    items.as_mut_ptr() as *mut ItemWeighted<T>,
                    items.len(),
                    items.capacity(),
                )
            }
        }
    }

    impl PlayerRepository for RecordingRepo {
        fn insert_player(&mut self, team_id: u16, player: &Player) -> Result<(), AppError> {
            let call_index = self.state.save_calls.get();
            self.state.save_calls.set(call_index + 1);

            if self.state.save_error_at.get() == Some(call_index) {
                return Err(AppError::Internal(anyhow!("save failed")));
            }

            self.state
                .saved
                .borrow_mut()
                .push((team_id, player.clone()));
            Ok(())
        }

        fn insert_offense_skills(
            &self,
            _tx: &Transaction,
            _player_id: i64,
            _offense_skills: &OffenseSkills,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn insert_batter_info(
            &self,
            _tx: &Transaction,
            _player_id: i64,
            _batter_info: &BatterInfo,
        ) -> Result<usize, AppError> {
            Ok(1)
        }

        fn insert_running_skills(
            &self,
            _tx: &Transaction,
            _player_id: i64,
            _running_skills: &RunningSkills,
        ) -> Result<usize, AppError> {
            Ok(1)
        }

        fn insert_defense_skills(
            &self,
            _tx: &Transaction,
            _player_id: i64,
            _defense_skills: &DefenseSkills,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn insert_fielder_info(
            &self,
            _tx: &Transaction,
            _player_id: i64,
            _fielder_info: &FielderInfo,
        ) -> Result<usize, AppError> {
            Ok(1)
        }

        fn insert_pitcher_info(
            &self,
            _tx: &Transaction,
            _player_id: i64,
            _pitcher_attribute: &PitcherInfo,
        ) -> Result<(), AppError> {
            Ok(())
        }

        fn insert_pitch_skill(
            &self,
            _tx: &Transaction,
            _player_id: i64,
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

        fn normal_params(
            &self,
            category1: &str,
            category2: &str,
            _name: &str,
        ) -> Result<NormalParam, AppError> {
            if category1 == PT {
                self.state
                    .pitch_skill_prob_calls
                    .set(self.state.pitch_skill_prob_calls.get() + 1);
                self.state
                    .pitch_skill_prob_types
                    .borrow_mut()
                    .push(PitchType::from_str(category2).unwrap());

                if self.state.pitch_skill_prob_error.get() {
                    return Err(AppError::Internal(anyhow!("pitch skill prob failed")));
                }
            }

            Ok(NormalParam {
                mean: 1.0,
                std_dev: 0.1,
                skew: 0.0,
                coefficient: 1.0,
                offset: 0.0,
            })
        }

        fn gamma_params(
            &self,
            _category1: &str,
            _category2: &str,
            _name: &str,
        ) -> Result<GammaParam, AppError> {
            self.state
                .player_attribute_prob_calls
                .set(self.state.player_attribute_prob_calls.get() + 1);

            if self.state.player_attribute_prob_error.get() {
                return Err(AppError::Internal(anyhow!("player info prob failed")));
            }

            Ok(self.state.player_attribute_prob.age)
        }

        fn item_probs<T>(
            &self,
            category1: &str,
            category2: &str,
        ) -> Result<Vec<ItemWeighted<T>>, AppError>
        where
            ItemWeighted<T>: FromRow<Error = AppError>,
        {
            self.state
                .item_probs_calls
                .set(self.state.item_probs_calls.get() + 1);

            match (category1, category2) {
                (PTI, "pitcher_style") => {
                    self.state
                        .pitcher_style_probs_calls
                        .set(self.state.pitcher_style_probs_calls.get() + 1);

                    if self.state.pitcher_style_probs_error.get() {
                        return Err(AppError::Internal(anyhow!("pitcher style probs failed")));
                    }

                    Ok(unsafe {
                        Self::cast_pitcher_style_probs(self.state.pitcher_style_probs.clone())
                    })
                }
                ("pitcher_style", style) => {
                    self.state
                        .pitch_type_probs_calls
                        .set(self.state.pitch_type_probs_calls.get() + 1);
                    self.state
                        .pitch_type_prob_styles
                        .borrow_mut()
                        .push(PitcherStyle::from_str(style).unwrap());

                    if self.state.pitch_type_probs_error.get() {
                        return Err(AppError::Internal(anyhow!("pitch type probs failed")));
                    }

                    Ok(unsafe { Self::cast_pitch_type_probs(self.state.pitch_type_probs.clone()) })
                }
                _ => panic!(
                    "unexpected item_probs call in PlayerService test: {category1}/{category2}"
                ),
            }
        }
    }

    fn service_with_repo() -> (PlayerService<RecordingRepo>, Rc<RepoState>) {
        let (repo, state) = RecordingRepo::new();
        (PlayerService { repo }, state)
    }

    #[test]
    fn load_player_info_probs_loads_from_repository() {
        let (service, state) = service_with_repo();

        let probs = service.load_player_info_probs().unwrap();

        assert_eq!(probs.age.shape, 2.5);
        assert_eq!(probs.age.scale, 0.5);
        assert_eq!(probs.age.offset, 18.0);
        assert_eq!(state.player_attribute_prob_calls.get(), 1);
    }

    #[test]
    fn load_player_info_probs_returns_error_from_repository() {
        let (repo, state) = RecordingRepo::new();
        state.player_attribute_prob_error.set(true);
        let service = PlayerService { repo };

        assert!(service.load_player_info_probs().is_err());
    }

    #[test]
    fn load_random_name_passes_current_i18n_language() {
        let (service, state) = service_with_repo();

        let name = service.load_random_name().unwrap();

        assert_eq!(name.first, "Shohei");
        assert_eq!(name.last, "Ohtani");
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
    fn pitch_type_prob_loads_all_styles_from_repository() {
        let (service, state) = service_with_repo();

        let pitch_type_map = service.load_pitch_type_prob().unwrap();

        let balanced_probs = pitch_type_map.get(&PitcherStyle::BalancedPitcher).unwrap();
        assert!(matches!(balanced_probs[0].name, PitchType::Slider));
        assert_eq!(
            state.pitch_type_probs_calls.get(),
            PitcherStyle::iter().count()
        );
        assert_eq!(state.item_probs_calls.get(), PitcherStyle::iter().count());
    }

    #[test]
    fn pitch_skill_probs_load_all_pitch_types_from_repository() {
        let (service, state) = service_with_repo();

        let probs = service.load_pitch_skill_probs().unwrap();

        assert!(probs.contains_key(&PitchType::Changeup));
        assert_eq!(
            state.pitch_skill_prob_calls.get(),
            PitchType::iter().count() * 11
        );
        assert!(
            state
                .pitch_skill_prob_types
                .borrow()
                .contains(&PitchType::Changeup)
        );
    }

    #[test]
    fn save_player_delegates_to_repository() {
        let (mut service, state) = service_with_repo();
        let player = Player {
            info: PlayerInfo::new(7, "First".into(), "Last".into(), 25, 18),
            offense_skills: OffenseSkills {
                batter: None,
                running: RunningSkills {
                    speed: 0.0,
                    lead_distance: 0.0,
                    start_reaction: 0.0,
                },
            },
            defense_skills: DefenseSkills::new(Position::P),
        };
        let team = Team::min(3, "Tigers");

        service.save_player(team.id, &player.clone()).unwrap();

        let saved = state.saved.borrow();
        assert_eq!(state.save_calls.get(), 1);
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].0, 3);
        assert_eq!(saved[0].1.info.id, 7);
        assert_eq!(saved[0].1.info.first_name, "First");
    }
}
