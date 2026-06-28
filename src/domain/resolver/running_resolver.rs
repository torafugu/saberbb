use super::fielding_resolver::{
    DefencePlayResult, DoublePlayDefencePlayResult, PlayType, StealDefencePlayResult,
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

#[derive(Clone, Debug)]
pub struct RunnersUnsaved {
    pub runner_1st: Option<Runner>,
    pub runner_2nd: Option<Runner>,
    pub runner_3rd: Option<Runner>,
}

#[derive(Clone, Debug)]
pub struct RunnersOnBase {
    pub batting_side: Option<RL>,
    pub batter_runner: Option<Runner>,
    pub runner_1st: Option<Runner>,
    pub runner_2nd: Option<Runner>,
    pub runner_3rd: Option<Runner>,
}
impl RunnersOnBase {
    pub fn new() -> Self {
        Self {
            batting_side: None,
            batter_runner: None,
            runner_1st: None,
            runner_2nd: None,
            runner_3rd: None,
        }
    }

    fn clear(&mut self) {
        self.batting_side = None;
        self.batter_runner = None;
        self.runner_1st = None;
        self.runner_2nd = None;
        self.batting_side = None;
    }

    fn set_batter_runner_taget_base(&mut self, target_base: Base) {
        if let Some(batter_runner) = self.batter_runner.as_mut() {
            batter_runner.target_base = Some(target_base);
        }
    }

    fn set_runner_1st_taget_base(&mut self, target_base: Base) {
        if let Some(runner_1st) = self.runner_1st.as_mut() {
            runner_1st.target_base = Some(target_base);
        }
    }

    fn set_runner_2nd_taget_base(&mut self, target_base: Base) {
        if let Some(runner_2nd) = self.runner_2nd.as_mut() {
            runner_2nd.target_base = Some(target_base);
        }
    }

    fn set_runner_3rd_taget_base(&mut self, target_base: Base) {
        if let Some(runner_3rd) = self.runner_3rd.as_mut() {
            runner_3rd.target_base = Some(target_base);
        }
    }

    fn batter_runner_taget_base(&self) -> Option<Base> {
        if let Some(batter_runner) = self.batter_runner {
            batter_runner.target_base
        } else {
            None
        }
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
        self.runner_1st.is_some() && self.runner_2nd.is_some() && self.runner_3rd.is_some()
    }

    fn running_distance_from_home_to_first_base(&self) -> Result<f64, GameError> {
        if let Some(batting_side) = &self.batting_side {
            if *batting_side == RL::Right {
                Ok(BASE_DISTANCE + 2.0)
            } else {
                Ok(BASE_DISTANCE)
            }
        } else {
            return Err(GameError::BatterRunner);
        }
    }

    fn running_distance_from_home_to_second_base(&self) -> Result<f64, GameError> {
        Ok(self.running_distance_from_home_to_first_base()? + BASE_DISTANCE)
    }

    fn running_distance_from_home_to_third_base(&self) -> Result<f64, GameError> {
        Ok(self.running_distance_from_home_to_first_base()? + BASE_DISTANCE + BASE_DISTANCE)
    }

    pub fn batter_runner_to_first_base_time(&self, with_lag: bool) -> Result<f64, GameError> {
        let lag = if with_lag {
            ACCELERATION_LAG_TO_FIRST_BASE
        } else {
            0.0
        };

        let running_distance = self.running_distance_from_home_to_first_base()?;
        if let Some(batter_runner) = &self.batter_runner {
            Ok((running_distance / batter_runner.speed) + lag)
        } else {
            return Err(GameError::BatterRunner);
        }
    }

    pub fn batter_runner_to_second_base_time(&self, with_lag: bool) -> Result<f64, GameError> {
        let lag = if with_lag {
            ACCELERATION_LAG_TO_FIRST_BASE + ACCELERATION_LAG_AFTER_FIRST_BASE
        } else {
            0.0
        };

        let running_distance = self.running_distance_from_home_to_first_base()? + BASE_DISTANCE;
        if let Some(batter_runner) = &self.batter_runner {
            Ok((running_distance / batter_runner.speed) + lag)
        } else {
            return Err(GameError::BatterRunner);
        }
    }

    pub fn batter_runner_to_third_base_time(&self, with_lag: bool) -> Result<f64, GameError> {
        let lag = if with_lag {
            ACCELERATION_LAG_TO_FIRST_BASE + ACCELERATION_LAG_AFTER_FIRST_BASE * 2.0
        } else {
            0.0
        };

        let running_distance =
            self.running_distance_from_home_to_first_base()? + BASE_DISTANCE * 2.0;
        if let Some(batter_runner) = &self.batter_runner {
            Ok((running_distance / batter_runner.speed) + lag)
        } else {
            return Err(GameError::BatterRunner);
        }
    }

    pub fn batter_runner_to_home_base_time(&self, with_lag: bool) -> Result<f64, GameError> {
        let lag = if with_lag {
            ACCELERATION_LAG_TO_FIRST_BASE + ACCELERATION_LAG_AFTER_FIRST_BASE * 3.0
        } else {
            0.0
        };

        let running_distance =
            self.running_distance_from_home_to_first_base()? + BASE_DISTANCE * 3.0;
        if let Some(batter_runner) = &self.batter_runner {
            Ok((running_distance / batter_runner.speed) + lag)
        } else {
            return Err(GameError::BatterRunner);
        }
    }

    fn runner_advance_one_base_time(
        &self,
        runner: Runner,
        with_lag: bool,
    ) -> Result<f64, GameError> {
        let lag = if with_lag {
            ACCELERATION_LAG_AFTER_FIRST_BASE
        } else {
            0.0
        };

        Ok((BASE_DISTANCE - runner.lead_distance) / runner.speed + lag)
    }

    fn runner_advance_two_base_time(
        &self,
        runner: Runner,
        with_lag: bool,
    ) -> Result<f64, GameError> {
        let lag = if with_lag {
            ACCELERATION_LAG_AFTER_FIRST_BASE * 2.0
        } else {
            0.0
        };

        Ok((BASE_DISTANCE * 2.0 - runner.lead_distance) / runner.speed + lag)
    }

    fn runner_advance_three_base_time(
        &self,
        runner: Runner,
        with_lag: bool,
    ) -> Result<f64, GameError> {
        let lag = if with_lag {
            ACCELERATION_LAG_AFTER_FIRST_BASE * 3.0
        } else {
            0.0
        };

        Ok((BASE_DISTANCE * 3.0 - runner.lead_distance) / runner.speed + lag)
    }

    pub fn total_runner_time(&self, from_base: Base, to_base: Base) -> Result<f64, GameError> {
        let total_runner_time = match to_base {
            Base::First => {
                if from_base == Base::Home {
                    self.batter_runner_to_first_base_time(true)?
                } else {
                    0.0
                }
            }
            Base::Second => {
                if from_base == Base::Home {
                    self.batter_runner_to_second_base_time(true)?
                } else if from_base == Base::First {
                    if let Some(runner_1st) = self.runner_1st {
                        self.runner_advance_one_base_time(runner_1st, true)?
                    } else {
                        return Err(GameError::BatterRunner);
                    }
                } else {
                    0.0
                }
            }
            Base::Third => {
                if from_base == Base::Home {
                    self.batter_runner_to_third_base_time(true)?
                } else if from_base == Base::First {
                    if let Some(runner_1st) = self.runner_1st {
                        self.runner_advance_two_base_time(runner_1st, true)?
                    } else {
                        return Err(GameError::BatterRunner);
                    }
                } else if from_base == Base::Second {
                    if let Some(runner_2nd) = self.runner_2nd {
                        self.runner_advance_one_base_time(runner_2nd, true)?
                    } else {
                        return Err(GameError::BatterRunner);
                    }
                } else {
                    0.0
                }
            }
            Base::Home => {
                if from_base == Base::Home {
                    self.batter_runner_to_home_base_time(true)?
                } else if from_base == Base::First {
                    if let Some(runner_1st) = self.runner_1st {
                        self.runner_advance_three_base_time(runner_1st, true)?
                    } else {
                        return Err(GameError::BatterRunner);
                    }
                } else if from_base == Base::Second {
                    if let Some(runner_2nd) = self.runner_2nd {
                        self.runner_advance_two_base_time(runner_2nd, true)?
                    } else {
                        return Err(GameError::BatterRunner);
                    }
                } else if from_base == Base::Third {
                    if let Some(runner_3rd) = self.runner_3rd {
                        self.runner_advance_one_base_time(runner_3rd, true)?
                    } else {
                        return Err(GameError::BatterRunner);
                    }
                } else {
                    0.0
                }
            }
        };
        Ok(total_runner_time)
    }

    // TODO: Consider Hit and Run
    fn set_running_plan(&mut self, time_to_catch: f64) -> Result<(), GameError> {
        let batter_to_first_time = self.batter_runner_to_first_base_time(false)?;
        let batter_to_second_time = self.batter_runner_to_second_base_time(false)?;

        // Triple
        if time_to_catch > batter_to_second_time {
            self.set_batter_runner_taget_base(Base::Third);
            self.set_runner_1st_taget_base(Base::Home);
            self.set_runner_2nd_taget_base(Base::Home);

        // Double
        } else if time_to_catch > batter_to_first_time {
            self.set_batter_runner_taget_base(Base::Second);
            self.set_runner_1st_taget_base(Base::Home);
            self.set_runner_2nd_taget_base(Base::Home);

        // Single
        } else {
            self.set_batter_runner_taget_base(Base::First);
            // CONSTRAINT: If not runner is already started, i.e. base sreal or hit and run
            self.set_runner_1st_taget_base(Base::Second);
            // CONSTRAINT: If not runner is already started, i.e. base sreal or hit and run
            self.set_runner_2nd_taget_base(Base::Home);
        }

        self.set_runner_3rd_taget_base(Base::Home);

        Ok(())
    }

    pub fn after_homerun(&mut self) -> u16 {
        let mut runs_scored: u16 = 1;

        if self.runner_1st.is_some() {
            runs_scored += 1;
        }

        if self.runner_2nd.is_some() {
            runs_scored += 1;
        }

        if self.runner_3rd.is_some() {
            runs_scored += 1;
        }

        self.clear(); // commit_unsaved_runners is not needed

        runs_scored
    }

    pub fn commit_unsaved_runners(&mut self, unsaved_runners: RunnersOnBase) {
        self.runner_1st = unsaved_runners.runner_1st;
        self.runner_2nd = unsaved_runners.runner_2nd;
        self.runner_3rd = unsaved_runners.runner_3rd;
    }

    pub fn after_infield_grounder(
        &mut self,
        defence_play_result: DefencePlayResult,
    ) -> Result<RunnerAdvanceResult, GameError> {
        let mut runner_time = 0.0;
        let mut time_difference = 0.0;
        let mut next_1st = None;
        let mut next_2nd = None;
        let mut next_3rd = None;
        let mut ruling = Ruling::Safe;
        let mut batting_result = BattingResult::Out;
        let mut runs_scored: u16 = 0;

        match defence_play_result.throw_target_base {
            Base::Home => {
                // Batter runner is automarically safe.
                next_1st = self.batter_runner;

                // 1st runner goes to second base.
                if self.runner_1st.is_some() {
                    next_2nd = self.runner_1st;
                };

                // 2nd runner goes to third base.
                if self.runner_2nd.is_some() {
                    next_3rd = self.runner_2nd;
                };

                if self.runner_3rd.is_some() {
                    runner_time = self.total_runner_time(Base::Third, Base::Home)?;

                    time_difference = defence_play_result.defense_time - runner_time;

                    if time_difference > 0.0 {
                        runs_scored += 1;
                        batting_result = BattingResult::FieldersChoice;
                    } else {
                        ruling = Ruling::Out;
                    };
                };
            }
            Base::Third => {
                // Batter runner is automarically safe.
                next_1st = self.batter_runner;

                // 1st runner goes to second base.
                if self.runner_1st.is_some() {
                    next_2nd = self.runner_1st;
                };

                if self.runner_2nd.is_some() {
                    runner_time = self.total_runner_time(Base::Second, Base::Third)?;

                    time_difference = defence_play_result.defense_time - runner_time;

                    if time_difference > 0.0 {
                        next_3rd = self.runner_2nd;
                        batting_result = BattingResult::FieldersChoice;
                    } else {
                        ruling = Ruling::Out;
                    };
                };

                // 3rd runner is automatically home in.
                if self.runner_3rd.is_some() {
                    runs_scored += 1;
                };
            }
            Base::Second => {
                // Batter runner is automarically safe.
                next_1st = self.batter_runner;

                if self.runner_1st.is_some() {
                    runner_time = self.total_runner_time(Base::First, Base::Second)?;

                    time_difference = defence_play_result.defense_time - runner_time;

                    if time_difference > 0.0 {
                        next_2nd = self.runner_1st;
                        batting_result = BattingResult::FieldersChoice;
                    } else {
                        ruling = Ruling::Out;
                    };
                };

                // 2nd runner goes to third base.
                if self.runner_2nd.is_some() {
                    next_3rd = self.runner_2nd;
                };

                // 3rd runner is automatically home in.
                if self.runner_3rd.is_some() {
                    runs_scored += 1;
                };
            }
            Base::First => {
                runner_time = self.batter_runner_to_first_base_time(true)?;

                time_difference = defence_play_result.defense_time - runner_time;

                if time_difference > 0.0 {
                    next_1st = self.batter_runner;
                    batting_result = BattingResult::Single;
                } else {
                    ruling = Ruling::Out;
                    batting_result = BattingResult::Out;
                };

                // 1st runner goes to second base.
                if self.runner_1st.is_some() {
                    next_2nd = self.runner_1st;
                };

                // 2nd runner goes to third base.
                if self.runner_2nd.is_some() {
                    next_3rd = self.runner_2nd;
                };

                // 3rd runner is automatically home in.
                if self.runner_3rd.is_some() {
                    runs_scored += 1;
                };
            }
        }

        let unsaved_runners = RunnersUnsaved {
            runner_1st: next_1st,
            runner_2nd: next_2nd,
            runner_3rd: next_3rd,
        };

        let runner_advance_result = RunnerAdvanceResult {
            defense_time: defence_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: defence_play_result.throw_target_base,
            play_type: defence_play_result.play_type,
            ruling: ruling,
            batting_result: batting_result,
            runs_scored: runs_scored,
            unsaved_runners: unsaved_runners,
        };
        Ok(runner_advance_result)
    }

    pub fn after_outfield_hit(
        &mut self,
        defence_play_result: DefencePlayResult,
    ) -> Result<RunnerAdvanceResult, GameError> {
        self.set_running_plan(defence_play_result.time_to_catch)?;

        let mut runner_time = 0.0;
        let mut time_difference = 0.0;
        let mut next_1st = None;
        let mut next_2nd = None;
        let mut next_3rd = None;
        let mut ruling = Ruling::Safe;
        let batting_result;
        let mut runs_scored: u16 = 0;

        if let Some(batter_runner_target_base) = self.batter_runner_taget_base() {
            match batter_runner_target_base {
                Base::Third => {
                    match defence_play_result.throw_target_base {
                        Base::Home => {
                            // Batter runner is automarically safe.
                            next_3rd = self.batter_runner;
                            batting_result = BattingResult::Triple;

                            if self.runner_1st.is_some() {
                                runner_time = self.total_runner_time(Base::First, Base::Home)?;

                                time_difference = defence_play_result.defense_time - runner_time;

                                if time_difference > 0.0 {
                                    runs_scored += 1;
                                } else {
                                    ruling = Ruling::Out
                                };
                            };
                        }
                        Base::Third => {
                            let runner_time = self.batter_runner_to_third_base_time(true)?;
                            time_difference = defence_play_result.defense_time - runner_time;

                            if time_difference > 0.0 {
                                // Batter runner is safe
                                next_3rd = self.batter_runner;
                                batting_result = BattingResult::Triple;
                            } else {
                                ruling = Ruling::Out;
                                batting_result = BattingResult::Double;
                            };

                            // 1st runner is automatically home in.
                            if self.runner_1st.is_some() {
                                runs_scored += 1;
                            };
                        }
                        _ => {
                            // Throw to second base
                            // Batter runner is automarically safe.
                            next_3rd = self.batter_runner;
                            batting_result = BattingResult::Triple;

                            // 1st runner is automatically home in.
                            if self.runner_1st.is_some() {
                                runs_scored += 1;
                            };
                        }
                    }

                    // 3rd runner is automatically home in.
                    if self.runner_3rd.is_some() {
                        runs_scored += 1;
                    };

                    // 2nd runner is automatically home in.
                    if self.runner_2nd.is_some() {
                        runs_scored += 1;
                    };
                }
                Base::Second => {
                    match defence_play_result.throw_target_base {
                        Base::Home => {
                            // Batter runner is automarically safe.
                            next_2nd = self.batter_runner;
                            batting_result = BattingResult::Double;

                            if self.runner_2nd.is_some() {
                                runner_time = self.total_runner_time(Base::Second, Base::Home)?;
                                time_difference = defence_play_result.defense_time - runner_time;

                                if time_difference > 0.0 {
                                    runs_scored += 1;
                                } else {
                                    ruling = Ruling::Out;
                                };
                            }

                            if self.runner_1st.is_some() {
                                if ruling == Ruling::Safe {
                                    // 2nd runner goes to home base even if 3rd runner is already home in.
                                    runner_time =
                                        self.total_runner_time(Base::First, Base::Home)?;
                                    time_difference =
                                        defence_play_result.defense_time - runner_time;

                                    if time_difference > 0.0 {
                                        runs_scored += 1;
                                    } else {
                                        ruling = Ruling::Out;
                                    };
                                } else {
                                    // 2nd runner stops at 3rd base in case 2nd runner is touched out.
                                    next_3rd = self.runner_1st;
                                }
                            };
                        }
                        Base::Third => {
                            // Batter runner is automarically safe.
                            next_2nd = self.batter_runner;
                            batting_result = BattingResult::Double;

                            // 2nd runner is automatically home in.
                            if self.runner_2nd.is_some() {
                                runs_scored += 1;
                            };

                            // 1st runner is automatically home in.
                            if self.runner_1st.is_some() {
                                runs_scored += 1;
                            };
                        }
                        _ => {
                            // Throw to second base
                            let runner_time = self.batter_runner_to_second_base_time(true)?;
                            time_difference = defence_play_result.defense_time - runner_time;

                            if time_difference > 0.0 {
                                // Batter runner is safe
                                next_2nd = self.batter_runner;
                                batting_result = BattingResult::Double;
                            } else {
                                ruling = Ruling::Out;
                                batting_result = BattingResult::Single;
                            };

                            // 2nd runner is automatically home in.
                            if self.runner_2nd.is_some() {
                                runs_scored += 1;
                            };

                            // 1st runner is automatically home in.
                            if self.runner_1st.is_some() {
                                runs_scored += 1;
                            };
                        }
                    }

                    // 3rd runner is automatically home in.
                    if self.runner_3rd.is_some() {
                        runs_scored += 1;
                    };
                }
                Base::First => {
                    // throw_target_base is always second base
                    // CONSTRAINT: Right Goundout case is not covered now

                    // Batter runner is automarically safe.
                    next_1st = self.batter_runner;
                    batting_result = BattingResult::Single;

                    // 1st runner goes to second base.
                    if self.runner_1st.is_some() {
                        next_2nd = self.runner_1st;
                    };

                    // 2nd runner goes to third base.
                    if self.runner_2nd.is_some() {
                        next_3rd = self.runner_2nd;
                    };

                    // 3rd runner is automatically home in.
                    if self.runner_3rd.is_some() {
                        runs_scored += 1;
                    };
                }
                _ => {
                    // CONSTRAINT: Running home run is not covered.
                    return Err(GameError::BatterRunnerTargetBase);
                }
            }
        } else {
            return Err(GameError::BatterRunner);
        }

        let unsaved_runners = RunnersUnsaved {
            runner_1st: next_1st,
            runner_2nd: next_2nd,
            runner_3rd: next_3rd,
        };

        let runner_advance_result = RunnerAdvanceResult {
            defense_time: defence_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: defence_play_result.throw_target_base,
            play_type: defence_play_result.play_type,
            ruling: ruling,
            batting_result: batting_result,
            runs_scored: runs_scored,
            unsaved_runners: unsaved_runners,
        };
        Ok(runner_advance_result)
    }

    pub fn after_tagup(
        &mut self,
        defence_play_result: DefencePlayResult,
    ) -> Result<RunnerAdvanceResult, GameError> {
        let mut runner_time = 0.0;
        let mut time_difference = 0.0;
        // 1st runner does not start
        // let mut next_1st = None;
        let mut next_2nd = None;
        let mut next_3rd = None;
        let mut ruling = Ruling::Safe;
        let mut runs_scored: u16 = 0;

        match defence_play_result.throw_target_base {
            Base::Home => {
                if self.runner_3rd.is_none() {
                    return Err(GameError::ThirdRunner);
                }

                runner_time = self.total_runner_time(Base::Third, Base::Home)?;

                time_difference = defence_play_result.defense_time - runner_time;

                if time_difference > 0.0 {
                    runs_scored += 1;
                } else {
                    ruling = Ruling::Out;
                };

                // 2nd runner does not move
                next_2nd = self.runner_2nd;
            }
            Base::Third => {
                if self.runner_2nd.is_some() {
                    runner_time = self.total_runner_time(Base::Second, Base::Third)?;

                    time_difference = defence_play_result.defense_time - runner_time;

                    if time_difference > 0.0 {
                        next_3rd = self.runner_2nd;
                    } else {
                        ruling = Ruling::Out;
                    };
                };

                // 3rd runner is automatically home in.
                if self.runner_3rd.is_some() {
                    runs_scored += 1;
                };
            }
            _ => {
                // 2nd runner goes to third base.
                if self.runner_2nd.is_some() {
                    next_3rd = self.runner_2nd;
                };

                // 3rd runner is automatically home in.
                if self.runner_3rd.is_some() {
                    runs_scored += 1;
                };
            }
        }

        let unsaved_runners = RunnersUnsaved {
            runner_1st: None,
            runner_2nd: next_2nd,
            runner_3rd: next_3rd,
        };

        let runner_advance_result = RunnerAdvanceResult {
            defense_time: defence_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: defence_play_result.throw_target_base,
            play_type: defence_play_result.play_type,
            ruling: ruling,
            batting_result: BattingResult::Out,
            runs_scored: runs_scored,
            unsaved_runners: unsaved_runners,
        };
        Ok(runner_advance_result)
    }

    pub fn after_double_play(
        &mut self,
        double_play_defence_play_result: DoublePlayDefencePlayResult,
        unsaved_runners: RunnersUnsaved,
    ) -> Result<DoublePlayRunnerAdvanceResult, GameError> {
        let mut runner_time = 0.0;
        let mut time_difference = 0.0;
        let mut next_1st = unsaved_runners.runner_1st;
        let mut next_2nd = unsaved_runners.runner_2nd;
        let mut next_3rd = unsaved_runners.runner_3rd;
        let mut ruling = Ruling::Safe;

        match double_play_defence_play_result.throw_target_base {
            Base::First => {
                // CONSTRAINT: Temporary runners do not go to next base.
                // Do nothing for temporary 3rd runner (2nd runner).
                // Do nothing for temporary 2nd runner (1st runner).

                runner_time = self.batter_runner_to_first_base_time(true)?;
                time_difference = double_play_defence_play_result.defense_time - runner_time;

                // CONSTRAINT: Batter runner should be exsit on first base because previous throw target is not first base.
                if time_difference > 0.0 {
                    // Do nothing for temporary 1st runner (Butter runner).
                } else {
                    ruling = Ruling::Out;
                    next_1st = None;
                };
            }
            Base::Second => {
                // CONSTRAINT: Temporary runners do not go to next base.
                // Do nothing for temporary 3rd runner (2nd runner).

                if next_2nd.is_some() {
                    runner_time = self.total_runner_time(Base::First, Base::Second)?;
                    time_difference = double_play_defence_play_result.defense_time - runner_time;

                    if time_difference > 0.0 {
                        // Do nothing for temporary 2nd runner (1st runner).
                    } else {
                        ruling = Ruling::Out;
                        next_2nd = None;
                    };
                };

                // CONSTRAINT: No need to update 1st runner becuase previous throw target is not first base or batter runner is already called safe.
                // Do nothing for temporary 1st runner (Butter runner).
            }
            Base::Third => {
                if next_3rd.is_some() {
                    runner_time = self.total_runner_time(Base::Second, Base::Third)?;
                    time_difference = double_play_defence_play_result.defense_time - runner_time;

                    if time_difference > 0.0 {
                        // Do nothing for temporary 3rd runner (2nd runner).
                    } else {
                        ruling = Ruling::Out;
                        next_3rd = None;
                    };
                };

                // CONSTRAINT: Temporary runners do not go to next base.
                // Do nothing for temporary 2nd runner (1st runner).

                // CONSTRAINT: No need to update 1st runner becuase previous throw target is not first base or batter runner is already called safe.
                // Do nothing for temporary 1st runner (Butter runner).
            }
            _ => {
                return Err(GameError::DoublePlayTargetBase);
            }
        };

        let unsaved_runners = RunnersUnsaved {
            runner_1st: next_1st,
            runner_2nd: next_2nd,
            runner_3rd: next_3rd,
        };

        let double_play_runner_advance_result = DoublePlayRunnerAdvanceResult {
            defense_time: double_play_defence_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: double_play_defence_play_result.throw_target_base,
            ruling: ruling,
            unsaved_runners: unsaved_runners,
        };

        Ok(double_play_runner_advance_result)
    }

    // CONSTRAINT: Home steal is not covered.
    // CONSTRAINT: Double steal is not covered.
    pub fn after_base_stealing(
        &mut self,
        steal_defence_play_result: StealDefencePlayResult,
        start_reaction: f64, // TODO: judge mechanism should be implemented.
    ) -> Result<StealRunnerAdvanceResult, GameError> {
        let runner_time;
        let time_difference;

        let mut next_2nd = None;
        let mut next_3rd = None;
        let mut ruling = Ruling::Safe;

        match steal_defence_play_result.throw_target_base {
            Base::Third => {
                if self.runner_2nd.is_none() {
                    return Err(GameError::SecondRunner);
                }

                runner_time = self.total_runner_time(Base::Second, Base::Third)? + start_reaction;
                time_difference = steal_defence_play_result.defense_time - runner_time;

                if time_difference > 0.0 {
                    next_3rd = self.runner_2nd;
                } else {
                    ruling = Ruling::Out;
                };
            }
            Base::Second => {
                if self.runner_1st.is_none() {
                    return Err(GameError::FirstRunner);
                }

                runner_time = self.total_runner_time(Base::First, Base::Second)? + start_reaction;
                time_difference = steal_defence_play_result.defense_time - runner_time;

                if time_difference > 0.0 {
                    next_2nd = self.runner_1st;
                } else {
                    ruling = Ruling::Out;
                };
            }
            _ => {
                return Err(GameError::StealTargetBase);
            }
        }

        self.runner_2nd = next_2nd;
        self.runner_3rd = next_3rd;

        let steal_runner_advance_result = StealRunnerAdvanceResult {
            defense_time: steal_defence_play_result.defense_time,
            runner_time: runner_time,
            time_difference: time_difference,
            throw_target_base: steal_defence_play_result.throw_target_base,
            ruling: ruling,
        };
        Ok(steal_runner_advance_result)
    }
}
