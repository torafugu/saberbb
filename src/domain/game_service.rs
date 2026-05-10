use super::resolver::batting_resolve;
use super::shared::game::{Bases, Count, GameRound, Inning, MAX_INNING, MAX_OUT};
use super::shared::player::Player;
use super::shared::types::{BattingResult, InningType};
use super::utils::next_tb;
use crate::t;
use anyhow::{Context, Result};
use std::i16;
use std::sync::Arc;

pub trait GameRepository {
    fn save_game_round(&mut self, round: &GameRound) -> Result<()>;
    fn load_game_round_to_process(&self) -> Result<GameRound>;
}

pub struct GameService<R: GameRepository> {
    pub repo: R,
}

impl<R: GameRepository> GameService<R> {
    pub fn process_game_round(&mut self) -> Result<()> {
        // 1. Get game round to process
        let mut game_round = self
            .repo
            .load_game_round_to_process()
            .context(t!("error", "function" => "load_game_round_to_process"))?;

        // 2. Procees games in the game round
        for game in game_round.games.iter_mut() {
            let mut is_in_game: bool = true;
            let mut inning_seq: i8 = 1;
            let mut inning_tb: InningType = InningType::Top;
            let mut top_total_score: i8 = 0;
            let mut bottom_total_score: i8 = 0;
            let mut top_batter_order: usize = 1;
            let mut bottom_batter_order: usize = 1;

            // TODO: Check postponement
            game.actual_date = game.planned_date;

            // TODO: Implement No DH case
            game.away_players = game.away_team.lineup(true);
            game.home_players = game.home_team.lineup(true);

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

                    let current_batter: Player;

                    if inning_tb == InningType::Top {
                        current_batter = game
                            .away_players
                            .iter()
                            .find(|i| i.order == top_batter_order as i8)
                            .expect(&t!("batter_not_found"))
                            .clone()
                            .player;
                    } else {
                        current_batter = game
                            .home_players
                            .iter()
                            .find(|i| i.order == bottom_batter_order as i8)
                            .expect(&t!("batter_not_found"))
                            .clone()
                            .player;
                    }

                    let mut count = Count {
                        seq: count_seq,
                        bases: bases.clone(),
                        batter: Arc::new(current_batter),
                        result: BattingResult::Out,
                        point: 0,
                        out,
                    };

                    // Batting result calculation
                    let batting_result = batting_resolve(&count.batter);
                    if batting_result == BattingResult::Out {
                        out += 1;
                    }

                    count.point = bases.advance(&batting_result);
                    inning.point += count.point;
                    count.result = batting_result;

                    // inning.point = count.bases_advance(batting_result);

                    if inning_tb == InningType::Top {
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
                        && inning_tb == InningType::Bottom
                        && bottom_total_score > top_total_score
                    {
                        is_in_game = false;
                        break;
                    }
                }

                if inning_tb == InningType::Bottom {
                    game.home_point += inning.point as i16;
                } else {
                    game.away_point += inning.point as i16;
                }
                game.innings.push(inning);

                // Check Game-Set
                if inning_seq == MAX_INNING {
                    if inning_tb == InningType::Bottom {
                        break;
                    } else if bottom_total_score > top_total_score {
                        break;
                    }
                } else {
                    if inning_tb == InningType::Bottom {
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
