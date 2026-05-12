use super::shared::game::{GameRound, GameState, InningState};
use crate::t;
use anyhow::{Context, Result};

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
            let mut game_state = GameState::new();

            // TODO: Check postponement
            game.actual_date = game.planned_date;

            // TODO: Implement No DH case
            game.away_players = game.away_team.lineup(true);
            game.home_players = game.home_team.lineup(true);

            // loop for an innning
            while game_state.is_active() {
                let mut inning = game_state.new_inning();
                let mut inning_state = InningState::new();

                while inning_state.is_active() {
                    inning_state.add_count_seq();

                    let current_batter = game
                        .away_players
                        .iter()
                        .find(|i| i.order == game_state.batter_order())
                        .expect(&t!("batter_not_found"))
                        .clone()
                        .player;

                    let count = inning_state.batting_resolve(&current_batter);
                    game_state.update(count.point);
                    inning.add_count(count);

                    // Check walk-off
                    if game_state.is_walk_off() {
                        break;
                    }
                }

                // update both point and batting order
                game.update_point(&game_state);
                game.innings.push(inning);

                // Check Game-Set
                if game_state.is_game_set() {
                    break;
                }
            }
        }

        if let Err(e) = self.repo.save_game_round(&game_round) {
            eprintln!("{}:{}", t!("error", "function" => "save_game_round"), e);
        }

        Ok(())
    }
}
