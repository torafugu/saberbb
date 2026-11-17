//! Launch parameters paired with official fair-ball results, regardless of fielding path.
use super::bip_audit::{CONVERSION_EV_EDGES_MS, CONVERSION_HLA_EDGES_DEG, band, is_bip};
use saberbb::domain::{
    resolver::{
        batting_resolver::{
            CONTACT_TIMING_POSITION_SCALE, LaunchSpeedTerms, SwingContactType,
            calculate_dynamic_attack_angle, calculate_trajectory,
            calculate_vertical_contact_deflection, launch_speed_terms,
        },
        fielding_physics::CatchType,
    },
    shared::{
        ball::{BattedBall, OutboundResult},
        game::{BattingResult, GameResult, GameSchedule},
        game_stats::PlayerGameBatting,
        stadium::Stadium,
    },
    util::calculate_polar_distance,
};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};

const HIGH_EV_MIN_MS: f64 = 45.0;
const TARGET_VLA: std::ops::Range<f64> = 7.0..10.0;

#[derive(Default, Serialize)]
struct Outcomes {
    plays: u64,
    by_result: BTreeMap<String, u64>,
    direct_catch_outs: u64,
}

impl Outcomes {
    fn observe(&mut self, result: BattingResult, direct_catch: bool) {
        self.plays += 1;
        *self.by_result.entry(result.as_ref().into()).or_default() += 1;
        self.direct_catch_outs += u64::from(direct_catch && result == BattingResult::Out);
    }
}

#[derive(Default, Serialize)]
struct ContactQualityLaunch {
    non_hr_in_play: u64,
    missing_exit_speed: u64,
    exit_speed_sum_ms: f64,
    ev_ge_45_ms: u64,
}

#[derive(Default, Serialize)]
struct SwingOutcomes {
    swings: u64,
    fouls: u64,
    non_hr_in_play: u64,
    home_runs: u64,
    whiffs: u64,
    other: u64,
}

#[derive(Default, Serialize)]
struct ExitSpeedTotals {
    plays: u64,
    missing_exit_speed: u64,
    exit_speed_sum_ms: f64,
    ev_ge_45_ms: u64,
}

impl ExitSpeedTotals {
    fn observe(&mut self, ev: f64) {
        self.plays += 1;
        if ev.is_finite() && ev > 0.0 {
            self.exit_speed_sum_ms += ev;
            self.ev_ge_45_ms += u64::from(ev >= HIGH_EV_MIN_MS);
        } else {
            self.missing_exit_speed += 1;
        }
    }
}

const BAT_TERM_PERCENTS: [u8; 3] = [100, 97, 94];

fn scaled_exit_speed(terms: &LaunchSpeedTerms, percent: u8) -> f64 {
    let swing_term = terms.swing_term_ms * (f64::from(percent) / 100.0);
    (terms.pitch_term_ms + swing_term) * terms.thickness_retention * terms.length_retention
}

#[derive(Default, Serialize)]
struct HomeRunFlightTrial {
    home_runs: u64,
    fence_impacts: u64,
    ground_rule_doubles: u64,
    foul: u64,
    in_field: u64,
    replay_errors: u64,
}

impl HomeRunFlightTrial {
    fn observe(&mut self, ball: &BattedBall) {
        self.fence_impacts += u64::from(ball.fence_impact_position.is_some());
        match ball.outbound_result {
            OutboundResult::HomeRun => self.home_runs += 1,
            OutboundResult::GroundRuleDouble => self.ground_rule_doubles += 1,
            OutboundResult::Foul => self.foul += 1,
            OutboundResult::InField => self.in_field += 1,
        }
    }
}

#[derive(Default, Serialize)]
struct HomeRunFlightSweep {
    selected: u64,
    exit_speed_reproduced: u64,
    missing_flight_inputs: u64,
    baseline_errors: u64,
    baseline_mismatch: u64,
    baseline_reproduced: u64,
    trials: BTreeMap<u8, HomeRunFlightTrial>,
}

