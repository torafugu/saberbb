use super::{GameError, GameState, Ruling};
use crate::domain::{
    resolver::{
        batting_resolver::{SwingContactResult, calculate_trajectory},
        fielding_physics::CatchType,
    },
    shared::{
        ball::{BattedBall, OutboundResult},
        game::BattingResult,
        game_stats::PlayerGameBattingMetrics,
        player::{BatterInfo, Position, RL},
        stadium::Base,
    },
};
use std::cell::RefCell;

#[derive(Clone, Copy, Debug)]
pub struct ContactFirstEvPlay {
    pub outcome: Option<BattingResult>,
    pub fielder: Option<Position>,
    pub direct_catch: bool,
    pub batter_out_at_first: bool,
    pub projected_bounce_distance_m: Option<f64>,
}

pub struct ContactFirstEvSwing {
    pub count_seq: u16,
    pub baseline_exit_speed_ms: f64,
    pub baseline_launch_angle_deg: f64,
    pub plays: Vec<ContactFirstEvPlay>,
}

pub struct ContactFirstLaunch {
    pub count_seq: u16,
    pub exit_speed_ms: f64,
    pub launch_angle_deg: f64,
    pub pitch_speed_ms: f64,
    pub bat_speed_ms: f64,
    pub swing_power: f64,
    pub contact: SwingContactResult,
}

struct Audit {
    reductions_ms: Vec<f64>,
    swings: Vec<ContactFirstEvSwing>,
}

thread_local! {
    static AUDIT: RefCell<Option<Audit>> = const { RefCell::new(None) };
    static LAUNCHES: RefCell<Option<Vec<ContactFirstLaunch>>> = const { RefCell::new(None) };
}

/// Collect inputs to the live exit-speed formula for every fair ContactFirst launch.
pub fn with_contact_first_launch_audit<T>(run: impl FnOnce() -> T) -> (T, Vec<ContactFirstLaunch>) {
    struct Restore(Option<Vec<ContactFirstLaunch>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            LAUNCHES.replace(self.0.take());
        }
    }
    let _restore = Restore(LAUNCHES.replace(Some(Vec::new())));
    let result = run();
    (
        result,
        LAUNCHES
            .take()
            .expect("ContactFirst launch audit is active"),
    )
}

fn is_fair_launch(ball: &BattedBall) -> bool {
    ball.outbound_result != OutboundResult::Foul && !ball.is_foul()
}

pub(super) fn record_launch(
    count_seq: u16,
    pitch_speed_ms: f64,
    batter: &BatterInfo,
    contact: &SwingContactResult,
    ball: &BattedBall,
) {
    LAUNCHES.with_borrow_mut(|slot| {
        if let Some(launches) = slot
            && is_fair_launch(ball)
        {
            launches.push(ContactFirstLaunch {
                count_seq,
                exit_speed_ms: ball.launch_speed,
                launch_angle_deg: ball.launch_angle,
                pitch_speed_ms,
                bat_speed_ms: batter.swing_speed,
                swing_power: batter.swing_power,
                contact: contact.clone(),
            });
        }
    });
}

/// Observe baseline ContactFirst fair flights at or above 45 m/s in a seeded game.
pub fn with_contact_first_ev_audit<T>(
    reductions_ms: &[f64],
    run: impl FnOnce() -> T,
) -> (T, Vec<ContactFirstEvSwing>) {
    struct Restore(Option<Audit>);
    impl Drop for Restore {
        fn drop(&mut self) {
            AUDIT.replace(self.0.take());
        }
    }
    let _restore = Restore(AUDIT.replace(Some(Audit {
        reductions_ms: reductions_ms.to_vec(),
        swings: Vec::new(),
    })));
    let result = run();
    let audit = AUDIT
        .take()
        .expect("ContactFirst exit-speed audit is active");
    (result, audit.swings)
}

impl GameState {
    pub(super) fn record_contact_first_ev_trials(
        &self,
        pitcher_id: i64,
        batter_id: i64,
        batting_side: RL,
        contact: &SwingContactResult,
        flight: Option<(BattedBall, PlayerGameBattingMetrics)>,
    ) -> Result<(), GameError> {
        AUDIT.with_borrow_mut(|slot| {
            let Some(audit) = slot.as_mut() else {
                return Ok(());
            };
            let Some((baseline, metrics)) = flight else {
                return Ok(());
            };
            if baseline.launch_speed < 45.0 || !is_fair_launch(&baseline) {
                return Ok(());
            }
            let mut plays = Vec::with_capacity(audit.reductions_ms.len() + 1);
            for reduction in std::iter::once(0.0).chain(audit.reductions_ms.iter().copied()) {
                let ball = if reduction == 0.0 {
                    baseline
                } else {
                    calculate_trajectory(
                        baseline.launch_speed - reduction,
                        baseline.launch_angle,
                        metrics.final_hla_deg,
                        baseline.spin_rate,
                        baseline.spin_angle,
                        &self.stadium,
                    )?
                };
                let play = self.replay_swing(
                    pitcher_id,
                    batter_id,
                    batting_side,
                    contact,
                    Some((ball, metrics)),
                    self.rng.fork().ok_or(GameError::PairedContactRng)?,
                )?;
                let diagnostics = play.fielding.and_then(|f| f.catch_diagnostics);
                plays.push(ContactFirstEvPlay {
                    outcome: play.batting_result,
                    fielder: play.fielding.map(|f| {
                        diagnostics.map_or(f.catch_fielder_position, |d| d.fielder_position)
                    }),
                    direct_catch: diagnostics.is_some_and(|d| {
                        d.catch_type == CatchType::DirectFly && d.error_type.is_none()
                    }),
                    batter_out_at_first: play.running.iter().any(|running| {
                        running.target_runner_id == Some(batter_id)
                            && running.throw_target_base == Base::First
                            && running.ruling == Ruling::Out
                    }),
                    projected_bounce_distance_m: ball.first_bounce_position.map(|p| p.distance),
                });
            }
            audit.swings.push(ContactFirstEvSwing {
                count_seq: self.count_seq,
                baseline_exit_speed_ms: baseline.launch_speed,
                baseline_launch_angle_deg: baseline.launch_angle,
                plays,
            });
            Ok(())
        })
    }
}
