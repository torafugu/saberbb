use super::{
    GameError, GameResult, GameState, Inning, VERTICAL_CONTACT_WINDOW_MM, launch_swing,
    vertical_contact_window_ev_shift_mm, with_ground_rule_double_catch_trial,
    with_vertical_contact_window_trial,
};
use crate::domain::{
    random_provider::RandomProvider,
    resolver::{
        batting_resolver::{
            SwingContactResult, SwingExecutionError, evaluate_swing_contact,
            pre_trial_exit_speed_ms,
        },
        pitching_resolver::PitchDisplacement,
    },
    shared::{
        ball::{BattedBall, OutboundResult, PitchedBall},
        game::BattingResult,
        game_stats::{PlayerGameBattingMetrics, PlayerGameFielding, PlayerGameRunning},
        player::{BatterInfo, RL},
        stadium::{Base, Stadium},
    },
};
use serde::Serialize;
use std::cell::RefCell;

mod fence;
pub use fence::FenceAudit;
use fence::PairedFenceAudit;

type Flight = Option<(BattedBall, PlayerGameBattingMetrics)>;

thread_local! {
    static PLAYS: RefCell<Option<Vec<PairedContactPlay>>> = const { RefCell::new(None) };
}

/// Replay the contact shifts under the historical automatic ground-rule double award.
pub fn with_paired_contact_fielding<T>(run: impl FnOnce() -> T) -> (T, Vec<PairedContactPlay>) {
    struct Restore(Option<Vec<PairedContactPlay>>);
    impl Drop for Restore {
        fn drop(&mut self) {
            PLAYS.replace(self.0.take());
        }
    }
    let _restore = Restore(PLAYS.replace(Some(Vec::new())));
    let result =
        with_ground_rule_double_catch_trial(false, || with_vertical_contact_window_trial(4, run));
    let plays = PLAYS.take().expect("paired contact audit is active");
    (result, plays)
}

#[derive(Serialize)]
pub struct PairedContactPlay {
    pub count_seq: u16,
    pub outs_before: u8,
    /// Occupancy of first, second and third, in that order.
    pub occupied_bases: [bool; 3],
    pub baseline_contact_z_mm: f64,
    pub baseline_ev_ms: f64,
    pub flat: Play,
    pub by_baseline_ev: Play,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fence_audit: Option<PairedFenceAudit>,
}

#[derive(Serialize)]
pub struct Play {
    pub ball: Option<BattedBall>,
    pub launch_direction_deg: Option<f64>,
    pub batting_result: Option<BattingResult>,
    pub fielding: Option<PlayerGameFielding>,
    pub running: Vec<PlayerGameRunning>,
}

impl Play {
    fn from_result(game: &GameResult, count_seq: u16, flight: Flight) -> Self {
        let batting_result = game
            .player_battings
            .last()
            .filter(|row| row.count_seq == count_seq)
            .map(|row| row.result);
        let fielding = game
            .player_fieldings
            .iter()
            .rev()
            .take_while(|row| row.count_seq == count_seq)
            .last()
            .copied();
        let mut running: Vec<_> = game
            .player_runnings
            .iter()
            .rev()
            .take_while(|row| row.count_seq == count_seq)
            .copied()
            .collect();
        running.reverse();
        Self {
            ball: flight.map(|(ball, _)| ball),
            launch_direction_deg: flight.map(|(_, metrics)| metrics.final_hla_deg),
            batting_result,
            fielding,
            running,
        }
    }

    pub fn launch_band(&self) -> &'static str {
        let Some(ball) = self.ball.as_ref() else {
            return "no_contact";
        };
        if ball.outbound_result == OutboundResult::Foul || ball.is_foul() {
            "foul"
        } else if !is_target(Some(ball)) {
            "outside_target"
        } else if ball.launch_angle < 20.0 {
            "15_to_20_deg"
        } else {
            "20_to_25_deg"
        }
    }

    pub fn result_label(&self) -> &str {
        self.batting_result
            .as_ref()
            .map_or("NoBattingResult", |result| result.as_ref())
    }
}

pub(super) fn is_target(ball: Option<&BattedBall>) -> bool {
    ball.is_some_and(|ball| {
        ball.outbound_result != OutboundResult::Foul
            && !ball.is_foul()
            && (45.0..47.5).contains(&ball.launch_speed)
            && (15.0..25.0).contains(&ball.launch_angle)
    })
}

pub(super) struct PairedSwing {
    baseline_contact_z_mm: f64,
    baseline_ev_ms: f64,
    contact: SwingContactResult,
}

