use crate::domain::resolver::running_resolver::RunnersOnBase;
use crate::domain::util::{FIRST, SECOND, THIRD};

pub struct HitAdvanceModel {
    pub second_scores_on_single: f64,
    pub first_to_third_on_single: f64,
    pub first_scores_on_double: f64,
}

impl HitAdvanceModel {
    pub const DEFAULT_ADVANCE_PROBS: Self = Self {
        second_scores_on_single: 0.60,
        first_to_third_on_single: 0.30,
        first_scores_on_double: 0.55,
    };

    pub fn costs(
        &self,
        re: &RunExpectancyTable,
        runners: &RunnersOnBase,
        outs: u8,
    ) -> Option<HitCosts> {
        let before = runners.base_mask();

        Some(HitCosts {
            single: self.single_cost(re, before, outs)?,
            double: self.double_cost(re, before, outs)?,
            triple: self.triple_cost(re, before, outs)?,
            home_run: self.home_run_cost(re, before, outs)?,
        })
    }

    fn single_cost(&self, re: &RunExpectancyTable, before: u8, outs: u8) -> Option<f64> {
        let on_first = before & FIRST != 0;
        let on_second = before & SECOND != 0;
        let on_third = before & THIRD != 0;
        let mut expected = 0.0;

        // Runner on second scores / stops at third
        for second_scores in [false, true] {
            let p_second = match (on_second, second_scores) {
                (false, false) => 1.0,
                (false, true) => 0.0,
                (true, false) => 1.0 - self.second_scores_on_single,
                (true, true) => self.second_scores_on_single,
            };

            // Consider the runner on first advancing first → third only when third base is free
            let can_first_take_third = on_first && (!on_second || second_scores);

            for first_takes_third in [false, true] {
                let p_first = match (can_first_take_third, first_takes_third) {
                    (false, false) => 1.0,
                    (false, true) => 0.0,
                    (true, false) => 1.0 - self.first_to_third_on_single,
                    (true, true) => self.first_to_third_on_single,
                };

                let probability = p_second * p_first;
                if probability == 0.0 {
                    continue;
                }

                let runs = u8::from(on_third) + u8::from(on_second && second_scores);

                let after = FIRST // batter runner
                    | if on_first && !first_takes_third {
                        SECOND
                    } else {
                        0
                    }
                    | if (on_second && !second_scores)
                        || first_takes_third
                    {
                        THIRD
                    } else {
                        0
                    };

                expected += probability * re.play_cost_runs(before, outs, runs, after)?;
            }
        }

        Some(expected)
    }

    fn double_cost(&self, re: &RunExpectancyTable, before: u8, outs: u8) -> Option<f64> {
        let on_first = before & FIRST != 0;
        let on_second = before & SECOND != 0;
        let on_third = before & THIRD != 0;
        let guaranteed_runs = u8::from(on_second) + u8::from(on_third);

        if !on_first {
            // Batter goes to second. Runners on second and third score
            return re.play_cost_runs(before, outs, guaranteed_runs, SECOND);
        }

        let scores = re.play_cost_runs(before, outs, guaranteed_runs + 1, SECOND)?;
        let stops_at_third = re.play_cost_runs(before, outs, guaranteed_runs, SECOND | THIRD)?;

        Some(
            self.first_scores_on_double * scores
                + (1.0 - self.first_scores_on_double) * stops_at_third,
        )
    }

    fn triple_cost(&self, re: &RunExpectancyTable, before: u8, outs: u8) -> Option<f64> {
        // In the initial version, all existing runners score and the batter goes to third
        re.play_cost_runs(before, outs, before.count_ones() as u8, THIRD)
    }

    fn home_run_cost(&self, re: &RunExpectancyTable, before: u8, outs: u8) -> Option<f64> {
        // The existing runners and the batter score, leaving the bases empty
        re.play_cost_runs(before, outs, before.count_ones() as u8 + 1, 0)
    }
}

pub struct HitCosts {
    pub single: f64,
    pub double: f64,
    pub triple: f64,
    pub home_run: f64,
}

pub struct RunExpectancyTable {
    // Run expectancy, Bitmask
    values: [[f64; 8]; 3],
}

impl RunExpectancyTable {
    pub const MLB_2021_2024: Self = Self {
        values: [
            // 0 out : 000, 001, 010, 011, 100, 101, 110, 111
            [0.50, 0.90, 1.14, 1.51, 1.37, 1.82, 2.04, 2.38],
            // 1 out : 000, 001, 010, 011, 100, 101, 110, 111
            [0.27, 0.54, 0.71, 0.94, 0.98, 1.19, 1.41, 1.63],
            // 2 outs : 000, 001, 010, 011, 100, 101, 110, 111
            [0.10, 0.23, 0.33, 0.46, 0.38, 0.51, 0.57, 0.82],
        ],
    };

