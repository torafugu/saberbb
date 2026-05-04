use crate::domain::shared::team::Standing;
use crate::t;
use anyhow::{Context, Result};

pub trait StatRepository {
    fn load_stadings(&self) -> Result<Vec<Standing>>;
}

pub struct StatService<R: StatRepository> {
    pub repo: R,
}

impl<R: StatRepository> StatService<R> {
    pub fn show_standing(&self) -> Result<Vec<Standing>> {
        let mut standings = self
            .repo
            .load_stadings()
            .context(t!("error", "function" => "load_stadings"))?;

        // 1. Sort by pct
        // partial_cmp is used to compare f32
        standings.sort_by(|a, b| b.pct.partial_cmp(&a.pct).unwrap());

        // 2. Get wins and loses of leader team
        // standings must not be null
        let leader_wins = standings[0].wins;
        let leader_losses = standings[0].losses;

        // 3. Update GB of all teams
        for s in standings.iter_mut() {
            let win_diff = leader_wins - s.wins;
            let loss_diff = s.losses - leader_losses;
            s.gb = (f32::from(win_diff) + f32::from(loss_diff)) / 2.0;
        }

        Ok(standings)
    }
}
