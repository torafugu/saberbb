use super::resolver::batting_resolve;
use super::shared::game::{Bases, Count, GameRound, Inning, MAX_INNING, MAX_OUT};
use super::shared::player::Batter;
use super::shared::types::{BattingResult, InningType};
use super::shared::utils::next_tb;
use crate::t;
use anyhow::{Context, Result};
use std::sync::Arc;

pub trait GameRepository {
    fn save_game_round(&mut self, round: &GameRound) -> Result<()>;
    fn load_game_round_to_process(&self) -> Result<GameRound>;
}

pub struct GameService<R: GameRepository> {
    pub repo: R,
}

impl<R: GameRepository> GameService<R> {
    pub fn process_game(&mut self) -> Result<()> {
        // 1. Get game round to process
        let mut game_round = self
            .repo
            .load_game_round_to_process()
            .context(t!("error", "function" => "load_game_round_to_process"))?;

        // 2. Procees games in the game round
        for game in game_round.games.iter_mut() {
            let mut is_in_game: bool = true;
            let mut inning_seq: i8 = 1;
            let mut inning_tb: InningType = InningType::TOP;
            let mut top_total_score: i8 = 0;
            let mut bottom_total_score: i8 = 0;
            let mut top_batter_order: usize = 1;
            let mut bottom_batter_order: usize = 1;

            game.away_batters = Vec::from([
                Batter::new(1, "Top batter 1", 1.0, -0.5),
                Batter::new(2, "Top batter 2", 1.2, -0.8),
                Batter::new(3, "Top batter 3", 1.4, 0.8),
                Batter::new(4, "Top batter 4", 1.6, 1.0),
                Batter::new(5, "Top batter 5", 1.5, 0.9),
                Batter::new(6, "Top batter 6", -0.1, 0.2),
                Batter::new(7, "Top batter 7", 0.1, -0.3),
                Batter::new(8, "Top batter 8", -1.0, -0.5),
                Batter::new(9, "Top batter 9", -1.2, -1.2),
            ]);
            game.home_batters = Vec::from([
                Batter::new(10, "Bottom batter 1", 0.9, -0.8),
                Batter::new(11, "Bottom batter 2", 1.1, -0.6),
                Batter::new(12, "Bottom batter 3", 1.2, 1.0),
                Batter::new(13, "Bottom batter 4", 1.4, 1.4),
                Batter::new(14, "Bottom batter 5", 0.2, 1.1),
                Batter::new(15, "Bottom batter 6", -0.5, -0.2),
                Batter::new(16, "Bottom batter 7", -0.8, -0.1),
                Batter::new(17, "Bottom batter 8", -1.3, -0.3),
                Batter::new(18, "Bottom batter 9", -1.4, -0.4),
            ]);

            // loop for an innning
            while is_in_game {
                let mut inning: Inning = Inning {
                    tb: inning_tb,
                    seq: inning_seq,
                    counts: Vec::new(),
                    point: 0,
                };
                let mut count_seq = 0;
                let mut bases = Bases::new();
                let mut out = 0;

                while out < MAX_OUT {
                    count_seq += 1;

                    let current_batter: &Batter;
                    if inning_tb == InningType::TOP {
                        current_batter = &game.away_batters[top_batter_order - 1];
                    } else {
                        current_batter = &game.home_batters[bottom_batter_order - 1];
                    }

                    let mut count = Count {
                        seq: count_seq,
                        bases: bases.clone(),
                        batter: Arc::new(current_batter.clone()),
                        result: BattingResult::OUT,
                        point: 0,
                        out,
                    };

                    // Batting result calculation
                    let batting_result = batting_resolve(&count.batter);
                    if batting_result == BattingResult::OUT {
                        out += 1;
                    }

                    count.point = bases.advance(&batting_result);
                    inning.point += count.point;
                    count.result = batting_result;

                    // inning.point = count.bases_advance(batting_result);

                    if inning_tb == InningType::TOP {
                        top_total_score += count.point;
                        if top_batter_order == 9 {
                            top_batter_order = 1;
                        } else {
                            top_batter_order += 1;
                        }
                    } else {
                        bottom_total_score += count.point;
                        if bottom_batter_order == 9 {
                            bottom_batter_order = 1;
                        } else {
                            bottom_batter_order += 1;
                        }
                    }
                    inning.counts.push(count);

                    // Check walk-off
                    if inning_seq == MAX_INNING
                        && inning_tb == InningType::BOTTOM
                        && bottom_total_score > top_total_score
                    {
                        is_in_game = false;
                        break;
                    }
                }

                game.innings.push(inning);

                // Check Game-Set
                if inning_seq == MAX_INNING {
                    if inning_tb == InningType::BOTTOM {
                        break;
                    } else if bottom_total_score > top_total_score {
                        break;
                    }
                } else {
                    if inning_tb == InningType::BOTTOM {
                        inning_seq += 1;
                    }
                }
                inning_tb = next_tb(inning_tb);
            }
        }

        if let Err(e) = self.repo.save_game_round(&game_round) {
            eprintln!("{}:{}", t!("error", "function" => "save_game_round"), e);
        }

        Ok(())
    }
}
