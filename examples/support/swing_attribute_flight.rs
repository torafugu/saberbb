use super::{GameResult, GameSchedule, bip_audit};
use saberbb::domain::{
    resolver::batting_resolver::{calculate_trajectory, launch_speed_terms},
    shared::{
        ball::{BattedBall, OutboundResult},
        game::BattingResult,
        game_stats::PlayerGameBatting,
        stadium::Stadium,
    },
    util::calculate_polar_distance,
};
use serde::Serialize;
use std::collections::BTreeMap;

pub(super) const POWER_DELTA: f64 = 1.0;
pub(super) const SPEED_DELTA_MS: f64 = 1.0;

#[derive(Default, Serialize)]
struct Transitions {
    home_runs: u64,
    gained: u64,
    lost: u64,
    gained_from_doubles: u64,
    exit_speed_delta_sum_ms: f64,
}

impl Transitions {
    fn observe(&mut self, was_hr: bool, was_double: bool, (is_hr, delta_ms): (bool, f64)) {
        self.home_runs += u64::from(is_hr);
        self.gained += u64::from(is_hr && !was_hr);
        self.gained_from_doubles += u64::from(is_hr && !was_hr && was_double);
        self.lost += u64::from(was_hr && !is_hr);
        self.exit_speed_delta_sum_ms += delta_ms;
    }
}

#[derive(Clone, Copy)]
struct Trial {
    name: &'static str,
    speed_delta_ms: f64,
    power_delta: f64,
}

const LEGACY_TRIALS: &[Trial] = &[
    Trial {
        name: "power_plus_1",
        speed_delta_ms: 0.0,
        power_delta: POWER_DELTA,
    },
    Trial {
        name: "speed_plus_1_ms",
        speed_delta_ms: SPEED_DELTA_MS,
        power_delta: 0.0,
    },
];

#[derive(Serialize)]
struct Cell {
    paired_contacts: u64,
    baseline_home_runs: u64,
    baseline_doubles: u64,
    #[serde(flatten)]
    trials: BTreeMap<&'static str, Transitions>,
}

impl Cell {
    fn new(trials: impl IntoIterator<Item = &'static str>) -> Self {
        Self {
            paired_contacts: 0,
            baseline_home_runs: 0,
            baseline_doubles: 0,
            trials: trials
                .into_iter()
                .map(|name| (name, Transitions::default()))
                .collect(),
        }
    }

    fn observe(
        &mut self,
        was_hr: bool,
        was_double: bool,
        outcomes: &[(&'static str, (bool, f64))],
    ) {
        self.paired_contacts += 1;
        self.baseline_home_runs += u64::from(was_hr);
        self.baseline_doubles += u64::from(was_double);
        for (name, outcome) in outcomes {
            self.trials
                .get_mut(name)
                .expect("registered trial")
                .observe(was_hr, was_double, *outcome);
        }
    }
}

fn same_flight(baseline: &BattedBall, live: &BattedBall) -> bool {
    baseline.outbound_result == live.outbound_result
        && (baseline.total_time - live.total_time).abs() <= 1e-9
        && calculate_polar_distance(&baseline.final_position, &live.final_position) <= 1e-8
        && baseline.first_bounce_time == live.first_bounce_time
}

pub(super) const EXIT_SPEED_REDUCTIONS: [(&str, f64); 4] = [
    ("ev_minus_0_5_ms", 0.5),
    ("ev_minus_1_ms", 1.0),
    ("ev_minus_2_ms", 2.0),
    ("ev_minus_3_ms", 3.0),
];

pub(super) const LAUNCH_ANGLE_SHIFTS: [(&str, f64); 4] = [
    ("vla_minus_4_deg", -4.0),
    ("vla_minus_2_deg", -2.0),
    ("vla_plus_2_deg", 2.0),
    ("vla_plus_4_deg", 4.0),
];

#[derive(Clone, Copy)]
pub(super) enum CappedFlightAdjustment {
    ExitSpeed,
    LaunchAngle,
}

impl CappedFlightAdjustment {
    fn trials(self) -> &'static [(&'static str, f64)] {
        match self {
            Self::ExitSpeed => &EXIT_SPEED_REDUCTIONS,
            Self::LaunchAngle => &LAUNCH_ANGLE_SHIFTS,
        }
    }
}

