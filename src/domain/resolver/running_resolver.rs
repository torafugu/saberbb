use super::fielding_resolver::{
    DefensePlayResult, DoublePlayDefensePlayResult, PlayType, StealDefensePlayResult,
};
use crate::domain::shared::game::{BASE_DISTANCE, BattingResult};
use crate::domain::shared::game_state::{GameError, Ruling};
use crate::domain::shared::player::RL;
use crate::domain::shared::stadium::Base;

pub const ACCELERATION_LAG_TO_FIRST_BASE: f64 = 0.5;
pub const ACCELERATION_LAG_AFTER_FIRST_BASE: f64 = 0.2;
pub const ACCELERATION_LAG_FROM_FIRST_TO_SECOND_BASE: f64 = 0.7;
pub const ACCELERATION_LAG_FROM_FIRST_TO_THIRD_BASE: f64 = 0.9;

#[derive(Clone, Copy, Debug)]
pub struct Runner {
    pub speed: f64,         // Base running speed (m/s) e.g. 7.7
    pub lead_distance: f64, // Current lead distance (m), valid when current_base > 0
    pub target_base: Option<Base>,
}

#[derive(Clone, Debug)]
pub struct StealRunnerAdvanceResult {
    pub defense_time: f64,
    pub runner_time: f64,
    pub time_difference: f64,
    pub throw_target_base: Base,
    pub ruling: Ruling,
}

#[derive(Clone, Debug)]
pub struct DoublePlayRunnerAdvanceResult {
    pub defense_time: f64,
    pub runner_time: f64,
    pub time_difference: f64,
    pub throw_target_base: Base,
    pub ruling: Ruling,
    pub unsaved_runners: RunnersUnsaved,
}

#[derive(Clone, Debug)]
pub struct RunnerAdvanceResult {
    pub defense_time: f64,
    pub runner_time: f64,
    pub time_difference: f64,
    pub throw_target_base: Base,
    pub play_type: PlayType,
    pub ruling: Ruling,
    pub batting_result: BattingResult,
    pub runs_scored: u16,
    pub unsaved_runners: RunnersUnsaved,
}