impl HomeRunFlightSweep {
    fn observe(&mut self, terms: &LaunchSpeedTerms, b: &PlayerGameBatting, stadium: &Stadium) {
        self.exit_speed_reproduced += 1;
        let live = &b.ball;
        if b.hla_model_version != Some(2)
            || ![
                b.final_hla_deg,
                live.launch_angle,
                live.spin_rate,
                live.spin_angle,
            ]
            .into_iter()
            .all(f64::is_finite)
            || live.spin_rate < 0.0
        {
            self.missing_flight_inputs += 1;
            return;
        }
        let replay = |percent| {
            calculate_trajectory(
                scaled_exit_speed(terms, percent),
                live.launch_angle,
                b.final_hla_deg,
                live.spin_rate,
                live.spin_angle,
                stadium,
            )
        };
        let Ok(baseline) = replay(100) else {
            self.baseline_errors += 1;
            return;
        };
        if baseline.outbound_result != OutboundResult::HomeRun
            || live.outbound_result != baseline.outbound_result
            || (baseline.total_time - live.total_time).abs() > 1e-9
            || calculate_polar_distance(&baseline.final_position, &live.final_position) > 1e-8
        {
            self.baseline_mismatch += 1;
            return;
        }
        self.baseline_reproduced += 1;
        for percent in BAT_TERM_PERCENTS {
            let trial = self.trials.entry(percent).or_default();
            let flight = if percent == 100 {
                Ok(baseline)
            } else {
                replay(percent)
            };
            match flight {
                Ok(ball) => trial.observe(&ball),
                Err(_) => trial.replay_errors += 1,
            }
        }
    }
}

#[derive(Default, Serialize)]
struct BatTermTrial {
    exit_speed: ExitSpeedTotals,
    exit_speed_bands_ms: BTreeMap<String, u64>,
    max_exit_speed_ms: Option<f64>,
    ev_ge_45_ms_vla_7_to_10: u64,
}

#[derive(Default, Serialize)]
struct BatTermSweep {
    selected: u64,
    missing_inputs: u64,
    baseline_mismatch: u64,
    trials: BTreeMap<u8, BatTermTrial>,
}

impl BatTermSweep {
    fn observe(&mut self, terms: &LaunchSpeedTerms, vla: f64) {
        for percent in BAT_TERM_PERCENTS {
            let ev = scaled_exit_speed(terms, percent);
            let trial = self.trials.entry(percent).or_default();
            trial.exit_speed.observe(ev);
            *trial
                .exit_speed_bands_ms
                .entry(band(
                    ev,
                    &[25.0, 30.0, 35.0, 40.0, 45.0, 47.5, 50.0, 52.5, 55.0, 60.0],
                ))
                .or_default() += 1;
            trial.max_exit_speed_ms = Some(trial.max_exit_speed_ms.map_or(ev, |max| max.max(ev)));
            trial.ev_ge_45_ms_vla_7_to_10 +=
                u64::from(ev >= HIGH_EV_MIN_MS && TARGET_VLA.contains(&vla));
        }
    }
}

#[derive(Default, Serialize)]
struct BatTermSweeps {
    all_fair: BatTermSweep,
    non_hr_in_play: BatTermSweep,
    live_home_runs: BatTermSweep,
    selected_solid_contacts: BatTermSweep,
}

#[derive(Default, Serialize)]
struct ExitSpeedComparison {
    observed: ExitSpeedTotals,
    neutral_swing_power: ExitSpeedTotals,
    zero_timing_depth: ExitSpeedTotals,
    centered_contact: ExitSpeedTotals,
    pitch_term_sum_ms: f64,
    swing_term_sum_ms: f64,
    thickness_retention_sum: f64,
    length_retention_sum: f64,
}

#[derive(Default, Serialize)]
struct ExitSpeedTermAudit {
    selected: u64,
    missing_inputs: u64,
    baseline_mismatch: u64,
    total: ExitSpeedComparison,
    by_input_bands: BTreeMap<String, ExitSpeedComparison>,
}

#[derive(Default, Serialize)]
struct ContactFlightCounts {
    fair_balls: u64,
    singles: u64,
    home_runs: u64,
    fast_low_balls: u64,
    fast_low_singles: u64,
}

impl ContactFlightCounts {
    fn observe(&mut self, result: BattingResult, fast_low: bool) {
        let single = result == BattingResult::Single;
        self.fair_balls += 1;
        self.singles += u64::from(single);
        self.home_runs += u64::from(result == BattingResult::HomeRun);
        self.fast_low_balls += u64::from(fast_low);
        self.fast_low_singles += u64::from(fast_low && single);
    }
}

#[derive(Default, Serialize)]
struct ContactFlightAudit {
    total: ContactFlightCounts,
    by_exit_speed_launch_angle_backspin: BTreeMap<String, ContactFlightCounts>,
    by_contact_parameter: BTreeMap<&'static str, BTreeMap<String, ContactFlightCounts>>,
    by_contact_type_bat_angle_signed_thickness: BTreeMap<String, ContactFlightCounts>,
}

