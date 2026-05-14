use super::shared::game::{Game, GameRound, GameState, InningState, Lineup};
use crate::t;
use anyhow::{Context, Result};

pub trait GameRepository {
    fn save_game_round(&mut self, round: &GameRound) -> Result<()>;
    fn load_game_round_to_process(&self) -> Result<GameRound>;
    fn load_processed_games(&self, season: i16) -> Result<Vec<Game>>;
    fn load_processed_seasons(&self) -> Result<Vec<i16>>;
    fn load_games(&self, game_round: &GameRound) -> Result<Vec<Game>>;
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
            // TODO: Implement No DH case
            game_state.away_batting_lineup = Lineup::new(game.away_team.lineup(true));
            game_state.home_batting_lineup = Lineup::new(game.home_team.lineup(true));

            // TODO: Check postponement
            game.actual_date = game.planned_date;

            // loop for an innning
            while game_state.is_active() {
                let mut inning = game_state.advance_half_inning();
                let mut inning_state = InningState::new();

                while inning_state.is_active() {
                    inning_state.add_count_seq();

                    let count = inning_state.batting_resolve(&game_state.current_batter());
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
