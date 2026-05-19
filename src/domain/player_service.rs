use super::shared::player::Player;
use super::utils::{age_random, rl_random, skewed_normal_random};
use crate::i18n::I18nManager;
use crate::repositories::player_repository::PlayerRepository;
use crate::t;
use anyhow::Result;

const SPEED_SKEW: f64 = 0.2;
const CONTROL_SKEW: f64 = 0.2;
const BA_SKEW: f64 = 0.2;
const SLG_SKEW: f64 = 0.2;
const THROW_LEFTY: f64 = 0.2;
const BAT_LEFTY: f64 = 0.4;

pub struct PlayerService<R: PlayerRepository> {
    pub repo: R,
}

impl<R: PlayerRepository> PlayerService<R> {
    // TODO: Divide batter generation and pitcher generation
    pub fn generate_players(&mut self, num_of_players: u16) -> Result<()> {
        for _ in 0..num_of_players {
            let name = self.repo.random_name(I18nManager::global().lang_db())?;
            let age = age_random();
            let throw = rl_random(THROW_LEFTY);
            let mod_speed = skewed_normal_random(SPEED_SKEW);
            let mod_control = skewed_normal_random(CONTROL_SKEW);
            let bat = rl_random(BAT_LEFTY);
            let mod_ba = skewed_normal_random(BA_SKEW);
            let mod_slg = skewed_normal_random(SLG_SKEW);
            let team = self.repo.next_player_dist_team()?;
            let player = Player {
                id: 0,
                first_name: name[0].clone().into(),
                last_name: name[1].clone().into(),
                age: age,
                throw: throw,
                mod_speed: mod_speed,
                mod_control: mod_control,
                defensive_skills: Vec::new(),
                bat: bat,
                mod_ba: mod_ba,
                mod_slg: mod_slg,
            };

            if let Err(e) = self.repo.save_player(team, player) {
                eprintln!("{}:{}", t!("error", "function" => "save_player"), e);
            }
        }
        Ok(())
    }
}
