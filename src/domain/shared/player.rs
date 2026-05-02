use super::utils;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

const BATTING_MIN_HIT_AVERAGE: f64 = 0.2;
const BATTING_MAX_HIT_AVERAGE: f64 = 0.32;
const BATTING_MIN_SLG: f64 = 0.3;
const BATTING_MAX_SLG: f64 = 0.55;
const DUMMY_BATTER_TEXT: &str = "Dummy Batter";

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Batter {
    pub id: i32,
    pub name: Arc<str>,
    pub mod_ba: f64,
    pub mod_slg: f64,
}

impl Batter {
    pub fn new(id: i32, name: &str, mod_ba: f64, mod_slg: f64) -> Batter {
        Batter {
            id: id,
            name: Arc::from(name),
            mod_ba: mod_ba,
            mod_slg: mod_slg,
        }
    }

    pub fn hit_average(&self) -> f64 {
        let ba = (BATTING_MAX_HIT_AVERAGE + BATTING_MIN_HIT_AVERAGE) * 0.5
            + (BATTING_MAX_HIT_AVERAGE - BATTING_MIN_HIT_AVERAGE)
                * (utils::sigmoid(self.mod_ba) - 0.5);
        ba
    }

    pub fn slg(&self) -> f64 {
        let slg = (BATTING_MAX_SLG + BATTING_MIN_SLG) * 0.5
            + (BATTING_MAX_SLG - BATTING_MIN_SLG) * (utils::sigmoid(self.mod_slg) - 0.5);
        slg
    }
}

impl Default for Batter {
    fn default() -> Self {
        Self {
            id: 0,
            name: Arc::from(DUMMY_BATTER_TEXT),
            mod_ba: 0.0,
            mod_slg: 0.0,
        }
    }
}