fn is_fast_low(b: &PlayerGameBatting) -> bool {
    is_bip(b.result)
        && b.ball.launch_speed.is_finite()
        && b.ball.launch_speed >= HIGH_EV_MIN_MS
        && TARGET_VLA.contains(&b.ball.launch_angle)
}

impl ContactFlightAudit {
    fn observe(&mut self, b: &PlayerGameBatting) {
        let ball = &b.ball;
        let ev = if ball.launch_speed > 0.0 {
            ball.launch_speed
        } else {
            f64::NAN
        };
        let fast_low = is_fast_low(b);
        self.total.observe(b.result, fast_low);

        // The flight model uses the cosine of spin angle for lift versus sink.
        let backspin = if ball.spin_rate >= 0.0 {
            ball.spin_rate * ball.spin_angle.to_radians().cos()
        } else {
            f64::NAN
        };
        let speed = band(ev, &[25.0, 35.0, 40.0, 45.0, 47.5, 50.0, 55.0]);
        let angle = band(
            ball.launch_angle,
            &[
                -10.0, 0.0, 5.0, 7.0, 10.0, 15.0, 20.0, 25.0, 30.0, 35.0, 45.0, 60.0,
            ],
        );
        let spin = band(
            backspin,
            &[-1500.0, -500.0, 0.0, 500.0, 1500.0, 2500.0, 3500.0],
        );
        self.by_exit_speed_launch_angle_backspin
            .entry(format!("EV:{speed}|VLA:{angle}|backspin_rpm:{spin}"))
            .or_default()
            .observe(b.result, fast_low);

        let contact_type = b.adjusted_contact_type.as_ref();
        let bat_angle = band(
            b.actual_bat_angle_deg.unwrap_or(f64::NAN),
            &[0.0, 10.0, 20.0, 30.0, 40.0, 50.0],
        );
        let signed_thickness = band(
            b.signed_thickness_offset_m.unwrap_or(f64::NAN),
            &[-0.025, -0.01, -0.005, 0.0, 0.005, 0.01, 0.025],
        );
        self.by_contact_type_bat_angle_signed_thickness
            .entry(format!("contact:{contact_type}|bat_angle_deg:{bat_angle}|signed_thickness_m:{signed_thickness}"))
            .or_default()
            .observe(b.result, fast_low);

        for (parameter, bin) in [
            ("contact_type", contact_type.to_owned()),
            ("bat_angle_deg", bat_angle),
            ("signed_thickness_m", signed_thickness),
            (
                "signed_length_m",
                band(
                    b.signed_length_offset_m.unwrap_or(f64::NAN),
                    &[-0.1, -0.05, -0.02, 0.0, 0.02, 0.05, 0.1],
                ),
            ),
            (
                "bat_speed_ms",
                band(
                    if b.bat_speed > 0.0 {
                        b.bat_speed
                    } else {
                        f64::NAN
                    },
                    &[30.0, 32.5, 35.0, 40.0],
                ),
            ),
            (
                "timing_error_ms",
                band(
                    b.adapted_timing_offset_sec.unwrap_or(f64::NAN) * 1000.0,
                    &[-10.0, -5.0, -2.0, 0.0, 2.0, 5.0, 10.0],
                ),
            ),
        ] {
            self.by_contact_parameter
                .entry(parameter)
                .or_default()
                .entry(bin)
                .or_default()
                .observe(b.result, fast_low);
        }
    }
}

#[derive(Default, Serialize)]
struct VerticalLaunchTerms {
    selected: u64,
    missing_inputs: u64,
    baseline_mismatch: u64,
    reproduced: u64,
    base_attack_angle_sum_deg: f64,
    bat_angle_adjustment_sum_deg: f64,
    effective_attack_angle_sum_deg: f64,
    contact_deflection_sum_deg: f64,
    abs_contact_deflection_sum_deg: f64,
    launch_angle_sum_deg: f64,
    by_attack_angle_and_contact_deflection: BTreeMap<String, u64>,
}

