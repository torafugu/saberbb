use super::player::Batter;
use super::team::Team;
use super::types::Inning;
use super::types::InningType;
use serde::{Deserialize, Serialize};

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub struct GameManager {
    pub season: i16,
    pub phase: i16,
}

#[derive(Clone)]
pub struct Game {
    pub seq: i32,
    pub top_team: Team,
    pub bottom_team: Team,
    pub inning_seq: i8, // the latest inning
    pub top_innings: Vec<Inning>,
    pub bottom_innings: Vec<Inning>,
    pub tb: InningType,
    pub top_batters: [Batter; 10],
    pub bottom_batters: [Batter; 10],
    pub current_top_batter_order: usize, // To change to HashMap
    pub current_bottom_batter_order: usize, // To change to HashMap
    pub current_batter: Batter,
    pub top_total_score: i8,
    pub bottom_total_score: i8,
}
impl Game {
    pub fn next_batter(&mut self) -> Batter {
        if matches!(self.tb, InningType::Top) {
            if self.current_top_batter_order == 9 {
                self.current_top_batter_order = 1;
            } else {
                self.current_top_batter_order += 1;
            }

            self.current_batter = self.top_batters[self.current_top_batter_order].clone();
        } else {
            if self.current_bottom_batter_order == 9 {
                self.current_bottom_batter_order = 1;
            } else {
                self.current_bottom_batter_order += 1;
            }

            self.current_batter = self.bottom_batters[self.current_bottom_batter_order].clone();
        }
        self.current_batter.clone()
    }
    pub fn add_inning(&mut self, inning: Inning) {
        if matches!(self.tb, InningType::Top) {
            self.top_innings.push(inning);
        } else {
            self.bottom_innings.push(inning);
        }
    }
    pub fn add_score(&mut self, score: i8) {
        if matches!(self.tb, InningType::Top) {
            self.top_total_score += score;
        } else {
            self.bottom_total_score += score;
        }
    }
}
