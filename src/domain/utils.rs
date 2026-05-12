use crate::domain::shared::{player::RL, types::InningType};
use rand_distr::{Distribution, Gamma};

// TODO: Get from config file
const AGE_SHAPE: f64 = 2.5;
const AGE_SCALE: f64 = 2.5;
const AGE_OFFSET: f64 = 18.0;

pub fn sigmoid(x: f64) -> f64 {
    1.0 / (1.0 + (-x).exp())
}

pub fn skewed_normal_random(skew_level: f64) -> f64 {
    let mut rng = rand::rng();
    let normal = rand_distr::Normal::new(0.0, 1.0).unwrap();
    let val1: f64 = normal.sample(&mut rng);
    let val2: f64 = normal.sample(&mut rng).abs();

    val1 + skew_level * val2
}

pub fn rl_random(lefty_rate: f64) -> RL {
    let mut rl = RL::Right;
    let rng: f64 = rand::random();
    if rng < lefty_rate {
        rl = RL::Left;
    }
    rl
}

pub fn age_random() -> i8 {
    let mut rng = rand::rng();
    let dist = Gamma::new(AGE_SHAPE, AGE_SCALE).unwrap();
    let age = dist.sample(&mut rng) + AGE_OFFSET;
    age.round() as i8
}