#[derive(Serialize)]
pub(super) struct CappedContactFlightAudit {
    #[serde(skip)]
    adjustment: CappedFlightAdjustment,
    selected_fair_contacts: u64,
    selected_home_runs: u64,
    missing_inputs: u64,
    baseline_flight_error: u64,
    baseline_flight_mismatch: u64,
    trial_flight_error: u64,
    by_original_ev_vla: BTreeMap<String, Cell>,
}

impl CappedContactFlightAudit {
    pub(super) fn new(adjustment: CappedFlightAdjustment) -> Self {
        Self {
            adjustment,
            selected_fair_contacts: 0,
            selected_home_runs: 0,
            missing_inputs: 0,
            baseline_flight_error: 0,
            baseline_flight_mismatch: 0,
            trial_flight_error: 0,
            by_original_ev_vla: BTreeMap::new(),
        }
    }

    pub(super) fn observe(&mut self, stadium: &Stadium, batting: &PlayerGameBatting) {
        let ball = &batting.ball;
        let ev = ball.launch_speed;
        let vla = ball.launch_angle;
        if !(47.5..52.5).contains(&ev) || !(20.0..35.0).contains(&vla) {
            return;
        }
        self.selected_fair_contacts += 1;
        let was_hr = batting.result == BattingResult::HomeRun;
        self.selected_home_runs += u64::from(was_hr);
        if ![batting.final_hla_deg, ball.spin_rate, ball.spin_angle]
            .into_iter()
            .all(f64::is_finite)
            || ball.spin_rate < 0.0
        {
            self.missing_inputs += 1;
            return;
        }
        let replay = |speed, angle| {
            calculate_trajectory(
                speed,
                angle,
                batting.final_hla_deg,
                ball.spin_rate,
                ball.spin_angle,
                stadium,
            )
        };
        let Ok(baseline) = replay(ev, vla) else {
            self.baseline_flight_error += 1;
            return;
        };
        if !same_flight(&baseline, ball)
            || (baseline.outbound_result == OutboundResult::HomeRun) != was_hr
        {
            self.baseline_flight_mismatch += 1;
            return;
        }
        let trials = self.adjustment.trials();
        let alternatives = trials
            .iter()
            .map(|&(name, amount)| {
                let (ev_delta, vla_delta) = match self.adjustment {
                    CappedFlightAdjustment::ExitSpeed => (-amount, 0.0),
                    CappedFlightAdjustment::LaunchAngle => (0.0, amount),
                };
                replay(ev + ev_delta, vla + vla_delta).map(|flight| {
                    (
                        name,
                        (flight.outbound_result == OutboundResult::HomeRun, ev_delta),
                    )
                })
            })
            .collect::<Result<Vec<_>, _>>();
        let Ok(alternatives) = alternatives else {
            self.trial_flight_error += 1;
            return;
        };
        let ev_band = bip_audit::band(ev, &[47.5, 50.0, 52.5]);
        let vla_band = bip_audit::band(vla, &[20.0, 25.0, 30.0, 35.0]);
        self.by_original_ev_vla
            .entry(format!("EV_ms:{ev_band}|VLA_deg:{vla_band}"))
            .or_insert_with(|| Cell::new(trials.iter().map(|&(name, _)| name)))
            .observe(was_hr, batting.result == BattingResult::Double, &alternatives);
    }
}

#[derive(Serialize)]
struct ContactGeometryCell {
    baseline_exit_speed_sum_ms: f64,
    #[serde(flatten)]
    flight: Cell,
}

