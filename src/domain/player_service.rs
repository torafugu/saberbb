use anyhow::Result;
use rand_distr::Distribution;

pub trait PlayerRepository {
    fn save_players(&mut self, num_of_players: i16) -> Result<()>;
}

pub struct PlayerService<R: PlayerRepository> {
    pub repo: R,
}

impl<R: PlayerRepository> PlayerService<R> {
    pub fn generate_players(&mut self) -> Result<()> {
        let mut rng = rand::rng();

        for _ in 0..30 {
            // Skew-Normal Distribution
            let normal = rand_distr::Normal::new(0.0, 1.0).unwrap();
            let val1: f64 = normal.sample(&mut rng);
            let val2: f64 = normal.sample(&mut rng).abs();

            let alpha: f64 = 0.5; // Skew level
            let skewed_val = val1 + alpha * val2;

            println!("{:.4}", skewed_val);
        }
        Ok(())
    }
}