impl VerticalLaunchTerms {
    fn observe(&mut self, b: &PlayerGameBatting, base_attack: Option<f64>) {
        self.selected += 1;
        let (Some(base), Some(bat), Some(z)) =
            (base_attack, b.actual_bat_angle_deg, b.contact_offset_z_m)
        else {
            self.missing_inputs += 1;
            return;
        };
        if ![base, bat, z, b.ball.launch_angle]
            .into_iter()
            .all(f64::is_finite)
        {
            self.missing_inputs += 1;
            return;
        }
        let attack = calculate_dynamic_attack_angle(base, bat);
        let deflection = calculate_vertical_contact_deflection(z);
        if (attack + deflection - b.ball.launch_angle).abs() > 1e-8 {
            self.baseline_mismatch += 1;
            return;
        }
        self.reproduced += 1;
        self.base_attack_angle_sum_deg += base;
        self.bat_angle_adjustment_sum_deg += attack - base;
        self.effective_attack_angle_sum_deg += attack;
        self.contact_deflection_sum_deg += deflection;
        self.abs_contact_deflection_sum_deg += deflection.abs();
        self.launch_angle_sum_deg += b.ball.launch_angle;
        let edges = &[
            -20.0, -10.0, -5.0, 0.0, 5.0, 7.0, 10.0, 15.0, 20.0, 25.0, 30.0,
        ];
        let attack_band = band(attack, edges);
        let deflection_band = band(deflection, edges);
        *self
            .by_attack_angle_and_contact_deflection
            .entry(format!(
                "attack_deg:{attack_band}|deflection_deg:{deflection_band}"
            ))
            .or_default() += 1;
    }
}

#[derive(Default, Serialize)]
pub(super) struct FairLaunchAudit {
    solid_contact_exit_speed_terms: ExitSpeedTermAudit,
    bat_speed_term_sweep: BatTermSweeps,
    home_run_flight_sweep: HomeRunFlightSweep,
    contact_flight_audit: ContactFlightAudit,
    vertical_launch_terms: BTreeMap<&'static str, VerticalLaunchTerms>,
    swing_decisions_missing: u64,
    recorded_swings: u64,
    swing_outcomes_by_adjusted_contact_type: BTreeMap<String, SwingOutcomes>,
    solid_contact_ev_by_bat_speed_and_contact_offsets:
        BTreeMap<String, BTreeMap<&'static str, ExitSpeedTotals>>,
    pub(super) home_runs: u64,
    pub(super) non_hr_in_play: u64,
    missing_exit_speed: u64,
    ev_ge_45_ms: u64,
    ev_ge_45_ms_missing_vla: u64,
    by_adjusted_contact_type: BTreeMap<String, ContactQualityLaunch>,
    ev_ge_45_ms_by_vla: BTreeMap<String, Outcomes>,
    ev_ge_45_ms_vla_7_to_10: Outcomes,
    ev_ge_45_ms_vla_7_to_10_by_ev_vla_hla: BTreeMap<String, Outcomes>,
}