#[derive(Serialize)]
struct CappedSwap {
    peer_batter_id: i64,
    effective_swing_speed_delta_ms: f64,
    swing_power_delta: f64,
    by_contact_geometry_at_25_to_30_deg: BTreeMap<String, ContactGeometryCell>,
}

#[derive(Serialize)]
pub(super) struct FrozenFlightAudit {
    #[serde(skip)]
    batter_id: Option<i64>,
    #[serde(skip)]
    trials: Vec<Trial>,
    selected_fair_contacts: u64,
    missing_inputs: u64,
    baseline_ev_mismatch: u64,
    baseline_flight_error: u64,
    baseline_flight_mismatch: u64,
    trial_flight_error: u64,
    total: Cell,
    by_original_ev_vla: BTreeMap<String, Cell>,
    #[serde(skip_serializing_if = "Option::is_none")]
    capped_swap: Option<CappedSwap>,
}

impl Default for FrozenFlightAudit {
    fn default() -> Self {
        Self::new(None, LEGACY_TRIALS)
    }
}

impl FrozenFlightAudit {
    fn new(batter_id: Option<i64>, trials: &[Trial]) -> Self {
        Self {
            batter_id,
            trials: trials.to_vec(),
            selected_fair_contacts: 0,
            missing_inputs: 0,
            baseline_ev_mismatch: 0,
            baseline_flight_error: 0,
            baseline_flight_mismatch: 0,
            trial_flight_error: 0,
            total: Cell::new(trials.iter().map(|trial| trial.name)),
            by_original_ev_vla: BTreeMap::new(),
            capped_swap: None,
        }
    }

    pub(super) fn for_batter(batter_id: i64, speed_delta_ms: f64, power_delta: f64) -> Self {
        Self::new(
            Some(batter_id),
            &[
                Trial {
                    name: "reference_speed",
                    speed_delta_ms,
                    power_delta: 0.0,
                },
                Trial {
                    name: "reference_power",
                    speed_delta_ms: 0.0,
                    power_delta,
                },
                Trial {
                    name: "reference_speed_and_power",
                    speed_delta_ms,
                    power_delta,
                },
            ],
        )
    }

    pub(super) fn for_capped_swap(
        batter_id: i64,
        peer_batter_id: i64,
        speed_delta_ms: f64,
        power_delta: f64,
    ) -> Self {
        let mut audit = Self::for_batter(batter_id, speed_delta_ms, power_delta);
        audit.capped_swap = Some(CappedSwap {
            peer_batter_id,
            effective_swing_speed_delta_ms: speed_delta_ms,
            swing_power_delta: power_delta,
            by_contact_geometry_at_25_to_30_deg: BTreeMap::new(),
        });
        audit
    }

