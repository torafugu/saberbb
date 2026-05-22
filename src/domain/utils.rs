use crate::domain::shared::player::RL;
use crate::domain::shared::types::Base;
use rand::distr::weighted::WeightedIndex;
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

pub fn age_random() -> u8 {
    let mut rng = rand::rng();
    let dist = Gamma::new(AGE_SHAPE, AGE_SCALE).unwrap();
    let age = dist.sample(&mut rng) + AGE_OFFSET;
    age.round() as u8
}

pub fn has_unique_elements_sorted<T: Ord>(mut vec: Vec<T>) -> bool {
    vec.sort();
    let len_before = vec.len();
    vec.dedup(); // Remove consecutive duplicates
    len_before == vec.len()
}

pub fn is_base_occupied(bases_occupied: u8, base: Base) -> bool {
    (bases_occupied & (1 << base as u8)) != 0
}

#[derive(Debug, Clone)]
pub struct ItemProb<T> {
    pub name: T,
    pub prob: f64,
}

pub fn choose_item_weighted<T>(items: &[ItemProb<T>]) -> Option<&T> {
    if items.is_empty() {
        return None;
    }

    // Extract weights
    let weights: Vec<f64> = items.iter().map(|item| item.prob).collect();

    let dist = match WeightedIndex::new(&weights) {
        Ok(d) => d,
        Err(_) => return None, // in case weights is all 0 or invalid
    };

    let mut rng = rand::rng();
    let index = dist.sample(&mut rng);

    Some(&items[index].name)
}
