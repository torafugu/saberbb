use crate::domain::shared::game::Base;
use crate::domain::shared::player::RL;
use crate::domain::shared::prob::ItemProb;
use rand::distr::weighted::WeightedIndex;
use rand_distr::{Distribution, Gamma};

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

pub fn age_random(age_shape: f64, age_scale: f64, age_offset: f64) -> u8 {
    let mut rng = rand::rng();
    let dist = Gamma::new(age_shape, age_scale).unwrap();
    let age = dist.sample(&mut rng) + age_offset;
    age.round() as u8
}

pub fn is_base_occupied(bases_occupied: u8, base: Base) -> bool {
    (bases_occupied & (1 << base as u8)) != 0
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── sigmoid ──────────────────────────────────────────────

    #[test]
    fn test_sigmoid_center() {
        assert!((sigmoid(0.0) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn test_sigmoid_large_positive() {
        assert!((sigmoid(10.0) - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_sigmoid_large_negative() {
        assert!((sigmoid(-10.0) - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_sigmoid_symmetry() {
        let x = 2.5;
        assert!((sigmoid(x) - (1.0 - sigmoid(-x))).abs() < 1e-9);
    }

    #[test]
    fn test_sigmoid_monotonic() {
        let a = sigmoid(-1.0);
        let b = sigmoid(0.0);
        let c = sigmoid(1.0);
        assert!(a < b && b < c);
    }

    #[test]
    fn test_sigmoid_extreme_values() {
        assert!(sigmoid(100.0).is_finite());
        assert!(sigmoid(-100.0).is_finite());
    }

    // ── skewed_normal_random ─────────────────────────────────

    #[test]
    fn test_skewed_normal_no_skew_mean_near_zero() {
        let samples: Vec<f64> = (0..10_000).map(|_| skewed_normal_random(0.0)).collect();
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!((mean - 0.0).abs() < 0.1);
    }

    #[test]
    fn test_skewed_normal_positive_skew_shifts_mean_positive() {
        let samples: Vec<f64> = (0..10_000).map(|_| skewed_normal_random(5.0)).collect();
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(mean > 1.0);
    }

    #[test]
    fn test_skewed_normal_negative_skew_shifts_mean_negative() {
        let samples: Vec<f64> = (0..10_000).map(|_| skewed_normal_random(-5.0)).collect();
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(mean < -1.0);
    }

    #[test]
    fn test_skewed_normal_variance_exceeds_one() {
        let samples: Vec<f64> = (0..10_000).map(|_| skewed_normal_random(3.0)).collect();
        let mean = samples.iter().sum::<f64>() / samples.len() as f64;
        let var = samples.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / samples.len() as f64;
        assert!(var > 1.0);
    }

    #[test]
    fn test_skewed_normal_produces_finite_values() {
        for _ in 0..100 {
            assert!(skewed_normal_random(2.0).is_finite());
        }
    }

    // ── rl_random ────────────────────────────────────────────

    #[test]
    fn test_rl_random_always_right_when_lefty_zero() {
        for _ in 0..100 {
            assert_eq!(rl_random(0.0), RL::Right);
        }
    }

    #[test]
    fn test_rl_random_always_left_when_lefty_one() {
        for _ in 0..100 {
            assert_eq!(rl_random(1.0), RL::Left);
        }
    }

    #[test]
    fn test_rl_random_distribution_matches_rate() {
        let trials = 10_000;
        let left_count = (0..trials)
            .filter(|_| matches!(rl_random(0.3), RL::Left))
            .count();
        let ratio = left_count as f64 / trials as f64;
        assert!((ratio - 0.3).abs() < 0.05);
    }

    #[test]
    fn test_rl_random_edge_rates_return_valid_variant() {
        let result = rl_random(0.5);
        assert!(result == RL::Right || result == RL::Left);

        // Very small lefty rate should still return a valid variant
        let small = rl_random(1e-10);
        assert!(small == RL::Right || small == RL::Left);
    }

    // ── age_random ───────────────────────────────────────────

    #[test]
    fn test_age_random_respects_offset_floor() {
        // With tiny Gamma contribution age should round to at least offset
        let age = age_random(0.1, 0.1, 18.0);
        assert!(age >= 18);
    }

    #[test]
    fn test_age_random_returns_reasonable_range() {
        let ages: Vec<u8> = (0..1000).map(|_| age_random(2.5, 2.5, 18.0)).collect();
        let min = ages.iter().min().unwrap();
        let max = ages.iter().max().unwrap();
        assert!(*min >= 18);
        assert!(*max <= 65);
    }

    #[test]
    fn test_age_random_fits_in_u8() {
        // Should never panic and always fit in u8
        for _ in 0..100 {
            let _age = age_random(2.5, 2.5, 18.0);
        }
    }

    #[test]
    fn test_age_random_mean_near_expected() {
        let shape = 2.5;
        let scale = 2.5;
        let offset = 18.0;
        let expected_mean = shape * scale + offset; // 24.25

        let samples: Vec<u8> = (0..10_000)
            .map(|_| age_random(shape, scale, offset))
            .collect();
        let mean = samples.iter().map(|&x| x as f64).sum::<f64>() / samples.len() as f64;
        // Rounding to u8 introduces ~0.5 bias, so use wider tolerance
        assert!((mean - expected_mean).abs() < 1.0);
    }

    // ── is_base_occupied ─────────────────────────────────────

    #[test]
    fn test_is_base_occupied_first() {
        assert!(is_base_occupied(0b001, Base::First));
        assert!(!is_base_occupied(0b001, Base::Second));
        assert!(!is_base_occupied(0b001, Base::Third));
    }

    #[test]
    fn test_is_base_occupied_second() {
        assert!(!is_base_occupied(0b010, Base::First));
        assert!(is_base_occupied(0b010, Base::Second));
        assert!(!is_base_occupied(0b010, Base::Third));
    }

    #[test]
    fn test_is_base_occupied_third() {
        assert!(!is_base_occupied(0b100, Base::First));
        assert!(!is_base_occupied(0b100, Base::Second));
        assert!(is_base_occupied(0b100, Base::Third));
    }

    #[test]
    fn test_is_base_occupied_empty() {
        assert!(!is_base_occupied(0b000, Base::First));
        assert!(!is_base_occupied(0b000, Base::Second));
        assert!(!is_base_occupied(0b000, Base::Third));
    }

    #[test]
    fn test_is_base_occupied_bases_loaded() {
        assert!(is_base_occupied(0b111, Base::First));
        assert!(is_base_occupied(0b111, Base::Second));
        assert!(is_base_occupied(0b111, Base::Third));
    }

    #[test]
    fn test_is_base_occupied_ignores_higher_bits() {
        // Upper bits beyond bit 2 should be ignored
        assert!(is_base_occupied(0b101101, Base::First));
        assert!(!is_base_occupied(0b101101, Base::Second));
        assert!(is_base_occupied(0b101101, Base::Third));
    }

    // ── choose_item_weighted ─────────────────────────────────

    #[test]
    fn test_choose_item_weighted_empty() {
        let items: Vec<ItemProb<&str>> = vec![];
        assert_eq!(choose_item_weighted(&items), None);
    }

    #[test]
    fn test_choose_item_weighted_single_item() {
        let items = vec![ItemProb {
            name: "only",
            prob: 1.0,
        }];
        for _ in 0..50 {
            assert_eq!(choose_item_weighted(&items), Some(&"only"));
        }
    }

    #[test]
    fn test_choose_item_weighted_zero_prob_item_never_chosen() {
        let items = vec![
            ItemProb {
                name: "impossible",
                prob: 0.0,
            },
            ItemProb {
                name: "certain",
                prob: 1.0,
            },
        ];
        for _ in 0..100 {
            assert_eq!(choose_item_weighted(&items), Some(&"certain"));
        }
    }

    #[test]
    fn test_choose_item_weighted_all_zeros_returns_none() {
        let items = vec![
            ItemProb {
                name: "a",
                prob: 0.0,
            },
            ItemProb {
                name: "b",
                prob: 0.0,
            },
        ];
        assert_eq!(choose_item_weighted(&items), None);
    }

    #[test]
    fn test_choose_item_weighted_distribution() {
        let items = vec![
            ItemProb {
                name: "common",
                prob: 0.7,
            },
            ItemProb {
                name: "rare",
                prob: 0.3,
            },
        ];
        let trials = 10_000;
        let mut common_count = 0;
        for _ in 0..trials {
            if choose_item_weighted(&items) == Some(&&"common") {
                common_count += 1;
            }
        }
        let ratio = common_count as f64 / trials as f64;
        assert!((ratio - 0.7).abs() < 0.05);
    }

    #[test]
    fn test_choose_item_weighted_negative_prob_does_not_panic() {
        let items = vec![
            ItemProb {
                name: "bad",
                prob: -0.5,
            },
            ItemProb {
                name: "good",
                prob: 1.0,
            },
        ];
        let result = std::panic::catch_unwind(|| choose_item_weighted(&items));
        assert!(result.is_ok());
    }

    #[test]
    fn test_choose_item_weighted_three_items_returns_valid() {
        let items = vec![
            ItemProb {
                name: "A",
                prob: 0.1,
            },
            ItemProb {
                name: "B",
                prob: 0.3,
            },
            ItemProb {
                name: "C",
                prob: 0.6,
            },
        ];
        for _ in 0..50 {
            let result = choose_item_weighted(&items);
            assert!(result.is_some());
            let name = *result.unwrap();
            assert!(name == "A" || name == "B" || name == "C");
        }
    }
}
