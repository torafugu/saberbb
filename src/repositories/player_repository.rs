use crate::domain::player_service::PlayerRepository;
use anyhow::Result;
use rusqlite::{Connection, params};

pub struct SqlPlayerRepository {
    pub pool: Connection,
}

impl PlayerRepository for SqlPlayerRepository {
    fn save_players(&mut self, num_of_players: i16) -> Result<()> {
        Ok(())
    }
}