impl FairLaunchAudit {
    pub(super) fn observe_launch_speed_terms(
        &mut self,
        schedule: &GameSchedule,
        game: &GameResult,
        geometry_scale: f64,
    ) {
        let batter_skills: BTreeMap<_, _> = schedule
            .away_team
            .players
            .iter()
            .chain(&schedule.home_team.players)
            .filter_map(|p| Some((p.info.id, p.offense_skills.batter.as_ref()?)))
            .collect();
        let pitches: BTreeMap<_, _> = game
            .player_pitchings
            .iter()
            .map(|p| ((p.count_seq, p.pitcher_id), p.ball.speed))
            .collect();
        let audit = &mut self.solid_contact_exit_speed_terms;
        let sweeps = &mut self.bat_speed_term_sweep;
        for b in &game.player_battings {
            let in_play = is_bip(b.result);
            let home_run = b.result == BattingResult::HomeRun;
            if !in_play && !home_run {
                continue;
            }
            let skills = batter_skills.get(&b.batter_id);
            let angle_cohort = if home_run {
                Some("home_runs")
            } else if is_fast_low(b) {
                Some(match b.signed_length_offset_m.filter(|x| x.is_finite()) {
                    Some(length) if length < 0.0 => "fast_low_negative_length",
                    Some(_) => "fast_low_nonnegative_length",
                    None => "fast_low_unknown_length",
                })
            } else {
                None
            };
            if let Some(cohort) = angle_cohort {
                self.vertical_launch_terms
                    .entry(cohort)
                    .or_default()
                    .observe(b, skills.map(|batter| batter.attack_angle));
            }
            self.home_run_flight_sweep.selected += u64::from(home_run);
            let length = b.length_offset_m.abs();
            let selected_solid = in_play
                && b.adjusted_contact_type == SwingContactType::SolidContact
                && (30.0..35.0).contains(&b.bat_speed)
                && (0.0..0.05).contains(&length);
            let mut groups = [
                Some(&mut sweeps.all_fair),
                in_play.then_some(&mut sweeps.non_hr_in_play),
                (!in_play).then_some(&mut sweeps.live_home_runs),
                selected_solid.then_some(&mut sweeps.selected_solid_contacts),
            ];
            for sweep in groups.iter_mut().flatten() {
                sweep.selected += 1;
            }
            audit.selected += u64::from(selected_solid);
            let depth = if geometry_scale == CONTACT_TIMING_POSITION_SCALE {
                b.timing_impact_x_m
            } else {
                b.adapted_timing_offset_sec
                    .map(|timing| b.bat_speed * timing * CONTACT_TIMING_POSITION_SCALE)
            };
            let (Some(&pitch), Some(power), Some(thickness), Some(depth)) = (
                pitches.get(&(b.count_seq, b.pitcher_id)),
                skills.map(|batter| batter.swing_power),
                b.thickness_offset_m,
                depth,
            ) else {
                for sweep in groups.iter_mut().flatten() {
                    sweep.missing_inputs += 1;
                }
                audit.missing_inputs += u64::from(selected_solid);
                continue;
            };
            if ![
                pitch,
                power,
                thickness,
                depth,
                b.bat_speed,
                length,
                b.ball.launch_speed,
            ]
            .into_iter()
            .all(f64::is_finite)
                || pitch <= 0.0
                || b.bat_speed <= 0.0
                || thickness < 0.0
                || b.ball.launch_speed <= 0.0
            {
                for sweep in groups.iter_mut().flatten() {
                    sweep.missing_inputs += 1;
                }
                audit.missing_inputs += u64::from(selected_solid);
                continue;
            }
            let replay = |power, depth, thickness, length| {
                launch_speed_terms(pitch, b.bat_speed, power, depth, thickness, length)
            };
            let terms = replay(power, depth, thickness, length);
            let observed = terms.exit_speed_ms();
            if (observed - b.ball.launch_speed).abs() > 1e-8 {
                for sweep in groups.iter_mut().flatten() {
                    sweep.baseline_mismatch += 1;
                }
                audit.baseline_mismatch += u64::from(selected_solid);
                continue;
            }
            for sweep in groups.into_iter().flatten() {
                sweep.observe(&terms, b.ball.launch_angle);
            }
            if home_run {
                self.home_run_flight_sweep
                    .observe(&terms, b, &schedule.stadium);
            }
            if !selected_solid {
                continue;
            }
            let neutral_power = replay(0.0, depth, thickness, length).exit_speed_ms();
            let zero_depth = replay(power, 0.0, thickness, length).exit_speed_ms();
            let centered = replay(power, depth, 0.0, 0.0).exit_speed_ms();

            let pitch_band = band(pitch, &[35.0, 38.0, 41.0, 44.0]);
            let power_band = band(power, &[-1.0, 0.0, 1.0, 2.0]);
            let bat_band = band(b.bat_speed, &[30.0, 32.5, 35.0]);
            let length_band = band(length, &[0.02, 0.05]);
            let thickness_band = band(thickness, &[0.01, 0.02, 0.025]);
            let cell = audit.by_input_bands.entry(format!(
                "pitch_speed_ms:{pitch_band}|swing_power:{power_band}|bat_speed_ms:{bat_band}|abs_length_m:{length_band}|abs_thickness_m:{thickness_band}"
            )).or_default();
            for group in [&mut audit.total, cell] {
                group.observed.observe(observed);
                group.neutral_swing_power.observe(neutral_power);
                group.zero_timing_depth.observe(zero_depth);
                group.centered_contact.observe(centered);
                group.pitch_term_sum_ms += terms.pitch_term_ms;
                group.swing_term_sum_ms += terms.swing_term_ms;
                group.thickness_retention_sum += terms.thickness_retention;
                group.length_retention_sum += terms.length_retention;
            }
        }
    }

    pub(super) fn observe(&mut self, game: &GameResult) {
        let direct_catches: BTreeSet<_> = game
            .player_fieldings
            .iter()
            .filter(|f| {
                f.seq == 1
                    && f.catch_diagnostics
                        .is_some_and(|catch| catch.catch_type == CatchType::DirectFly)
            })
            .map(|f| f.count_seq)
            .collect();
        for b in &game.player_battings {
            self.observe_swing(b);
            if b.result == BattingResult::HomeRun || is_bip(b.result) {
                self.contact_flight_audit.observe(b);
            }
            // The stored ball carries the launch parameters even on the early-return
            // paths for direct catches, home runs, and ground-rule doubles.
            if b.result == BattingResult::HomeRun {
                self.home_runs += 1;
            } else if is_bip(b.result) {
                let hla = (b.hla_model_version == Some(2)).then_some(b.final_hla_deg);
                self.observe_in_play(
                    b.ball.launch_speed,
                    b.ball.launch_angle,
                    hla,
                    b.adjusted_contact_type,
                    b.result,
                    direct_catches.contains(&b.count_seq),
                );
            }
        }
    }

