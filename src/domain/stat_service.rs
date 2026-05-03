pub trait GameRepository {
    fn load_game_results(&self) -> Result<GameRound>;
}