fn fork_rng(rng: &dyn RandomProvider) -> Result<Box<dyn RandomProvider>, GameError> {
    rng.fork().ok_or(GameError::PairedContactRng)
}

impl PairedSwing {
    pub(super) fn prepare(
        rng: &dyn RandomProvider,
        batter: &BatterInfo,
        pitch: &PitchedBall,
        displacement: &PitchDisplacement,
        mut swing_error: SwingExecutionError,
        baseline_z_m: f64,
    ) -> Result<Option<Self>, GameError> {
        let baseline_contact_z_mm = baseline_z_m * 1000.0;
        if !PLAYS.with_borrow(Option::is_some)
            || !VERTICAL_CONTACT_WINDOW_MM.contains(&baseline_contact_z_mm)
        {
            return Ok(None);
        }
        let Some(baseline_ev_ms) =
            pre_trial_exit_speed_ms(batter, pitch.speed, displacement, &swing_error)
        else {
            return Ok(None);
        };
        let shift = vertical_contact_window_ev_shift_mm(Some(baseline_ev_ms));
        if shift == 4 {
            return Ok(None);
        }
        swing_error.additional_z_m += f64::from(shift) / 1000.0;
        let contact =
            evaluate_swing_contact(fork_rng(rng)?.as_mut(), batter, displacement, &swing_error);
        Ok(Some(Self {
            baseline_contact_z_mm,
            baseline_ev_ms,
            contact,
        }))
    }

    pub(super) fn launch(
        &self,
        rng: &dyn RandomProvider,
        batter: &BatterInfo,
        pitch: PitchedBall,
        stadium: &Stadium,
    ) -> Result<Flight, GameError> {
        launch_swing(
            fork_rng(rng)?.as_mut(),
            batter,
            pitch,
            &self.contact,
            stadium,
        )
    }

    pub(super) fn resolve_alternative(
        self,
        live: &GameState,
        pitcher_id: i64,
        batter_id: i64,
        batting_side: RL,
        flights: [Flight; 2],
    ) -> Result<ResolvedAlternative, GameError> {
        let [flat, alternative] = flights;
        let fence_audit = PairedFenceAudit::compare(live, flat, alternative)?;
        let by_baseline_ev = live.replay_swing(
            pitcher_id,
            batter_id,
            batting_side,
            &self.contact,
            alternative,
            fork_rng(live.rng.as_ref())?,
        )?;
        Ok(ResolvedAlternative {
            outs_before: live.inning_state.out,
            occupied_bases: [Base::First, Base::Second, Base::Third]
                .map(|base| live.inning_state.runners.has_runner_on(base)),
            baseline_contact_z_mm: self.baseline_contact_z_mm,
            baseline_ev_ms: self.baseline_ev_ms,
            by_baseline_ev,
            fence_audit,
        })
    }
}

impl GameState {
    pub(super) fn replay_swing(
        &self,
        pitcher_id: i64,
        batter_id: i64,
        batting_side: RL,
        contact: &SwingContactResult,
        flight: Flight,
        rng: Box<dyn RandomProvider>,
    ) -> Result<Play, GameError> {
        let live = self;
        let mut replay = GameState {
            pitch_location_settings: self.pitch_location_settings,
            record_contact_states: false,
            track_position_player_workload: false,
            away_manager: live.away_manager.clone(),
            home_manager: live.home_manager.clone(),
            batter_characters: live.batter_characters.clone(),
            batting_strategy_settings: live.batting_strategy_settings.clone(),
            rng,
            inning_seq: live.inning_seq,
            inning_tb: live.inning_tb,
            count_seq: live.count_seq,
            away_total_point: live.away_total_point,
            home_total_point: live.home_total_point,
            away_lineup: live.away_lineup.clone(),
            home_lineup: live.home_lineup.clone(),
            inning_state: live.inning_state.clone(),
            inning: Inning {
                seq: live.inning.seq,
                tb: live.inning.tb,
                counts: Vec::new(),
            },
            game_result: GameResult::new(
                live.game_result.id,
                live.game_result.actual_date,
                &[],
                &[],
            ),
            stadium: live.stadium.clone(),
            max_inning: live.max_inning,
            base_four_seam_speed: live.base_four_seam_speed,
        };
        replay.commit_swing(pitcher_id, batter_id, batting_side, contact, flight)?;
        Ok(Play::from_result(
            &replay.game_result,
            live.count_seq,
            flight,
        ))
    }
}