    fn observe_swing(&mut self, b: &PlayerGameBatting) {
        let Some(decision) = b.swing_decision else {
            self.swing_decisions_missing += 1;
            return;
        };
        if !decision.swung {
            return;
        }
        self.recorded_swings += 1;
        let contact = self
            .swing_outcomes_by_adjusted_contact_type
            .entry(b.adjusted_contact_type.as_ref().into())
            .or_default();
        contact.swings += 1;
        let outcome = match b.result {
            BattingResult::Foul => {
                contact.fouls += 1;
                "foul"
            }
            BattingResult::HomeRun => {
                contact.home_runs += 1;
                "home_run"
            }
            result if is_bip(result) => {
                contact.non_hr_in_play += 1;
                "non_hr_in_play"
            }
            BattingResult::StrikeSwung | BattingResult::Strikeout => {
                contact.whiffs += 1;
                return;
            }
            _ => {
                contact.other += 1;
                return;
            }
        };
        if b.adjusted_contact_type == SwingContactType::SolidContact {
            let speed = if b.bat_speed > 0.0 {
                b.bat_speed
            } else {
                f64::NAN
            };
            let bat_speed = band(speed, &[30.0, 35.0, 37.5, 40.0, 42.5, 45.0]);
            let length = band(b.length_offset_m.abs(), &[0.02, 0.05, 0.10, 0.16]);
            let thickness = band(
                b.thickness_offset_m.map(f64::abs).unwrap_or(f64::NAN),
                &[0.005, 0.01, 0.02, 0.025, 0.04, 0.055, 0.07],
            );
            self.solid_contact_ev_by_bat_speed_and_contact_offsets
                .entry(format!(
                    "bat_speed_ms:{bat_speed}|abs_length_m:{length}|abs_thickness_m:{thickness}"
                ))
                .or_default()
                .entry(outcome)
                .or_default()
                .observe(b.ball.launch_speed);
        }
    }