    pub fn get(&self, outs: u8, bases: u8) -> Option<f64> {
        self.values
            .get(usize::from(outs))?
            .get(usize::from(bases))
            .copied()
    }

    pub fn walk_cost_runs(&self, runners: &RunnersOnBase, outs: u8) -> Option<f64> {
        let before = runners.base_mask();
        let (after, runs_scored) = runners.bases_after_walk();

        Some(f64::from(runs_scored) + self.get(outs, after)? - self.get(outs, before)?)
    }

    pub fn play_cost_runs(&self, before: u8, outs: u8, runs_scored: u8, after: u8) -> Option<f64> {
        Some(f64::from(runs_scored) + self.get(outs, after)? - self.get(outs, before)?)
    }
}

pub struct ExtraBaseHitProbabilities {
    pub double: f64,
    pub triple: f64,
    pub home_run: f64,
}

pub const DEFAULT_XBH_PROBS: ExtraBaseHitProbabilities = ExtraBaseHitProbabilities {
    double: 0.043,
    triple: 0.004,
    home_run: 0.030,
};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::game_state::ActiveRunner;
    use crate::domain::shared::player::RunningSkills;

    fn assert_near(actual: f64, expected: f64) {
        let epsilon = 1e-9;
        assert!(
            (actual - expected).abs() < epsilon,
            "expected {actual} to be near {expected}"
        );
    }

    fn runner(id: i64) -> ActiveRunner {
        ActiveRunner {
            id,
            skills: RunningSkills {
                speed: 8.0,
                lead_distance: 0.0,
                start_reaction: 0.1,
            },
        }
    }

    fn runners(
        runner_1st: Option<ActiveRunner>,
        runner_2nd: Option<ActiveRunner>,
        runner_3rd: Option<ActiveRunner>,
    ) -> RunnersOnBase {
        RunnersOnBase {
            batter_runner: None,
            runner_1st,
            runner_2nd,
            runner_3rd,
        }
    }

    #[test]
    fn get_returns_run_expectancy_by_outs_and_base_mask() {
        let table = RunExpectancyTable::MLB_2021_2024;

        assert_near(table.get(0, 0b000).unwrap(), 0.50);
        assert_near(table.get(1, 0b101).unwrap(), 1.19);
        assert_near(table.get(2, 0b111).unwrap(), 0.82);
        assert_eq!(table.get(3, 0b000), None);
        assert_eq!(table.get(0, 0b1000), None);
    }

    #[test]
    fn walk_cost_runs_returns_expected_change_in_run_expectancy() {
        let table = RunExpectancyTable::MLB_2021_2024;
        let runner = Some(runner(1));
        let cases = [
            (runners(None, None, None), 0, 0.40),
            (runners(runner, None, None), 0, 0.61),
            (runners(None, runner, runner), 1, 0.22),
            (runners(runner, runner, runner), 2, 1.00),
        ];

        for (runners, outs, expected) in cases {
            assert_near(table.walk_cost_runs(&runners, outs).unwrap(), expected);
        }

        assert_eq!(table.walk_cost_runs(&runners(None, None, None), 3), None);
    }

    fn runners_from_mask(mask: u8) -> RunnersOnBase {
        runners(
            (mask & FIRST != 0).then(|| runner(1)),
            (mask & SECOND != 0).then(|| runner(2)),
            (mask & THIRD != 0).then(|| runner(3)),
        )
    }

    #[test]
    fn play_cost_runs_accounts_for_scoring_and_changed_base_state() {
        let table = RunExpectancyTable::MLB_2021_2024;
        let cases = [
            (0, 0, 0, FIRST, 0.40),
            (FIRST, 1, 0, SECOND, 0.17),
            (THIRD, 2, 1, FIRST, 0.85),
            (FIRST | SECOND | THIRD, 0, 4, 0, 2.12),
            (SECOND, 1, 0, SECOND, 0.0),
            (THIRD, 0, 0, FIRST, -0.47),
        ];

        for (before, outs, runs, after, expected) in cases {
            assert_near(
                table.play_cost_runs(before, outs, runs, after).unwrap(),
                expected,
            );
        }
    }

    #[test]
    fn play_cost_runs_rejects_invalid_outs_and_base_masks() {
        let table = RunExpectancyTable::MLB_2021_2024;
        for invalid in [3, u8::MAX] {
            assert_eq!(table.play_cost_runs(0, invalid, 1, FIRST), None);
        }
        for invalid in [8, u8::MAX] {
            assert_eq!(table.play_cost_runs(invalid, 0, 1, FIRST), None);
            assert_eq!(table.play_cost_runs(FIRST, 0, 1, invalid), None);
        }
    }

    #[test]
    fn default_hit_costs_cover_every_base_state_and_out_count() {
        let table = RunExpectancyTable::MLB_2021_2024;
        let model = HitAdvanceModel::DEFAULT_ADVANCE_PROBS;
        // Each outcome is (probability, runs scored, bases after the hit).
        // In particular, a runner holding at third blocks first-to-third advancement.
        let singles: [&[(f64, u8, u8)]; 8] = [
            &[(1.0, 0, FIRST)],
            &[(0.70, 0, FIRST | SECOND), (0.30, 0, FIRST | THIRD)],
            &[(0.40, 0, FIRST | THIRD), (0.60, 1, FIRST)],
            &[
                (0.40, 0, 7),
                (0.42, 1, FIRST | SECOND),
                (0.18, 1, FIRST | THIRD),
            ],
            &[(1.0, 1, FIRST)],
            &[(0.70, 1, FIRST | SECOND), (0.30, 1, FIRST | THIRD)],
            &[(0.40, 1, FIRST | THIRD), (0.60, 2, FIRST)],
            &[
                (0.40, 1, 7),
                (0.42, 2, FIRST | SECOND),
                (0.18, 2, FIRST | THIRD),
            ],
        ];
        let doubles: [&[(f64, u8, u8)]; 8] = [
            &[(1.0, 0, SECOND)],
            &[(0.45, 0, SECOND | THIRD), (0.55, 1, SECOND)],
            &[(1.0, 1, SECOND)],
            &[(0.45, 1, SECOND | THIRD), (0.55, 2, SECOND)],
            &[(1.0, 1, SECOND)],
            &[(0.45, 1, SECOND | THIRD), (0.55, 2, SECOND)],
            &[(1.0, 2, SECOND)],
            &[(0.45, 2, SECOND | THIRD), (0.55, 3, SECOND)],
        ];
        let runner_counts = [0, 1, 1, 2, 1, 2, 2, 3];

        for outs in 0..3 {
            for mask in 0..8 {
                let costs = model.costs(&table, &runners_from_mask(mask), outs).unwrap();
                let before = table.get(outs, mask).unwrap();
                let expected = |outcomes: &[(f64, u8, u8)]| -> f64 {
                    outcomes
                        .iter()
                        .map(|&(probability, runs, after)| {
                            probability * (f64::from(runs) + table.get(outs, after).unwrap())
                        })
                        .sum::<f64>()
                        - before
                };
                assert_near(costs.single, expected(singles[mask as usize]));
                assert_near(costs.double, expected(doubles[mask as usize]));
                let runs = f64::from(runner_counts[mask as usize]);
                assert_near(
                    costs.triple,
                    runs + table.get(outs, THIRD).unwrap() - before,
                );
                assert_near(
                    costs.home_run,
                    runs + 1.0 + table.get(outs, 0).unwrap() - before,
                );
            }
        }
    }

    #[test]
    fn hit_advance_probability_boundaries_control_runner_destinations() {
        let table = RunExpectancyTable::MLB_2021_2024;
        let loaded = runners_from_mask(7);
        for (second_scores, first_to_third, first_scores, single, double) in [
            (0.0, 0.0, 0.0, 1.0, 1.66),
            (0.0, 1.0, 1.0, 1.0, 1.76), // Third is blocked on the single.
            (1.0, 0.0, 0.0, 1.13, 1.66),
            (1.0, 1.0, 1.0, 1.44, 1.76),
        ] {
            let model = HitAdvanceModel {
                second_scores_on_single: second_scores,
                first_to_third_on_single: first_to_third,
                first_scores_on_double: first_scores,
            };
            let costs = model.costs(&table, &loaded, 0).unwrap();
            assert_near(costs.single, single);
            assert_near(costs.double, double);
            assert_near(costs.triple, 1.99);
            assert_near(costs.home_run, 2.12);
        }
    }

    #[test]
    fn hit_costs_reject_invalid_out_counts() {
        for mask in 0..8 {
            for outs in [3, u8::MAX] {
                assert!(
                    HitAdvanceModel::DEFAULT_ADVANCE_PROBS
                        .costs(
                            &RunExpectancyTable::MLB_2021_2024,
                            &runners_from_mask(mask),
                            outs,
                        )
                        .is_none()
                );
            }
        }
    }

    #[test]
    fn default_extra_base_hit_probabilities_match_baseline_rates() {
        let probabilities = DEFAULT_XBH_PROBS;
        assert_near(probabilities.double, 0.043);
        assert_near(probabilities.triple, 0.004);
        assert_near(probabilities.home_run, 0.030);
        // These are unconditional rates, so they do not sum to one.
        assert_near(
            probabilities.double + probabilities.triple + probabilities.home_run,
            0.077,
        );
    }
}