// TODO: runner_1st, runner_2nd, and runner_3rd should be used by running strategy.
#[derive(Clone, Debug)]
pub struct RunningPlan {
    pub batter_runner: Base,
    pub runner_1st: Base,
    pub runner_2nd: Base,
    pub runner_3rd: Base,
}
impl RunningPlan {
    // TODO: Consider Hit and Run
    fn set(time_to_field: f64, batter_to_first_time: f64, batter_to_second_time: f64) -> Self {
        let (batter_runner, runner_1st, runner_2nd) = if time_to_field > batter_to_second_time {
            // Triple
            (Base::Third, Base::Home, Base::Home)
        } else if time_to_field > batter_to_first_time {
            // Double
            (Base::Second, Base::Home, Base::Home)
        } else {
            // Single
            // CONSTRAINT: Runner is not already started, i.e. base steal or hit and run
            (Base::First, Base::Second, Base::Home)
        };

        Self {
            batter_runner: batter_runner,
            runner_1st: runner_1st,
            runner_2nd: runner_2nd,
            runner_3rd: Base::Home,
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RunnersUnsaved {
    pub runner_1st: Option<Runner>,
    pub runner_2nd: Option<Runner>,
    pub runner_3rd: Option<Runner>,
}
impl RunnersUnsaved {
    fn put(&mut self, base: Base, runner: Runner) {
        match base {
            Base::First => self.runner_1st = Some(runner),
            Base::Second => self.runner_2nd = Some(runner),
            Base::Third => self.runner_3rd = Some(runner),
            _ => {}
        };
    }

    fn put_if_some(&mut self, base: Base, runner: Option<Runner>) {
        if runner.is_some() {
            match base {
                Base::First => self.runner_1st = runner,
                Base::Second => self.runner_2nd = runner,
                Base::Third => self.runner_3rd = runner,
                _ => {}
            }
        };
    }

    fn score_if_some(runner: Option<Runner>) -> u16 {
        if runner.is_some() { 1 } else { 0 }
    }
}

fn advance_count(from: Base, to: Base) -> Result<u8, GameError> {
    let count = match (from, to) {
        (Base::Home, Base::First) => 1,
        (Base::Home, Base::Second) => 2,
        (Base::Home, Base::Third) => 3,
        // CONSTRAINT:  inside-the-park homerun is not supported
        // (Base::Home, Base::Home) => 4,
        (Base::First, Base::Second) => 1,
        (Base::First, Base::Third) => 2,
        (Base::First, Base::Home) => 3,
        (Base::Second, Base::Third) => 1,
        (Base::Second, Base::Home) => 2,
        (Base::Third, Base::Home) => 1,
        _ => {
            return Err(GameError::UnsupportedPath);
        }
    };
    Ok(count)
}

fn judge(defense_time: f64, runner_time: f64) -> (Ruling, f64) {
    let diff = defense_time - runner_time;
    let ruling = if diff > 0.0 {
        Ruling::Safe
    } else {
        Ruling::Out
    };
    (ruling, diff)
}

#[derive(Clone, Debug, Default)]
pub struct RunnersOnBase {
    pub batting_side: Option<RL>,
    pub batter_runner: Option<Runner>,
    pub runner_1st: Option<Runner>,
    pub runner_2nd: Option<Runner>,
    pub runner_3rd: Option<Runner>,
}
impl RunnersOnBase {
    fn empty(&mut self) {
        self.batting_side = None;
        self.batter_runner = None;
        self.runner_1st = None;
        self.runner_2nd = None;
        self.runner_3rd = None;
    }

    pub fn has_runner_on(&self, base: Base) -> bool {
        match base {
            Base::First => self.runner_1st.is_some(),
            Base::Second => self.runner_2nd.is_some(),
            Base::Third => self.runner_3rd.is_some(),
            Base::Home => false,
        }
    }

    pub fn is_loaded(&self) -> bool {
        self.runner_1st.is_some() && self.runner_2nd.is_some() && self.runner_3rd.is_some()
    }

    pub fn has_first_and_second(&self) -> bool {
        self.runner_1st.is_some() && self.runner_2nd.is_some()
    }

    pub fn has_second_and_third(&self) -> bool {
        self.runner_2nd.is_some() && self.runner_3rd.is_some()
    }

    pub fn has_first_and_third(&self) -> bool {
        self.runner_1st.is_some() && self.runner_3rd.is_some()
    }

    fn runner_on(&self, base: Base) -> Result<Runner, GameError> {
        match base {
            Base::Home => {
                if let Some(runner) = self.batter_runner {
                    Ok(runner)
                } else {
                    return Err(GameError::BatterRunner);
                }
            }
            Base::First => {
                if let Some(runner) = self.runner_1st {
                    Ok(runner)
                } else {
                    return Err(GameError::Runner1st);
                }
            }
            Base::Second => {
                if let Some(runner) = self.runner_2nd {
                    Ok(runner)
                } else {
                    return Err(GameError::Runner2nd);
                }
            }
            Base::Third => {
                if let Some(runner) = self.runner_3rd {
                    Ok(runner)
                } else {
                    return Err(GameError::Runner3rd);
                }
            }
        }
    }

    fn batter_runner_time_to(&self, to_base: Base, with_lag: bool) -> Result<f64, GameError> {
        let batter_runner = self.runner_on(Base::Home)?;
        let base_count = advance_count(Base::Home, to_base)?;

        let right_batter_penalty_distance = if let Some(batting_side) = &self.batting_side {
            if *batting_side == RL::Right { 2.0 } else { 0.0 }
        } else {
            return Err(GameError::BatterRunner);
        };

        let lag = if with_lag {
            ACCELERATION_LAG_TO_FIRST_BASE
                + ACCELERATION_LAG_AFTER_FIRST_BASE * (base_count - 1) as f64
        } else {
            0.0
        };

        let batter_runner_time = ((BASE_DISTANCE * base_count as f64)
            + right_batter_penalty_distance)
            / batter_runner.speed
            + lag;

        Ok(batter_runner_time)
    }

    fn runner_advance_time(runner: Runner, base_count: u8, with_lag: bool) -> f64 {
        let lag = if with_lag {
            ACCELERATION_LAG_AFTER_FIRST_BASE * base_count as f64
        } else {
            0.0
        };

        ((BASE_DISTANCE * base_count as f64) - runner.lead_distance) / runner.speed + lag
    }

    pub fn total_runner_time(&self, from_base: Base, to_base: Base) -> Result<f64, GameError> {
        if from_base == to_base {
            return Err(GameError::SameTargetBase);
        }

        if from_base == Base::Home {
            return self.batter_runner_time_to(to_base, true);
        }

        let runner = self.runner_on(from_base)?;
        let base_count = advance_count(from_base, to_base)?;

        Ok(Self::runner_advance_time(runner, base_count, true))
    }

    pub fn after_homerun(&mut self) -> u16 {
        let mut runs_scored: u16 = 1;
        runs_scored += RunnersUnsaved::score_if_some(self.runner_1st);
        runs_scored += RunnersUnsaved::score_if_some(self.runner_2nd);
        runs_scored += RunnersUnsaved::score_if_some(self.runner_3rd);
        self.empty(); // commit_unsaved_runners is not needed

        runs_scored
    }

    pub fn commit_unsaved_runners(&mut self, unsaved_runners: RunnersUnsaved) {
        self.runner_1st = unsaved_runners.runner_1st;
        self.runner_2nd = unsaved_runners.runner_2nd;
        self.runner_3rd = unsaved_runners.runner_3rd;
    }

    pub fn after_infield_grounder(
        &self,
        defense_play_result: DefensePlayResult,
    ) -> Result<RunnerAdvanceResult, GameError> {
        let runner_time;
        let time_difference;
        let mut unsaved_runners = RunnersUnsaved::default();
        let ruling;
        let mut batting_result = BattingResult::Out;
        let mut runs_scored: u16 = 0;

        match defense_play_result.throw_target_base {
            Base::Home => {
                unsaved_runners.put_if_some(Base::First, self.batter_runner);
                unsaved_runners.put_if_some(Base::Second, self.runner_1st);
                unsaved_runners.put_if_some(Base::Third, self.runner_2nd);

                runner_time = self.total_runner_time(Base::Third, Base::Home)?;
                (ruling, time_difference) = judge(defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Safe {
                    runs_scored += 1;
                    batting_result = BattingResult::FieldersChoice;
                };
            }
            Base::Third => {
                unsaved_runners.put_if_some(Base::First, self.batter_runner);
                unsaved_runners.put_if_some(Base::Second, self.runner_1st);

                runner_time = self.total_runner_time(Base::Second, Base::Third)?;
                (ruling, time_difference) = judge(defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Safe {
                    unsaved_runners.put_if_some(Base::Third, self.runner_2nd);
                    batting_result = BattingResult::FieldersChoice;
                };

                runs_scored += RunnersUnsaved::score_if_some(self.runner_3rd);
            }
            Base::Second => {
                unsaved_runners.put_if_some(Base::First, self.batter_runner);

                runner_time = self.total_runner_time(Base::First, Base::Second)?;
                (ruling, time_difference) = judge(defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Safe {
                    unsaved_runners.runner_2nd = self.runner_1st;
                    batting_result = BattingResult::FieldersChoice;
                };

                unsaved_runners.put_if_some(Base::Third, self.runner_2nd);
                runs_scored += RunnersUnsaved::score_if_some(self.runner_3rd);
            }
            Base::First => {
                runner_time = self.batter_runner_time_to(Base::First, true)?;
                (ruling, time_difference) = judge(defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Safe {
                    unsaved_runners.runner_1st = self.batter_runner;
                    batting_result = BattingResult::Single;
                };

                unsaved_runners.put_if_some(Base::Second, self.runner_1st);
                unsaved_runners.put_if_some(Base::Third, self.runner_2nd);
                runs_scored += RunnersUnsaved::score_if_some(self.runner_3rd);
            }
        }

        let runner_advance_result = RunnerAdvanceResult {
            defense_time: defense_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: defense_play_result.throw_target_base,
            play_type: defense_play_result.play_type,
            ruling: ruling,
            batting_result: batting_result,
            runs_scored: runs_scored,
            unsaved_runners: unsaved_runners,
        };
        Ok(runner_advance_result)
    }

    fn resolve_triple_attempt(
        &self,
        throw_target_base: Base,
        defense_time: f64,
    ) -> Result<(f64, f64, Ruling, Option<Base>, BattingResult), GameError> {
        let runner_time = match throw_target_base {
            Base::Home => self.total_runner_time(Base::First, Base::Home)?,
            Base::Third => self.batter_runner_time_to(Base::Third, true)?,
            _ => 0.0,
        };

        let (ruling, time_difference) = judge(defense_time, runner_time);
        let (batting_result, retired_runner) = if ruling == Ruling::Out {
            if throw_target_base == Base::Third {
                (BattingResult::Double, Some(Base::Home))
            } else {
                (BattingResult::Triple, Some(Base::First))
            }
        } else {
            (BattingResult::Triple, None)
        };

        Ok((
            runner_time,
            time_difference,
            ruling,
            retired_runner,
            batting_result,
        ))
    }

    fn resolve_double_attempt(
        &self,
        throw_target_base: Base,
        defense_time: f64,
    ) -> Result<(f64, f64, Ruling, Option<Base>, BattingResult), GameError> {
        let mut runner_time = 0.0;
        let mut time_difference = 0.0;
        let mut ruling = Ruling::Safe;
        let mut retired_runner = None;

        if throw_target_base == Base::Home {
            if self.has_runner_on(Base::Second) {
                runner_time = self.total_runner_time(Base::Second, Base::Home)?;
                (ruling, time_difference) = judge(defense_time, runner_time);
                if ruling == Ruling::Out {
                    retired_runner = Some(Base::Second);
                }
            }

            if self.has_runner_on(Base::First) {
                if ruling == Ruling::Safe {
                    runner_time = self.total_runner_time(Base::First, Base::Home)?;
                    (ruling, time_difference) = judge(defense_time, runner_time);
                    if ruling == Ruling::Out {
                        retired_runner = Some(Base::First);
                    }
                }
            }

            return Ok((
                runner_time,
                time_difference,
                ruling,
                retired_runner,
                BattingResult::Double,
            ));
        }

        (runner_time, retired_runner) = match throw_target_base {
            Base::Third => (
                self.total_runner_time(Base::First, Base::Third)?,
                Some(Base::First),
            ),
            Base::Second => (
                self.batter_runner_time_to(Base::Second, true)?,
                Some(Base::Home),
            ),
            _ => (0.0, None),
        };

        let (ruling, time_difference) = judge(defense_time, runner_time);

        if ruling == Ruling::Safe {
            retired_runner = None;
        };

        let batting_result = if retired_runner == Some(Base::Home) {
            BattingResult::Single
        } else {
            BattingResult::Double
        };

        Ok((
            runner_time,
            time_difference,
            ruling,
            retired_runner,
            batting_result,
        ))
    }

    fn resolve_single_attempt(
        &self,
        throw_target_base: Base,
        defense_time: f64,
    ) -> Result<(f64, f64, Ruling, Option<Base>, BattingResult), GameError> {
        let (runner_time, mut retired_runner) = match throw_target_base {
            Base::Home => (
                self.total_runner_time(Base::Third, Base::Home)?,
                Some(Base::Third),
            ),
            Base::Third => (
                self.total_runner_time(Base::Second, Base::Third)?,
                Some(Base::Second),
            ),
            Base::Second => (
                self.total_runner_time(Base::First, Base::Second)?,
                Some(Base::First),
            ),
            Base::First => (
                self.batter_runner_time_to(Base::First, true)?,
                Some(Base::Home),
            ),
        };

        let (ruling, time_difference) = judge(defense_time, runner_time);

        if ruling == Ruling::Safe {
            retired_runner = None;
        };

        let batting_result = if retired_runner == Some(Base::Home) {
            BattingResult::Out
        } else {
            BattingResult::Single
        };

        Ok((
            runner_time,
            time_difference,
            ruling,
            retired_runner,
            batting_result,
        ))
    }

    fn score_for_existing_runners(
        &self,
        batter_target_base: Base,
        retired_runner: Option<Base>,
    ) -> Result<u16, GameError> {
        let mut runs_scored: u16 = 0;

        match batter_target_base {
            Base::Third => {
                runs_scored += RunnersUnsaved::score_if_some(self.runner_3rd);
                runs_scored += RunnersUnsaved::score_if_some(self.runner_2nd);
                runs_scored += if retired_runner == Some(Base::First) {
                    0
                } else {
                    RunnersUnsaved::score_if_some(self.runner_1st)
                };
            }
            Base::Second => {
                runs_scored += RunnersUnsaved::score_if_some(self.runner_3rd);
                runs_scored += if retired_runner == Some(Base::Second) {
                    0
                } else if retired_runner == Some(Base::First) {
                    RunnersUnsaved::score_if_some(self.runner_2nd)
                } else {
                    RunnersUnsaved::score_if_some(self.runner_2nd)
                        + RunnersUnsaved::score_if_some(self.runner_1st)
                };
            }
            Base::First => {
                runs_scored += if retired_runner == Some(Base::Third) {
                    0
                } else {
                    RunnersUnsaved::score_if_some(self.runner_3rd)
                };
            }
            _ => {
                // CONSTRAINT: inside-the-park homerun is not supported.
                return Err(GameError::BatterRunnerTargetBase);
            }
        };

        Ok(runs_scored)
    }

    fn build_runner_advance_result(
        &self,
        batter_target_base: Base,
        retired_runner: Option<Base>,
    ) -> Result<RunnersUnsaved, GameError> {
        let mut unsaved_runners = RunnersUnsaved::default();

        match batter_target_base {
            Base::Third => {
                if retired_runner != Some(Base::Home) {
                    unsaved_runners.put_if_some(Base::Third, self.batter_runner);
                };
            }
            Base::Second => {
                if retired_runner != Some(Base::Home) {
                    unsaved_runners.put_if_some(Base::Second, self.batter_runner);

                    // In case threw to home base and second runner was touched out.
                    if retired_runner == Some(Base::Second) {
                        // 3rd runner went home and 1st runner stopped at 3rd base.
                        unsaved_runners.put_if_some(Base::Third, self.runner_1st);
                    };
                };
                // 3rd, 2nd and 1st runners went home in case threw to second base.
            }
            Base::First => {
                // CONSTRAINT: 3rd runner go home whatever the result is.

                // In case threw to second base and first runner was touched out.
                if retired_runner != Some(Base::First) {
                    unsaved_runners.put_if_some(Base::Second, self.runner_1st);
                };

                // In case threw to third base and second runner was touched out.
                if retired_runner != Some(Base::Second) {
                    unsaved_runners.put_if_some(Base::Third, self.runner_2nd);
                };

                if retired_runner != Some(Base::Home) {
                    unsaved_runners.put_if_some(Base::First, self.batter_runner);
                };
            }
            _ => {
                // CONSTRAINT: inside-the-park homerun is not supported.
                return Err(GameError::BatterRunnerTargetBase);
            }
        };

        Ok(unsaved_runners)
    }

    pub fn after_outfield_hit(
        &self,
        defense_play_result: DefensePlayResult,
    ) -> Result<RunnerAdvanceResult, GameError> {
        let batter_to_first_time = self.batter_runner_time_to(Base::First, false)?;
        let batter_to_second_time = self.batter_runner_time_to(Base::Second, false)?;

        let running_plan = RunningPlan::set(
            defense_play_result.time_to_field,
            batter_to_first_time,
            batter_to_second_time,
        );

        // if let Some(target_base) = self.runner_on(Base::Home)?.target_base {
        let (runner_time, time_difference, ruling, retired_runner, batting_result) =
            match running_plan.batter_runner {
                Base::Third => self.resolve_triple_attempt(
                    defense_play_result.throw_target_base,
                    defense_play_result.defense_time,
                )?,
                Base::Second => self.resolve_double_attempt(
                    defense_play_result.throw_target_base,
                    defense_play_result.defense_time,
                )?,
                // throw_target_base is always second base
                // CONSTRAINT: Right Goundout case is not covered now
                Base::First => self.resolve_single_attempt(
                    defense_play_result.throw_target_base,
                    defense_play_result.defense_time,
                )?,
                // CONSTRAINT: inside-the-park homerun is not supported.
                _ => {
                    return Err(GameError::BatterRunnerTargetBase);
                }
            };

        let runs_scored =
            self.score_for_existing_runners(running_plan.batter_runner, retired_runner)?;
        let unsaved_runners =
            self.build_runner_advance_result(running_plan.batter_runner, retired_runner)?;

        let runner_advance_result = RunnerAdvanceResult {
            defense_time: defense_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: defense_play_result.throw_target_base,
            play_type: defense_play_result.play_type,
            ruling: ruling.clone(),
            batting_result: batting_result,
            runs_scored: runs_scored,
            unsaved_runners: unsaved_runners,
        };
        Ok(runner_advance_result)
    }

    pub fn after_tagup(
        &self,
        defense_play_result: DefensePlayResult,
    ) -> Result<RunnerAdvanceResult, GameError> {
        let mut runner_time = 0.0;
        let mut time_difference = 0.0;
        let mut unsaved_runners: RunnersUnsaved = RunnersUnsaved::default();
        let mut ruling = Ruling::Safe;
        let mut runs_scored: u16 = 0;

        match defense_play_result.throw_target_base {
            Base::Home => {
                runner_time = self.total_runner_time(Base::Third, Base::Home)?;
                (ruling, time_difference) = judge(defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Safe {
                    runs_scored += 1;
                };

                // 1st and 2nd runners does not move
                unsaved_runners.runner_2nd = self.runner_2nd;
                unsaved_runners.runner_1st = self.runner_1st;
            }
            Base::Third => {
                // 1st runner does not move
                unsaved_runners.runner_1st = self.runner_1st;

                runner_time = self.total_runner_time(Base::Second, Base::Third)?;
                (ruling, time_difference) = judge(defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Safe {
                    unsaved_runners.put(Base::Third, self.runner_on(Base::Second)?);
                };

                runs_scored += RunnersUnsaved::score_if_some(self.runner_3rd);
            }
            _ => {
                // 1st and 2nd runners does not move
                unsaved_runners.runner_2nd = self.runner_2nd;
                unsaved_runners.runner_1st = self.runner_1st;
                runs_scored += RunnersUnsaved::score_if_some(self.runner_3rd);
            }
        }

        let runner_advance_result = RunnerAdvanceResult {
            defense_time: defense_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: defense_play_result.throw_target_base,
            play_type: defense_play_result.play_type,
            ruling: ruling,
            batting_result: BattingResult::Out,
            runs_scored: runs_scored,
            unsaved_runners: unsaved_runners,
        };
        Ok(runner_advance_result)
    }

    // CONSTRAINT: Temporary runners do not go to the next base.
    pub fn after_double_play(
        &self,
        double_play_defense_play_result: DoublePlayDefensePlayResult,
        previous_unsaved_runners: RunnersUnsaved,
    ) -> Result<DoublePlayRunnerAdvanceResult, GameError> {
        let runner_time;
        let time_difference;
        let mut unsaved_runners: RunnersUnsaved = RunnersUnsaved {
            runner_1st: previous_unsaved_runners.runner_1st,
            runner_2nd: previous_unsaved_runners.runner_2nd,
            runner_3rd: previous_unsaved_runners.runner_3rd,
        };
        let ruling;

        match double_play_defense_play_result.throw_target_base {
            Base::First => {
                runner_time = self.batter_runner_time_to(Base::First, true)?;
                (ruling, time_difference) =
                    judge(double_play_defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Out {
                    unsaved_runners.runner_1st = None;
                };
            }
            Base::Second => {
                runner_time = self.total_runner_time(Base::First, Base::Second)?;
                (ruling, time_difference) =
                    judge(double_play_defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Out {
                    unsaved_runners.runner_2nd = None;
                };
            }
            Base::Third => {
                runner_time = self.total_runner_time(Base::Second, Base::Third)?;
                (ruling, time_difference) =
                    judge(double_play_defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Out {
                    unsaved_runners.runner_3rd = None;
                };
            }
            _ => {
                return Err(GameError::DoublePlayTargetBase);
            }
        };

        let double_play_runner_advance_result = DoublePlayRunnerAdvanceResult {
            defense_time: double_play_defense_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: double_play_defense_play_result.throw_target_base,
            ruling: ruling,
            unsaved_runners: unsaved_runners,
        };

        Ok(double_play_runner_advance_result)
    }

    // CONSTRAINT: Home steal is not supported.
    // CONSTRAINT: Double steal is not supported.
    pub fn after_base_stealing(
        &mut self,
        steal_defense_play_result: StealDefensePlayResult,
        start_reaction: f64, // TODO: judge mechanism should be implemented.
    ) -> Result<StealRunnerAdvanceResult, GameError> {
        let runner_time;
        let time_difference;
        let ruling;

        match steal_defense_play_result.throw_target_base {
            Base::Third => {
                runner_time = self.total_runner_time(Base::Second, Base::Third)? + start_reaction;
                (ruling, time_difference) =
                    judge(steal_defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Safe {
                    self.runner_3rd = self.runner_2nd;
                    self.runner_2nd = None;
                };
            }
            Base::Second => {
                runner_time = self.total_runner_time(Base::First, Base::Second)? + start_reaction;
                (ruling, time_difference) =
                    judge(steal_defense_play_result.defense_time, runner_time);

                if ruling == Ruling::Safe {
                    self.runner_2nd = self.runner_1st;
                    self.runner_1st = None;
                };
            }
            _ => {
                return Err(GameError::StealTargetBase);
            }
        }

        let steal_runner_advance_result = StealRunnerAdvanceResult {
            defense_time: steal_defense_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: steal_defense_play_result.throw_target_base,
            ruling: ruling,
        };
        Ok(steal_runner_advance_result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::shared::player::{Position, RL};

    const EPSILON: f64 = 1e-9;

    fn assert_near(actual: f64, expected: f64) {
        assert!(
            (actual - expected).abs() < EPSILON,
            "expected {actual} to be near {expected}"
        );
    }

    fn runner(speed: f64) -> Runner {
        Runner {
            speed,
            lead_distance: 0.0,
            target_base: None,
        }
    }

    fn runner_with_lead(speed: f64, lead_distance: f64) -> Runner {
        Runner {
            speed,
            lead_distance,
            target_base: None,
        }
    }

    fn runners(
        batting_side: RL,
        batter_runner: Option<Runner>,
        runner_1st: Option<Runner>,
        runner_2nd: Option<Runner>,
        runner_3rd: Option<Runner>,
    ) -> RunnersOnBase {
        RunnersOnBase {
            batting_side: Some(batting_side),
            batter_runner,
            runner_1st,
            runner_2nd,
            runner_3rd,
        }
    }

    fn defense_result(
        throw_target_base: Base,
        play_type: PlayType,
        time_to_field: f64,
        defense_time: f64,
    ) -> DefensePlayResult {
        DefensePlayResult {
            time_to_field,
            throw_target_base,
            play_type,
            final_fielder_position: Position::FB,
            cutoff_fielder_position: None,
            defense_time,
        }
    }

    fn double_play_result(
        throw_target_base: Base,
        defense_time: f64,
    ) -> DoublePlayDefensePlayResult {
        DoublePlayDefensePlayResult {
            throw_target_base,
            final_fielder_position: Position::FB,
            defense_time,
        }
    }

    fn steal_result(throw_target_base: Base, defense_time: f64) -> StealDefensePlayResult {
        StealDefensePlayResult {
            throw_target_base,
            final_fielder_position: Position::SB,
            defense_time,
        }
    }

    #[test]
    fn total_runner_time_applies_batter_side_penalty_lag_and_runner_lead() {
        let runners = runners(
            RL::Right,
            Some(runner(8.0)),
            Some(runner_with_lead(7.0, 3.0)),
            None,
            None,
        );

        let batter_to_second = runners
            .total_runner_time(Base::Home, Base::Second)
            .unwrap();
        let runner_to_third = runners
            .total_runner_time(Base::First, Base::Third)
            .unwrap();

        assert_near(
            batter_to_second,
            ((BASE_DISTANCE * 2.0) + 2.0) / 8.0
                + ACCELERATION_LAG_TO_FIRST_BASE
                + ACCELERATION_LAG_AFTER_FIRST_BASE,
        );
        assert_near(
            runner_to_third,
            ((BASE_DISTANCE * 2.0) - 3.0) / 7.0 + (ACCELERATION_LAG_AFTER_FIRST_BASE * 2.0),
        );
    }

    #[test]
    fn total_runner_time_rejects_same_or_missing_runner_paths() {
        let empty_runners = runners(RL::Left, Some(runner(8.0)), None, None, None);

        assert!(matches!(
            empty_runners.total_runner_time(Base::First, Base::First),
            Err(GameError::SameTargetBase)
        ));
        assert!(matches!(
            empty_runners.total_runner_time(Base::Second, Base::Home),
            Err(GameError::Runner2nd)
        ));
        let runners_with_second = runners(
            RL::Left,
            Some(runner(8.0)),
            None,
            Some(runner(7.0)),
            None,
        );
        assert!(matches!(
            runners_with_second.total_runner_time(Base::Second, Base::First),
            Err(GameError::UnsupportedPath)
        ));
    }

    #[test]
    fn after_homerun_scores_all_occupied_bases_and_clears_runners() {
        let mut runners = runners(
            RL::Left,
            Some(runner(8.0)),
            Some(runner(7.0)),
            Some(runner(7.1)),
            Some(runner(7.2)),
        );

        let runs_scored = runners.after_homerun();

        assert_eq!(runs_scored, 4);
        assert!(runners.batting_side.is_none());
        assert!(runners.batter_runner.is_none());
        assert!(runners.runner_1st.is_none());
        assert!(runners.runner_2nd.is_none());
        assert!(runners.runner_3rd.is_none());
    }

    #[test]
    fn after_infield_grounder_safe_at_first_records_single_and_forced_advances() {
        let runners = runners(
            RL::Left,
            Some(runner(8.0)),
            Some(runner(7.0)),
            Some(runner(7.0)),
            Some(runner(7.0)),
        );
        let batter_time = runners.batter_runner_time_to(Base::First, true).unwrap();

        let result = runners
            .after_infield_grounder(defense_result(
                Base::First,
                PlayType::ForcePlay,
                1.0,
                batter_time + 0.01,
            ))
            .unwrap();

        assert_eq!(result.ruling, Ruling::Safe);
        assert_eq!(result.batting_result, BattingResult::Single);
        assert_eq!(result.runs_scored, 1);
        assert!(result.unsaved_runners.runner_1st.is_some());
        assert!(result.unsaved_runners.runner_2nd.is_some());
        assert!(result.unsaved_runners.runner_3rd.is_some());
    }

    #[test]
    fn after_infield_grounder_out_at_home_keeps_force_advances_without_scoring() {
        let runners = runners(
            RL::Left,
            Some(runner(8.0)),
            Some(runner(7.0)),
            Some(runner(7.0)),
            Some(runner(7.0)),
        );
        let runner_time = runners.total_runner_time(Base::Third, Base::Home).unwrap();

        let result = runners
            .after_infield_grounder(defense_result(
                Base::Home,
                PlayType::ForcePlay,
                1.0,
                runner_time - 0.01,
            ))
            .unwrap();

        assert_eq!(result.ruling, Ruling::Out);
        assert_eq!(result.batting_result, BattingResult::Out);
        assert_eq!(result.runs_scored, 0);
        assert!(result.unsaved_runners.runner_1st.is_some());
        assert!(result.unsaved_runners.runner_2nd.is_some());
        assert!(result.unsaved_runners.runner_3rd.is_some());
    }

    #[test]
    fn after_outfield_hit_late_fielding_attempts_double_and_scores_existing_runners() {
        let runners = runners(
            RL::Left,
            Some(runner(8.0)),
            Some(runner(7.0)),
            Some(runner(7.0)),
            Some(runner(7.0)),
        );
        let batter_to_first_without_lag = runners.batter_runner_time_to(Base::First, false).unwrap();
        let batter_to_second = runners.batter_runner_time_to(Base::Second, true).unwrap();

        let result = runners
            .after_outfield_hit(defense_result(
                Base::Second,
                PlayType::TouchPlay,
                batter_to_first_without_lag + 0.01,
                batter_to_second + 0.01,
            ))
            .unwrap();

        assert_eq!(result.ruling, Ruling::Safe);
        assert_eq!(result.batting_result, BattingResult::Double);
        assert_eq!(result.runs_scored, 3);
        assert!(result.unsaved_runners.runner_1st.is_none());
        assert!(result.unsaved_runners.runner_2nd.is_some());
        assert!(result.unsaved_runners.runner_3rd.is_none());
    }

    #[test]
    fn after_outfield_hit_batter_out_still_scores_existing_runner_from_third() {
        let runners = runners(
            RL::Left,
            Some(runner(8.0)),
            Some(runner(7.0)),
            None,
            Some(runner(7.0)),
        );
        let batter_to_first_without_lag = runners.batter_runner_time_to(Base::First, false).unwrap();
        let batter_to_first = runners.batter_runner_time_to(Base::First, true).unwrap();

        let result = runners
            .after_outfield_hit(defense_result(
                Base::First,
                PlayType::ForcePlay,
                batter_to_first_without_lag - 0.01,
                batter_to_first - 0.01,
            ))
            .unwrap();

        assert_eq!(result.ruling, Ruling::Out);
        assert_eq!(result.batting_result, BattingResult::Out);
        assert_eq!(result.runs_scored, 1);
        assert!(result.unsaved_runners.runner_1st.is_none());
        assert!(result.unsaved_runners.runner_2nd.is_some());
        assert!(result.unsaved_runners.runner_3rd.is_none());
    }

    #[test]
    fn after_tagup_safe_at_home_scores_and_holds_other_runners() {
        let runners = runners(
            RL::Left,
            Some(runner(8.0)),
            Some(runner(7.0)),
            Some(runner(7.0)),
            Some(runner(7.0)),
        );
        let runner_time = runners.total_runner_time(Base::Third, Base::Home).unwrap();

        let result = runners
            .after_tagup(defense_result(
                Base::Home,
                PlayType::TouchPlay,
                1.0,
                runner_time + 0.01,
            ))
            .unwrap();

        assert_eq!(result.ruling, Ruling::Safe);
        assert_eq!(result.batting_result, BattingResult::Out);
        assert_eq!(result.runs_scored, 1);
        assert!(result.unsaved_runners.runner_1st.is_some());
        assert!(result.unsaved_runners.runner_2nd.is_some());
        assert!(result.unsaved_runners.runner_3rd.is_none());
    }

    #[test]
    fn after_double_play_removes_runner_when_second_throw_wins() {
        let runners = runners(
            RL::Left,
            Some(runner(8.0)),
            Some(runner(7.0)),
            None,
            None,
        );
        let previous_unsaved = RunnersUnsaved {
            runner_1st: None,
            runner_2nd: Some(runner(7.0)),
            runner_3rd: None,
        };
        let runner_time = runners.total_runner_time(Base::First, Base::Second).unwrap();

        let result = runners
            .after_double_play(double_play_result(Base::Second, runner_time - 0.01), previous_unsaved)
            .unwrap();

        assert_eq!(result.ruling, Ruling::Out);
        assert!(result.unsaved_runners.runner_2nd.is_none());
    }

    #[test]
    fn after_double_play_rejects_home_target() {
        let runners = runners(RL::Left, Some(runner(8.0)), None, None, None);

        assert!(matches!(
            runners.after_double_play(
                double_play_result(Base::Home, 1.0),
                RunnersUnsaved::default()
            ),
            Err(GameError::DoublePlayTargetBase)
        ));
    }

    #[test]
    fn after_base_stealing_safe_to_second_moves_runner() {
        let mut runners = runners(RL::Left, None, Some(runner(7.0)), None, None);
        let runner_time = runners.total_runner_time(Base::First, Base::Second).unwrap();

        let result = runners
            .after_base_stealing(steal_result(Base::Second, runner_time + 0.11), 0.1)
            .unwrap();

        assert_eq!(result.ruling, Ruling::Safe);
        assert!(runners.runner_1st.is_none());
        assert!(runners.runner_2nd.is_some());
    }

    #[test]
    fn after_base_stealing_out_to_third_keeps_runner_on_second() {
        let mut runners = runners(RL::Left, None, None, Some(runner(7.0)), None);
        let runner_time = runners.total_runner_time(Base::Second, Base::Third).unwrap();

        let result = runners
            .after_base_stealing(steal_result(Base::Third, runner_time - 0.01), 0.0)
            .unwrap();

        assert_eq!(result.ruling, Ruling::Out);
        assert!(runners.runner_2nd.is_some());
        assert!(runners.runner_3rd.is_none());
    }

    #[test]
    fn after_base_stealing_rejects_unsupported_target() {
        let mut runners = runners(RL::Left, None, Some(runner(7.0)), None, None);

        assert!(matches!(
            runners.after_base_stealing(steal_result(Base::Home, 1.0), 0.0),
            Err(GameError::StealTargetBase)
        ));
    }
}
