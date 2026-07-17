use super::player::Position;
use crate::domain::random_provider::RandomProvider;
use crate::domain::shared::game_state::{
    ActiveBatter, ActiveCatcher, ActiveFielder, ActivePitcher, ActiveRunner, GameError,
};
use crate::domain::shared::player::Player;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use validator::Validate;

pub const MAX_LINEUP_PLAYERS: usize = 10;

#[derive(Serialize, Deserialize, Clone, Debug, Validate)]
pub struct League {
    pub id: u16,
    pub name: Arc<str>,
    pub teams: Vec<Team>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Validate)]
pub struct Team {
    pub id: u16,
    pub name: Arc<str>,
    pub players: Vec<Player>,
}
impl Team {
    pub fn min(id: u16, name: &str) -> Self {
        Self {
            id: id,
            name: name.into(),
            players: Vec::new(),
        }
    }

    pub fn lineup(&mut self, mut rng: Box<dyn RandomProvider>) -> Result<Lineup, GameError> {
        let mut batters = Vec::new();
        let mut fielders = Vec::new();
        let mut temp_pitcher: Option<ActivePitcher> = None;
        let mut temp_catcher: Option<ActiveCatcher> = None;

        let mut position_map = self.init_position_hashmap();

        for player in &self.players {
            position_map
                .entry(player.defense_skills.position)
                .or_insert_with(Vec::new)
                .push(player);
        }

        for (key, vec) in position_map {
            if vec.is_empty() {
                return Err(GameError::NoPlayerFor(key.to_string()));
            }

            // TODO: Consider player ability to select the lineup
            let index = rng.gen_range(0, vec.len() - 1);
            let random_player = vec[index];

            if random_player.defense_skills.position != Position::P {
                batters.push(ActiveBatter::new(
                    random_player.info.id,
                    random_player.batter()?,
                    random_player.runner(),
                ));
            } else {
                temp_pitcher = Some(ActivePitcher {
                    id: random_player.info.id,
                    pitcher: random_player.pitcher()?,
                });
            }

            if random_player.defense_skills.position == Position::C {
                temp_catcher = Some(ActiveCatcher {
                    id: random_player.info.id,
                    catcher: random_player.catcher()?,
                });
            }

            if random_player.defense_skills.position != Position::DH {
                fielders.push(ActiveFielder::new(
                    random_player.defense_skills.position,
                    random_player.info.id,
                    random_player.fielder()?,
                ));
            }
        }

        let pitcher = temp_pitcher.ok_or(GameError::NoPlayerFor(Position::P.to_string()))?;
        let catcher = temp_catcher.ok_or(GameError::NoPlayerFor(Position::C.to_string()))?;

        // TODO: The batting order should be considered by team starategy.
        batters.sort_by(|a, b| b.batter.swing_speed.total_cmp(&a.batter.swing_speed));
        for (order, active_batter) in batters.iter_mut().enumerate() {
            active_batter.index = (order + 1) as u8;
        }

        Ok(Lineup::new(batters, fielders, pitcher, catcher)?)
    }

    fn init_position_hashmap(&self) -> HashMap<Position, Vec<&Player>> {
        let mut map: HashMap<Position, Vec<&Player>> = HashMap::new();
        for position in Position::ALL {
            map.entry(position).or_insert_with(Vec::new);
        }
        map
    }
}

#[derive(Clone, Debug)]
pub struct Lineup {
    pub current_index: usize,
    pub batters: [ActiveBatter; 9],
    pub fielders: [ActiveFielder; 9],
    pub pitcher: ActivePitcher,
    pub catcher: ActiveCatcher,
}
impl Lineup {
    pub fn new(
        vec_batters: Vec<ActiveBatter>,
        vec_fielders: Vec<ActiveFielder>,
        pitcher: ActivePitcher,
        catcher: ActiveCatcher,
    ) -> Result<Self, GameError> {
        let arr_batters: [ActiveBatter; 9] = vec_batters
            .try_into()
            .expect("Number of batters must be 9.");
        let arr_fielders: [ActiveFielder; 9] = vec_fielders
            .try_into()
            .expect("Number of batters must be 9.");

        Ok(Self {
            current_index: 0,
            batters: arr_batters,
            fielders: arr_fielders,
            pitcher: pitcher,
            catcher: catcher,
        })
    }

    pub fn next(&mut self) -> Result<&ActiveBatter, GameError> {
        if self.batters.is_empty() {
            return Err(GameError::Lineup("batters are empty.".to_string()));
        }

        let active_batter = &self.batters[self.current_index];
        self.current_index = (self.current_index + 1) % self.batters.len();

        Ok(active_batter)
    }
}
