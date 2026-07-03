use rand::{RngExt, SeedableRng, rngs::StdRng};

pub trait RandomProvider: std::fmt::Debug {
    fn random(&mut self) -> f64;
}

#[derive(Debug)]
pub struct RealRng(pub StdRng);

impl RealRng {
    pub fn new() -> Self {
        Self(rand::make_rng())
    }

    pub fn from_seed(seed: u64) -> Self {
        Self(StdRng::seed_from_u64(seed))
    }
}

impl RandomProvider for RealRng {
    fn random(&mut self) -> f64 {
        self.0.random()
    }
}

#[derive(Debug)]
pub struct FixedRng {
    value: f64,
}

impl FixedRng {
    pub fn new(value: f64) -> Self {
        Self { value }
    }
}

impl RandomProvider for FixedRng {
    fn random(&mut self) -> f64 {
        self.value
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_generates_same_sequence() {
        let mut rng1 = RealRng::from_seed(1);
        let mut rng2 = RealRng::from_seed(1);

        assert_eq!(rng1.random(), rng2.random());
        assert_eq!(rng1.random(), rng2.random());
        assert_eq!(rng1.random(), rng2.random());
    }

    #[test]
    fn different_seeds_generate_different_values() {
        let mut rng1 = RealRng::from_seed(1);
        let mut rng2 = RealRng::from_seed(2);

        assert_ne!(rng1.random(), rng2.random());
    }

    #[test]
    fn fixed_rng_returns_fixed_value() {
        let mut rng = FixedRng::new(0.1);

        assert_eq!(rng.random(), 0.1);
        assert_eq!(rng.random(), 0.1);
        assert_eq!(rng.random(), 0.1);
    }
}
