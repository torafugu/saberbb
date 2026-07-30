use crate::domain::shared::prob::ItemWeighted;
use crate::domain::shared::prob::{GammaParam, NormalParam};
use crate::error::AppError;
use rand::{RngExt, SeedableRng, rngs::StdRng};
use rand_distr::{Distribution, Gamma};

pub trait RandomProvider: std::fmt::Debug {
    fn random(&mut self) -> f64;
    fn gen_range(&mut self, low: usize, high: usize) -> usize;
    fn range_f64(&mut self, low: f64, high: f64) -> f64;
    fn normal(&mut self, normal: NormalParam) -> f64;
    fn normal_factor_std_1percent(&mut self) -> f64;
    fn normal_random(
        &mut self,
        mean: f64,
        std_dev: f64,
        skew: f64,
        coefficient: f64,
        offset: f64,
    ) -> f64;
    fn gamma(&mut self, gamma: GammaParam) -> f64;
    fn gamma_random(&mut self, shape: f64, scale: f64, offset: f64) -> f64;
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

    fn gen_range(&mut self, low: usize, high: usize) -> usize {
        self.0.random_range(low..=high)
    }

    fn range_f64(&mut self, low: f64, high: f64) -> f64 {
        self.0.random_range(low..high)
    }

    fn normal(&mut self, normal: NormalParam) -> f64 {
        self.normal_random(
            normal.mean,
            normal.std_dev,
            normal.skew,
            normal.coefficient,
            normal.offset,
        )
    }

    fn normal_factor_std_1percent(&mut self) -> f64 {
        self.normal_random(1.0, 1.01, 0.0, 1.0, 0.0)
    }

    fn normal_random(
        &mut self,
        mean: f64,
        std_dev: f64,
        skew: f64,
        coefficient: f64,
        offset: f64,
    ) -> f64 {
        let normal = rand_distr::Normal::new(mean, std_dev).unwrap();

        if skew == 0.0 {
            normal.sample(&mut self.0) * coefficient + offset
        } else {
            let val1: f64 = normal.sample(&mut self.0);
            let val2: f64 = normal.sample(&mut self.0).abs();
            (val1 + skew * val2) * coefficient + offset
        }
    }

    fn gamma(&mut self, gamma: GammaParam) -> f64 {
        self.gamma_random(gamma.shape, gamma.scale, gamma.offset)
    }

    fn gamma_random(&mut self, shape: f64, scale: f64, offset: f64) -> f64 {
        let dist = Gamma::new(shape, scale).unwrap();
        dist.sample(&mut self.0) + offset
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

    fn gen_range(&mut self, low: usize, high: usize) -> usize {
        low + (self.value * (high - low) as f64).round() as usize
    }

    fn range_f64(&mut self, low: f64, high: f64) -> f64 {
        low + self.value * (high - low)
    }

    fn normal(&mut self, normal: NormalParam) -> f64 {
        self.normal_random(
            normal.mean,
            normal.std_dev,
            normal.skew,
            normal.coefficient,
            normal.offset,
        )
    }

    fn normal_factor_std_1percent(&mut self) -> f64 {
        self.normal_random(1.0, 1.01, 0.0, 1.0, 0.0)
    }

    fn normal_random(
        &mut self,
        mean: f64,
        _std_dev: f64,
        skew: f64,
        coefficient: f64,
        offset: f64,
    ) -> f64 {
        // Return the expected value as a deterministic result
        if skew == 0.0 {
            mean * coefficient + offset
        } else {
            mean + skew * mean.abs() * coefficient + offset
        }
    }

    fn gamma(&mut self, gamma: GammaParam) -> f64 {
        self.gamma_random(gamma.shape, gamma.scale, gamma.offset)
    }

    fn gamma_random(&mut self, shape: f64, scale: f64, offset: f64) -> f64 {
        // Return the mean of the Gamma distribution as a deterministic value
        shape * scale + offset
    }
}

#[track_caller]
pub fn choose_item_weighted<'a, T>(
    rng: &mut dyn RandomProvider,
    items: &'a [ItemWeighted<T>],
) -> Result<&'a T, AppError> {
    if items.is_empty() {
        let caller = std::panic::Location::caller();
        return Err(AppError::NotFound(format!(
            "item at {}:{}",
            caller.file(),
            caller.line()
        )));
    }

    let total_weight: f64 = items.iter().map(|item| item.weight).sum();
    if total_weight <= 0.0 {
        let caller = std::panic::Location::caller();
        return Err(AppError::NotFound(format!(
            "weights at {}:{}",
            caller.file(),
            caller.line()
        )));
    }

    let target = rng.random() * total_weight;
    let mut cumulative = 0.0;

    for item in items {
        cumulative += item.weight;
        if target <= cumulative {
            return Ok(&item.name);
        }
    }

    Ok(&items[items.len() - 1].name)
}