    pub(super) fn observe_game(&mut self, schedule: &GameSchedule, game: &GameResult) {
        let power: BTreeMap<_, _> = schedule
            .away_team
            .players
            .iter()
            .chain(&schedule.home_team.players)
            .filter_map(|p| Some((p.info.id, p.offense_skills.batter?.swing_power)))
            .collect();
        let pitch_speeds: BTreeMap<_, _> = game
            .player_pitchings
            .iter()
            .map(|p| ((p.count_seq, p.pitcher_id), p.ball.speed))
            .collect();
        for b in &game.player_battings {
            if self.batter_id.is_some_and(|id| id != b.batter_id)
                || (!bip_audit::is_bip(b.result) && b.result != BattingResult::HomeRun)
            {
                continue;
            }
            self.selected_fair_contacts += 1;
            let (Some(&power), Some(&pitch), Some(depth), Some(thickness)) = (
                power.get(&b.batter_id),
                pitch_speeds.get(&(b.count_seq, b.pitcher_id)),
                b.timing_impact_x_m,
                b.thickness_offset_m,
            ) else {
                self.missing_inputs += 1;
                continue;
            };
            let live = &b.ball;
            let speed = b.bat_speed;
            let length = b.length_offset_m.abs();
            if b.hla_model_version != Some(2)
                || ![
                    power,
                    pitch,
                    depth,
                    thickness,
                    depth,
                    speed,
                    length,
                    live.launch_speed,
                    live.launch_angle,
                    b.final_hla_deg,
                    live.spin_rate,
                    live.spin_angle,
                ]
                .into_iter()
                .all(f64::is_finite)
                || pitch <= 0.0
                || speed <= 0.0
                || live.launch_speed <= 0.0
                || thickness < 0.0
                || live.spin_rate < 0.0
            {
                self.missing_inputs += 1;
                continue;
            }

            let exit_speed = |speed, power| {
                launch_speed_terms(pitch, speed, power, depth, thickness, length).exit_speed_ms()
            };
            let replay = |ev| {
                calculate_trajectory(
                    ev,
                    live.launch_angle,
                    b.final_hla_deg,
                    live.spin_rate,
                    live.spin_angle,
                    &schedule.stadium,
                )
            };
            if (exit_speed(speed, power) - live.launch_speed).abs() > 1e-8 {
                self.baseline_ev_mismatch += 1;
                continue;
            }
            let Ok(baseline) = replay(live.launch_speed) else {
                self.baseline_flight_error += 1;
                continue;
            };
            let was_hr = baseline.outbound_result == OutboundResult::HomeRun;
            if !same_flight(&baseline, live)
                || (self.capped_swap.is_some() && was_hr != (b.result == BattingResult::HomeRun))
            {
                self.baseline_flight_mismatch += 1;
                continue;
            }

            let alternatives = self
                .trials
                .iter()
                .map(|trial| {
                    let ev = exit_speed(speed + trial.speed_delta_ms, power + trial.power_delta);
                    replay(ev).map(|flight| {
                        (
                            trial.name,
                            (
                                flight.outbound_result == OutboundResult::HomeRun,
                                ev - live.launch_speed,
                            ),
                        )
                    })
                })
                .collect::<Result<Vec<_>, _>>();
            let Ok(alternatives) = alternatives else {
                self.trial_flight_error += 1;
                continue;
            };
            let was_double = b.result == BattingResult::Double;
            self.total.observe(was_hr, was_double, &alternatives);
            if let Some(swap) = &mut self.capped_swap
                && (25.0..30.0).contains(&live.launch_angle)
            {
                let length_band = b.signed_length_offset_m.map_or_else(
                    || "unknown".into(),
                    |m| bip_audit::band(m.abs() * 100.0, &[5.0]),
                );
                let thickness_band = b.signed_thickness_offset_m.map_or_else(
                    || "unknown".into(),
                    |m| bip_audit::band(m.abs() * 100.0, &[2.0]),
                );
                let cell = swap
                    .by_contact_geometry_at_25_to_30_deg
                    .entry(format!(
                        "length_cm:{length_band}|thickness_cm:{thickness_band}"
                    ))
                    .or_insert_with(|| ContactGeometryCell {
                        baseline_exit_speed_sum_ms: 0.0,
                        flight: Cell::new(self.trials.iter().map(|trial| trial.name)),
                    });
                cell.baseline_exit_speed_sum_ms += live.launch_speed;
                cell.flight.observe(was_hr, was_double, &alternatives);
            }
            let ev = bip_audit::band(live.launch_speed, &[35.0, 40.0, 45.0, 47.5, 50.0, 55.0]);
            let vla = bip_audit::band(
                live.launch_angle,
                &[0.0, 10.0, 20.0, 25.0, 30.0, 40.0, 50.0],
            );
            self.by_original_ev_vla
                .entry(format!("EV_ms:{ev}|VLA_deg:{vla}"))
                .or_insert_with(|| Cell::new(self.trials.iter().map(|trial| trial.name)))
                .observe(was_hr, was_double, &alternatives);
        }
    }
}