    fn observe_in_play(
        &mut self,
        ev: f64,
        vla: f64,
        hla: Option<f64>,
        contact_type: SwingContactType,
        result: BattingResult,
        direct_catch: bool,
    ) {
        self.non_hr_in_play += 1;
        let contact = self
            .by_adjusted_contact_type
            .entry(format!("{contact_type:?}"))
            .or_default();
        contact.non_hr_in_play += 1;
        if !ev.is_finite() || ev <= 0.0 {
            self.missing_exit_speed += 1;
            contact.missing_exit_speed += 1;
            return;
        }
        contact.exit_speed_sum_ms += ev;
        if ev < HIGH_EV_MIN_MS {
            return;
        }
        self.ev_ge_45_ms += 1;
        contact.ev_ge_45_ms += 1;
        if !vla.is_finite() || !(-90.0..=90.0).contains(&vla) {
            self.ev_ge_45_ms_missing_vla += 1;
            return;
        }

        let angle = band(
            vla,
            &[-10.0, 0.0, 5.0, 6.0, 7.0, 8.0, 9.0, 10.0, 25.0, 50.0],
        );
        self.ev_ge_45_ms_by_vla
            .entry(angle.clone())
            .or_default()
            .observe(result, direct_catch);
        if TARGET_VLA.contains(&vla) {
            self.ev_ge_45_ms_vla_7_to_10.observe(result, direct_catch);
            let speed = band(ev, CONVERSION_EV_EDGES_MS);
            let direction = band(hla.unwrap_or(f64::NAN), CONVERSION_HLA_EDGES_DEG);
            self.ev_ge_45_ms_vla_7_to_10_by_ev_vla_hla
                .entry(format!("EV:{speed}|VLA:{angle}|HLA:{direction}"))
                .or_default()
                .observe(result, direct_catch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vertical_launch_terms_preserve_signed_deflection_and_reject_mismatches() {
        use saberbb::domain::shared::game_stats::PlayerGameBattingMetrics;
        let mut game = GameResult::new(1, chrono::NaiveDate::MIN, &[], &[]);
        let base = 12.0;
        let bat = 40.0;
        let z = -0.015;
        let deflection = calculate_vertical_contact_deflection(z);
        let metrics = PlayerGameBattingMetrics {
            actual_bat_angle_deg: Some(bat),
            contact_offset_z_m: Some(z),
            ..Default::default()
        };
        let ball = BattedBall {
            launch_angle: 15.5 + deflection,
            ..BattedBall::default()
        };
        game.add_player_batting(1, 1, 2, metrics, ball, None, BattingResult::Single);
        let mut terms = VerticalLaunchTerms::default();
        let batting = &game.player_battings[0];
        terms.observe(batting, Some(base));
        terms.observe(batting, Some(base + 1.0));
        terms.observe(batting, None);
        assert_eq!(
            (
                terms.selected,
                terms.reproduced,
                terms.baseline_mismatch,
                terms.missing_inputs
            ),
            (3, 1, 1, 1)
        );
        assert_eq!(terms.bat_angle_adjustment_sum_deg, 3.5);
        assert_eq!(terms.contact_deflection_sum_deg, deflection);
        assert!(terms.contact_deflection_sum_deg < 0.0);
    }

    #[test]
    fn home_run_sweep_reproduces_the_baseline_and_counts_a_lost_home_run() {
        use saberbb::domain::shared::game_stats::PlayerGameBattingMetrics;
        let stadium = Stadium::default();
        let (terms, ball) = (300..600)
            .find_map(|swing_tenths| {
                let terms = LaunchSpeedTerms {
                    pitch_term_ms: 7.0,
                    swing_term_ms: f64::from(swing_tenths) / 10.0,
                    thickness_retention: 1.0,
                    length_retention: 1.0,
                };
                let live =
                    calculate_trajectory(terms.exit_speed_ms(), 28.0, 0.0, 1800.0, 0.0, &stadium)
                        .ok()?;
                let reduced = calculate_trajectory(
                    scaled_exit_speed(&terms, 94),
                    28.0,
                    0.0,
                    1800.0,
                    0.0,
                    &stadium,
                )
                .ok()?;
                (live.outbound_result == OutboundResult::HomeRun
                    && reduced.outbound_result != OutboundResult::HomeRun)
                    .then_some((terms, live))
            })
            .expect("a marginal home run in the default stadium");
        let mut game = GameResult::new(1, chrono::NaiveDate::MIN, &[], &[]);
        let metrics = PlayerGameBattingMetrics {
            hla_model_version: Some(2),
            ..Default::default()
        };
        game.add_player_batting(1, 1, 2, metrics, ball, None, BattingResult::HomeRun);
        let mut sweep = HomeRunFlightSweep::default();
        sweep.observe(&terms, &game.player_battings[0], &stadium);
        assert_eq!(sweep.baseline_reproduced, 1);
        assert_eq!(sweep.trials[&100].home_runs, 1);
        assert_eq!(sweep.trials[&94].home_runs, 0);
        assert_eq!(sweep.trials[&94].replay_errors, 0);
    }

    #[test]
    fn bat_term_sweep_scales_only_the_swing_contribution() {
        let terms = LaunchSpeedTerms {
            pitch_term_ms: 8.0,
            swing_term_ms: 40.0,
            thickness_retention: 0.95,
            length_retention: 1.0,
        };
        let mut sweep = BatTermSweep::default();
        sweep.observe(&terms, 8.0);
        for (percent, expected, high) in [(100, 45.6, 1), (97, 44.46, 0), (94, 43.32, 0)] {
            let trial = &sweep.trials[&percent];
            assert!((trial.exit_speed.exit_speed_sum_ms - expected).abs() < 1e-10);
            assert_eq!(trial.ev_ge_45_ms_vla_7_to_10, high);
        }
    }

    #[test]
    fn swing_funnel_distinguishes_takes_and_swinging_strikeouts() {
        use saberbb::domain::{
            resolver::batting_resolver::{CountStatus, SwingDecisionDiagnostics},
            shared::{ball::BattedBall, game_stats::PlayerGameBattingMetrics},
        };
        let mut game = GameResult::new(1, chrono::NaiveDate::MIN, &[], &[]);
        let decision = SwingDecisionDiagnostics {
            count_status: CountStatus::C02,
            location_x: 0.0,
            location_y: 0.0,
            count_factor: 0.0,
            approach_factor: 0.0,
            protection_factor: 0.0,
            zone_factor: 0.0,
            pitch_factor: 0.0,
            anticipation_factor: 0.0,
            aptitude_factor: 0.0,
            score: 0.0,
            probability: 0.5,
            swung: true,
        };
        for (result, contact, swung) in [
            (BattingResult::Strikeout, SwingContactType::Take, false),
            (
                BattingResult::Strikeout,
                SwingContactType::SwungAndMiss,
                true,
            ),
            (BattingResult::Foul, SwingContactType::SolidContact, true),
            (BattingResult::Out, SwingContactType::SolidContact, true),
            (BattingResult::HomeRun, SwingContactType::SolidContact, true),
        ] {
            let metrics = PlayerGameBattingMetrics {
                adjusted_contact_type: contact,
                bat_speed: 40.0,
                length_offset_m: 0.03,
                thickness_offset_m: Some(0.007),
                ..PlayerGameBattingMetrics::default()
            };
            let ball = BattedBall {
                launch_speed: 45.0,
                launch_angle: 8.0,
                ..BattedBall::default()
            };
            game.add_player_batting(
                game.player_battings.len() as u16,
                1,
                2,
                metrics,
                ball,
                None,
                result,
            );
            game.player_battings.last_mut().unwrap().swing_decision =
                Some(SwingDecisionDiagnostics { swung, ..decision });
        }
        let mut audit = FairLaunchAudit::default();
        audit.observe(&game);
        let flights = &audit.contact_flight_audit;
        assert_eq!(flights.total.fair_balls, 2);
        assert_eq!(flights.total.home_runs, 1);
        assert_eq!(flights.total.fast_low_balls, 1);
        let launch = &flights.by_exit_speed_launch_angle_backspin["EV:[45,47.5)|VLA:[7,10)|backspin_rpm:[0,500)"];
        assert_eq!(launch.fair_balls, 2);
        assert_eq!(launch.fast_low_balls, 1);
        assert_eq!(
            flights.by_contact_parameter["signed_thickness_m"]["unknown"].fair_balls,
            2
        );
        assert_eq!(audit.recorded_swings, 4);
        assert_eq!(
            audit.swing_outcomes_by_adjusted_contact_type["SwungAndMiss"].whiffs,
            1
        );
        let solid = &audit.swing_outcomes_by_adjusted_contact_type["SolidContact"];
        assert_eq!(
            (
                solid.swings,
                solid.fouls,
                solid.non_hr_in_play,
                solid.home_runs
            ),
            (3, 1, 1, 1)
        );
        let cell = &audit.solid_contact_ev_by_bat_speed_and_contact_offsets["bat_speed_ms:[40,42.5)|abs_length_m:[0.02,0.05)|abs_thickness_m:[0.005,0.01)"];
        assert_eq!(cell["non_hr_in_play"].ev_ge_45_ms, 1);
        assert_eq!(cell["foul"].exit_speed_sum_ms, 45.0);
        assert_eq!(cell["home_run"].plays, 1);
    }

    #[test]
    fn keeps_direct_catch_outs_and_uses_half_open_launch_bands() {
        use SwingContactType::{MarginalContact, SolidContact, WeakContact};
        let mut audit = FairLaunchAudit::default();
        audit.observe_in_play(
            44.99,
            7.0,
            Some(0.0),
            WeakContact,
            BattingResult::Out,
            false,
        );
        audit.observe_in_play(45.0, 7.0, Some(0.0), SolidContact, BattingResult::Out, true);
        audit.observe_in_play(45.0, 9.99, None, SolidContact, BattingResult::Single, false);
        audit.observe_in_play(
            45.0,
            10.0,
            Some(0.0),
            SolidContact,
            BattingResult::Double,
            false,
        );
        audit.observe_in_play(
            45.0,
            f64::NAN,
            None,
            SolidContact,
            BattingResult::Out,
            false,
        );

        audit.observe_in_play(
            f64::NAN,
            7.0,
            None,
            MarginalContact,
            BattingResult::Out,
            false,
        );
        assert_eq!(audit.non_hr_in_play, 6);
        let solid = &audit.by_adjusted_contact_type["SolidContact"];
        assert_eq!(
            (
                solid.non_hr_in_play,
                solid.ev_ge_45_ms,
                solid.exit_speed_sum_ms
            ),
            (4, 4, 180.0)
        );
        let weak = &audit.by_adjusted_contact_type["WeakContact"];
        assert_eq!((weak.non_hr_in_play, weak.ev_ge_45_ms), (1, 0));
        let marginal = &audit.by_adjusted_contact_type["MarginalContact"];
        assert_eq!(
            (marginal.non_hr_in_play, marginal.missing_exit_speed),
            (1, 1)
        );
        assert_eq!(audit.ev_ge_45_ms, 4);
        assert_eq!(audit.ev_ge_45_ms_missing_vla, 1);
        let target = &audit.ev_ge_45_ms_vla_7_to_10;
        assert_eq!(target.plays, 2);
        assert_eq!(target.by_result["Single"], 1);
        assert_eq!(target.direct_catch_outs, 1);
        assert_eq!(
            audit.ev_ge_45_ms_vla_7_to_10_by_ev_vla_hla["EV:[45,50)|VLA:[7,8)|HLA:[-5,5)"]
                .direct_catch_outs,
            1
        );
    }
}