pub fn choose_item_if_exists<T: Clone>(
    rng: &mut dyn RandomProvider,
    items: &[ItemWeighted<T>],
) -> Result<Vec<T>, AppError> {
    let mut existing_items = Vec::new();

    for item in items {
        if rng.random() < item.weight {
            existing_items.push(item.name.clone());
        }
    }

    Ok(existing_items)
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

    #[test]
    fn same_seed_generates_same_gamma() {
        let mut rng1 = RealRng::from_seed(42);
        let mut rng2 = RealRng::from_seed(42);

        assert_eq!(
            rng1.gamma_random(2.0, 3.0, 1.0),
            rng2.gamma_random(2.0, 3.0, 1.0)
        );
    }

    #[test]
    fn different_seeds_generate_different_gamma() {
        let mut rng1 = RealRng::from_seed(1);
        let mut rng2 = RealRng::from_seed(2);

        assert_ne!(
            rng1.gamma_random(2.0, 3.0, 1.0),
            rng2.gamma_random(2.0, 3.0, 1.0)
        );
    }

    #[test]
    fn real_gamma_random_is_at_least_offset() {
        let mut rng = RealRng::from_seed(99);

        let result = rng.gamma_random(2.0, 0.5, 10.0);

        assert!(result >= 10.0);
    }

    #[test]
    fn real_gamma_random_with_different_shapes_produces_different_results() {
        let mut rng = RealRng::from_seed(7);

        let result_a = rng.gamma_random(1.0, 1.0, 0.0);
        let result_b = rng.gamma_random(10.0, 1.0, 0.0);

        // Larger shape should produce a larger value on average
        assert!(result_b > result_a);
    }

    #[test]
    fn real_gamma_random_does_not_panic_with_various_parameters() {
        let mut rng = RealRng::from_seed(123);

        // These should not panic
        let _ = rng.gamma_random(0.5, 1.0, 0.0);
        let _ = rng.gamma_random(1.0, 2.5, -5.0);
        let _ = rng.gamma_random(5.0, 0.1, 100.0);
        let _ = rng.gamma_random(100.0, 0.01, -0.5);
    }

    #[test]
    fn fixed_gamma_random_returns_mean_of_distribution() {
        let mut rng = FixedRng::new(0.5);
        let shape = 2.0;
        let scale = 3.0;
        let offset = 1.0;

        let result = rng.gamma_random(shape, scale, offset);

        // FixedRng returns shape * scale + offset (the mean)
        assert_eq!(result, 2.0 * 3.0 + 1.0);
    }

    #[test]
    fn fixed_gamma_random_differs_with_different_parameters() {
        let mut rng = FixedRng::new(0.5);

        let result1 = rng.gamma_random(1.0, 1.0, 0.0);
        let result2 = rng.gamma_random(2.0, 3.0, 5.0);

        assert_eq!(result1, 1.0);
        assert_eq!(result2, 2.0 * 3.0 + 5.0);
    }

    #[test]
    fn same_seed_generates_same_normal() {
        let mut rng1 = RealRng::from_seed(10);
        let mut rng2 = RealRng::from_seed(10);

        assert_eq!(
            rng1.normal_random(5.0, 2.0, 0.0, 1.0, 0.0),
            rng2.normal_random(5.0, 2.0, 0.0, 1.0, 0.0)
        );
    }

    #[test]
    fn different_seeds_generate_different_normal() {
        let mut rng1 = RealRng::from_seed(1);
        let mut rng2 = RealRng::from_seed(2);

        assert_ne!(
            rng1.normal_random(5.0, 2.0, 0.0, 1.0, 0.0),
            rng2.normal_random(5.0, 2.0, 0.0, 1.0, 0.0)
        );
    }

    #[test]
    fn real_normal_random_skew_produces_asymmetric_result() {
        let mut rng = RealRng::from_seed(42);

        let no_skew = rng.normal_random(10.0, 3.0, 0.0, 1.0, 0.0);
        let skewed = rng.normal_random(10.0, 3.0, 2.0, 1.0, 0.0);

        // Skewed version should be different (larger due to positive skew)
        assert_ne!(no_skew, skewed);
    }

    #[test]
    fn real_normal_random_does_not_panic_with_various_parameters() {
        let mut rng = RealRng::from_seed(7);

        let _ = rng.normal_random(0.0, 1.0, 0.0, 1.0, 0.0);
        let _ = rng.normal_random(100.0, 15.0, -2.0, 0.5, -5.0);
        let _ = rng.normal_random(-10.0, 5.0, 1.0, 3.0, 2.0);
    }

    #[test]
    fn fixed_normal_random_without_skew_returns_expected_value() {
        let mut rng = FixedRng::new(0.5);

        let result = rng.normal_random(5.0, 2.0, 0.0, 3.0, 1.0);

        assert_eq!(result, 5.0 * 3.0 + 1.0);
    }

    #[test]
    fn fixed_normal_random_with_positive_skew_includes_skew_term() {
        let mut rng = FixedRng::new(0.5);

        let result = rng.normal_random(5.0, 2.0, 2.0, 3.0, 1.0);

        assert_eq!(result, 5.0 + 2.0 * 5.0_f64.abs() * 3.0 + 1.0);
    }

    #[test]
    fn fixed_normal_random_with_negative_skew_uses_absolute_mean() {
        let mut rng = FixedRng::new(0.5);

        let result = rng.normal_random(5.0, 2.0, -1.5, 2.0, 0.5);

        assert_eq!(result, 5.0 + (-1.5) * 5.0_f64.abs() * 2.0 + 0.5);
    }

    #[test]
    fn fixed_normal_random_differs_with_different_coefficient_and_offset() {
        let mut rng = FixedRng::new(0.3);

        let result_a = rng.normal_random(10.0, 1.0, 0.0, 1.0, 0.0);
        let result_b = rng.normal_random(10.0, 1.0, 0.0, 2.0, 5.0);

        assert_eq!(result_a, 10.0);
        assert_eq!(result_b, 10.0 * 2.0 + 5.0);
    }

    #[test]
    fn same_seed_generates_same_gen_range() {
        let mut rng1 = RealRng::from_seed(55);
        let mut rng2 = RealRng::from_seed(55);

        assert_eq!(rng1.gen_range(0, 100), rng2.gen_range(0, 100));
    }

    #[test]
    fn different_seeds_generate_different_gen_range() {
        let mut rng1 = RealRng::from_seed(1);
        let mut rng2 = RealRng::from_seed(2);

        assert_ne!(rng1.gen_range(10, 20), rng2.gen_range(10, 20));
    }

    #[test]
    fn real_gen_range_stays_within_bounds() {
        let mut rng = RealRng::from_seed(42);

        for _ in 0..20 {
            let val = rng.gen_range(5, 15);
            assert!(val >= 5);
            assert!(val <= 15);
        }
    }

    #[test]
    fn real_gen_range_handles_narrow_range() {
        let mut rng = RealRng::from_seed(99);

        let val = rng.gen_range(7, 7);
        assert_eq!(val, 7);
    }

    #[test]
    fn fixed_gen_range_returns_value_based_on_fixed_proportion() {
        let mut rng = FixedRng::new(0.5);

        let val = rng.gen_range(0, 100);

        assert_eq!(val, 50);
    }

    #[test]
    fn fixed_gen_range_with_zero_value_returns_low() {
        let mut rng = FixedRng::new(0.0);

        let val = rng.gen_range(10, 20);

        assert_eq!(val, 10);
    }

    #[test]
    fn fixed_gen_range_with_narrow_range_returns_low() {
        let mut rng = FixedRng::new(0.9);

        let val = rng.gen_range(5, 5);

        assert_eq!(val, 5);
    }

    #[test]
    fn fixed_gen_range_differs_with_different_fixed_values() {
        let mut rng_a = FixedRng::new(0.2);
        let mut rng_b = FixedRng::new(0.8);

        let val_a = rng_a.gen_range(0, 100);
        let val_b = rng_b.gen_range(0, 100);

        assert_eq!(val_a, 20);
        assert_eq!(val_b, 80);
    }

    #[test]
    fn same_seed_chooses_same_weighted_item() {
        let mut rng1 = RealRng::from_seed(30);
        let mut rng2 = RealRng::from_seed(30);
        let items = [
            ItemWeighted {
                name: "A",
                weight: 1.0,
            },
            ItemWeighted {
                name: "B",
                weight: 2.0,
            },
            ItemWeighted {
                name: "C",
                weight: 3.0,
            },
        ];

        let a = choose_item_weighted(&mut rng1, &items).unwrap();
        let b = choose_item_weighted(&mut rng2, &items).unwrap();

        assert_eq!(*a, *b);
    }

    #[test]
    fn choose_item_weighted_selects_different_items_for_different_thresholds() {
        let mut rng1 = FixedRng::new(0.1);
        let mut rng2 = FixedRng::new(0.9);
        let items = [
            ItemWeighted {
                name: "A",
                weight: 1.0,
            },
            ItemWeighted {
                name: "B",
                weight: 2.0,
            },
            ItemWeighted {
                name: "C",
                weight: 3.0,
            },
        ];

        let a = choose_item_weighted(&mut rng1, &items).unwrap();
        let b = choose_item_weighted(&mut rng2, &items).unwrap();

        assert_eq!(*a, "A");
        assert_eq!(*b, "C");
    }

    #[test]
    fn real_choose_item_weighted_returns_error_for_empty_list() {
        let mut rng = RealRng::from_seed(42);
        let items: [ItemWeighted<i32>; 0] = [];

        let result = choose_item_weighted(&mut rng, &items);

        assert!(matches!(result, Err(AppError::NotFound(_))));
    }

    #[test]
    fn real_choose_item_weighted_returns_item_from_single_element_list() {
        let mut rng = RealRng::from_seed(7);
        let items = [ItemWeighted {
            name: "only",
            weight: 1.0,
        }];

        let chosen = choose_item_weighted(&mut rng, &items).unwrap();

        assert_eq!(*chosen, "only");
    }

    #[test]
    fn fixed_choose_item_weighted_selects_item_based_on_fixed_value() {
        let mut rng = FixedRng::new(0.5);
        let items = [
            ItemWeighted {
                name: "first",
                weight: 2.0,
            },
            ItemWeighted {
                name: "second",
                weight: 3.0,
            },
            ItemWeighted {
                name: "third",
                weight: 5.0,
            },
        ];

        let chosen = choose_item_weighted(&mut rng, &items).unwrap();

        // total weight = 10, target = 0.5 * 10 = 5.0
        // cumulative: first=2 (<5), second=5 (>=5) -> picks second
        assert_eq!(*chosen, "second");
    }

    #[test]
    fn fixed_choose_item_weighted_with_zero_value_selects_first() {
        let mut rng = FixedRng::new(0.0);
        let items = [
            ItemWeighted {
                name: "head",
                weight: 5.0,
            },
            ItemWeighted {
                name: "tail",
                weight: 5.0,
            },
        ];

        let chosen = choose_item_weighted(&mut rng, &items).unwrap();

        // target = 0.0, first item's cumulative = 5.0 >= 0.0
        assert_eq!(*chosen, "head");
    }

    #[test]
    fn fixed_choose_item_weighted_returns_error_for_empty_list() {
        let mut rng = FixedRng::new(0.5);
        let items: [ItemWeighted<i32>; 0] = [];

        let result = choose_item_weighted(&mut rng, &items);

        assert!(matches!(result, Err(AppError::NotFound(_))));
    }

    #[test]
    fn fixed_choose_item_weighted_selects_correctly_with_uneven_weights() {
        let mut rng = FixedRng::new(0.15);
        let items = [
            ItemWeighted {
                name: "light",
                weight: 1.0,
            },
            ItemWeighted {
                name: "heavy",
                weight: 9.0,
            },
        ];

        let chosen = choose_item_weighted(&mut rng, &items).unwrap();

        // total = 10, target = 0.15 * 10 = 1.5
        // first cumulative = 1.0 < 1.5, second cumulative = 10.0 >= 1.5 -> picks second
        assert_eq!(*chosen, "heavy");
    }

    #[test]
    fn real_normal_delegates_to_normal_random() {
        let mut rng = RealRng::from_seed(10);
        let mut rng_direct = RealRng::from_seed(10);

        let param = NormalParam {
            mean: 5.0,
            std_dev: 2.0,
            skew: 0.0,
            coefficient: 1.0,
            offset: 0.0,
        };
        let via_normal = rng.normal(param);
        let via_direct = rng_direct.normal_random(5.0, 2.0, 0.0, 1.0, 0.0);

        assert_eq!(via_normal, via_direct);
    }

    #[test]
    fn real_normal_with_skew_matches_direct_call() {
        let mut rng = RealRng::from_seed(20);
        let mut rng_direct = RealRng::from_seed(20);

        let param = NormalParam {
            mean: 10.0,
            std_dev: 3.0,
            skew: 1.5,
            coefficient: 2.0,
            offset: 1.0,
        };
        let via_normal = rng.normal(param);
        let via_direct = rng_direct.normal_random(10.0, 3.0, 1.5, 2.0, 1.0);

        assert_eq!(via_normal, via_direct);
    }

    #[test]
    fn real_gamma_delegates_to_gamma_random() {
        let mut rng = RealRng::from_seed(15);
        let mut rng_direct = RealRng::from_seed(15);

        let param = GammaParam {
            shape: 2.0,
            scale: 3.0,
            offset: 1.0,
        };
        let via_gamma = rng.gamma(param);
        let via_direct = rng_direct.gamma_random(2.0, 3.0, 1.0);

        assert_eq!(via_gamma, via_direct);
    }

    #[test]
    fn real_gamma_with_different_params_matches_direct_call() {
        let mut rng = RealRng::from_seed(25);
        let mut rng_direct = RealRng::from_seed(25);

        let param = GammaParam {
            shape: 0.5,
            scale: 1.5,
            offset: -2.0,
        };
        let via_gamma = rng.gamma(param);
        let via_direct = rng_direct.gamma_random(0.5, 1.5, -2.0);

        assert_eq!(via_gamma, via_direct);
    }

    #[test]
    fn fixed_normal_delegates_to_normal_random() {
        let mut rng = FixedRng::new(0.3);

        let param = NormalParam {
            mean: 5.0,
            std_dev: 2.0,
            skew: 0.0,
            coefficient: 3.0,
            offset: 1.0,
        };
        let result = rng.normal(param);

        assert_eq!(result, 5.0 * 3.0 + 1.0);
    }

    #[test]
    fn fixed_normal_with_skew_matches_direct_call() {
        let mut rng = FixedRng::new(0.3);

        let param = NormalParam {
            mean: 5.0,
            std_dev: 2.0,
            skew: 2.0,
            coefficient: 3.0,
            offset: 1.0,
        };
        let result = rng.normal(param);

        assert_eq!(result, 5.0 + 2.0 * 5.0_f64.abs() * 3.0 + 1.0);
    }

    #[test]
    fn fixed_gamma_delegates_to_gamma_random() {
        let mut rng = FixedRng::new(0.3);

        let param = GammaParam {
            shape: 2.0,
            scale: 3.0,
            offset: 1.0,
        };
        let result = rng.gamma(param);

        assert_eq!(result, 2.0 * 3.0 + 1.0);
    }

    #[test]
    fn fixed_gamma_with_different_params() {
        let mut rng = FixedRng::new(0.3);

        let param = GammaParam {
            shape: 0.5,
            scale: 1.5,
            offset: -2.0,
        };
        let result = rng.gamma(param);

        assert_eq!(result, 0.5 * 1.5 + (-2.0));
    }

    #[test]
    fn real_choose_item_if_exists_filters_by_weight() {
        let mut rng = RealRng::from_seed(5);
        let items = [
            ItemWeighted {
                name: "low",
                weight: 0.1,
            },
            ItemWeighted {
                name: "mid",
                weight: 0.5,
            },
            ItemWeighted {
                name: "high",
                weight: 0.9,
            },
        ];

        let result = choose_item_if_exists(&mut rng, &items).unwrap();

        // With seed 5, the random draws < 0.1, < 0.5, < 0.9 determine inclusion
        assert!(!result.is_empty());
        assert!(result.len() <= items.len());
    }

    #[test]
    fn real_choose_item_if_exists_returns_empty_for_empty_list() {
        let mut rng = RealRng::from_seed(5);
        let items: [ItemWeighted<i32>; 0] = [];

        let result = choose_item_if_exists(&mut rng, &items).unwrap();

        assert!(result.is_empty());
    }

    #[test]
    fn fixed_choose_item_if_exists_includes_above_threshold() {
        let mut rng = FixedRng::new(0.5);
        let items = [
            ItemWeighted {
                name: "a",
                weight: 0.3,
            },
            ItemWeighted {
                name: "b",
                weight: 0.5,
            },
            ItemWeighted {
                name: "c",
                weight: 0.8,
            },
        ];

        let result = choose_item_if_exists(&mut rng, &items).unwrap();

        // weight 0.3 <= 0.5 -> excluded, weight 0.5 <= 0.5 -> excluded, weight 0.8 > 0.5 -> included
        assert_eq!(result, vec!["c"]);
    }

    #[test]
    fn fixed_choose_item_if_exists_with_full_threshold_includes_none() {
        let mut rng = FixedRng::new(1.0);
        let items = [
            ItemWeighted {
                name: "a",
                weight: 0.3,
            },
            ItemWeighted {
                name: "b",
                weight: 0.9,
            },
        ];

        let result = choose_item_if_exists(&mut rng, &items).unwrap();

        // Only weights greater than 1.0 pass (none do)
        assert!(result.is_empty());
    }

    #[test]
    fn fixed_choose_item_if_exists_with_zero_threshold_includes_all() {
        let mut rng = FixedRng::new(0.0);
        let items = [
            ItemWeighted {
                name: "x",
                weight: 0.1,
            },
            ItemWeighted {
                name: "y",
                weight: 0.5,
            },
        ];

        let result = choose_item_if_exists(&mut rng, &items).unwrap();

        // All positive weights pass
        assert_eq!(result, vec!["x", "y"]);
    }

    #[test]
    fn fixed_choose_item_if_exists_returns_empty_for_empty_list() {
        let mut rng = FixedRng::new(0.5);
        let items: [ItemWeighted<i32>; 0] = [];

        let result = choose_item_if_exists(&mut rng, &items).unwrap();

        assert!(result.is_empty());
    }
}