pub(super) struct ResolvedAlternative {
    outs_before: u8,
    occupied_bases: [bool; 3],
    baseline_contact_z_mm: f64,
    baseline_ev_ms: f64,
    by_baseline_ev: Play,
    fence_audit: Option<PairedFenceAudit>,
}

impl ResolvedAlternative {
    pub(super) fn record(self, live: &GameState, count_seq: u16, flight: Flight) {
        PLAYS.with_borrow_mut(|plays| {
            plays
                .as_mut()
                .expect("paired contact audit is active")
                .push(PairedContactPlay {
                    count_seq,
                    outs_before: self.outs_before,
                    occupied_bases: self.occupied_bases,
                    baseline_contact_z_mm: self.baseline_contact_z_mm,
                    baseline_ev_ms: self.baseline_ev_ms,
                    flat: Play::from_result(&live.game_result, count_seq, flight),
                    by_baseline_ev: self.by_baseline_ev,
                    fence_audit: self.fence_audit,
                });
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        game_service::simulate_game, random_provider::RealRng,
        strategy::batting_strategy::BattingStrategySettings, test_support::game_schedule,
    };

    #[test]
    fn paired_fielding_preserves_the_seeded_flat_game_and_matches_its_results() {
        let mut schedule = game_schedule();
        for player in schedule
            .away_team
            .players
            .iter_mut()
            .chain(&mut schedule.home_team.players)
        {
            if let Some(batter) = player.offense_skills.batter.as_mut() {
                batter.attack_angle = 6.0;
                batter.swing_speed = 32.0;
            }
            if let Some(pitcher) = player.defense_skills.pitcher.as_mut() {
                for (index, pitch) in pitcher.pitch_skills.iter_mut().enumerate() {
                    pitch.velocity = if index == 0 { 1.0 } else { 0.92 };
                    pitch.spin_rate = 1.0;
                }
            }
        }
        let mut fielded_pairs = 0;
        for seed in [113, 271, 20261109] {
            let run = || {
                simulate_game(
                    schedule.clone(),
                    Box::new(RealRng::from_seed(seed)),
                    BattingStrategySettings::player_driven(),
                    Some(5000),
                    false,
                )
                .unwrap()
            };
            let flat = with_ground_rule_double_catch_trial(false, || {
                with_vertical_contact_window_trial(4, run)
            });
            let (audited, pairs) = with_paired_contact_fielding(run);
            assert_eq!(
                serde_json::to_value(&audited).unwrap(),
                serde_json::to_value(&flat).unwrap()
            );
            for pair in pairs {
                assert!(
                    is_target(pair.flat.ball.as_ref())
                        || is_target(pair.by_baseline_ev.ball.as_ref())
                );
                assert!(VERTICAL_CONTACT_WINDOW_MM.contains(&pair.baseline_contact_z_mm));
                assert_eq!(
                    pair.fence_audit.is_some(),
                    [pair.flat.ball, pair.by_baseline_ev.ball]
                        .iter()
                        .flatten()
                        .any(|ball| { ball.outbound_result == OutboundResult::GroundRuleDouble })
                );
                assert_eq!(
                    vertical_contact_window_ev_shift_mm(Some(pair.baseline_ev_ms)),
                    2
                );
                let logged = flat
                    .player_battings
                    .iter()
                    .find(|row| row.count_seq == pair.count_seq);
                assert_eq!(pair.flat.batting_result, logged.map(|row| row.result));
                if let Some(ball) = pair.flat.ball {
                    if let Some(logged) = logged {
                        assert_eq!(ball.launch_speed, logged.ball.launch_speed);
                        assert_eq!(ball.launch_angle, logged.ball.launch_angle);
                    }
                }
                fielded_pairs += usize::from(pair.by_baseline_ev.fielding.is_some());
            }
        }
        assert!(
            fielded_pairs > 0,
            "fixture should include an alternate play resolved by fielders"
        );
    }

    #[test]
    fn target_ranges_are_half_open_and_require_a_fair_trajectory() {
        let mut ball = BattedBall::new(
            45.0,
            15.0,
            110.0,
            0.0,
            3.0,
            None,
            None,
            None,
            None,
            OutboundResult::InField,
        );
        assert!(is_target(Some(&ball)));
        ball.launch_angle = 25.0;
        assert!(!is_target(Some(&ball)));
        ball.launch_angle = 20.0;
        ball.launch_speed = 47.5;
        assert!(!is_target(Some(&ball)));
        ball.launch_speed = 47.49;
        ball.outbound_result = OutboundResult::Foul;
        assert!(!is_target(Some(&ball)));
        assert!(!is_target(None));
    }
}
