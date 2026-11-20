use super::contact_first_ev_comparison::angle_band;
use super::{Args, GameSchedule, derived_seed, simulate_game};
use anyhow::{Context, Result, ensure};
use saberbb::domain::{
    random_provider::RealRng,
    resolver::batting_resolver::{
        contact_geometry_is_miss, launch_speed_terms, project_contact_offsets,
    },
    shared::game_state::{ContactFirstLaunch, with_contact_first_launch_audit},
    strategy::batting_strategy::BattingStrategySettings,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;

const REFERENCE_BAT_SPEED_MS: f64 = 32.5;
const HARD_EV_MS: f64 = 45.0;

struct Terms {
    inputs: [(&'static str, f64); 9],
    contributions: [(&'static str, f64); 7],
    alternatives: [(&'static str, Alternative); 7],
}

#[derive(Clone, Copy)]
enum Alternative {
    ExitSpeed(f64),
    GeometryMiss,
    MissingGeometry,
}

fn terms(launch: &ContactFirstLaunch) -> Result<Terms> {
    let pitch = launch.pitch_speed_ms;
    let bat = launch.bat_speed_ms;
    let power = launch.swing_power;
    let contact = &launch.contact;
    let depth = contact.timing_impact_x_m;
    let thickness = contact.thickness_offset_m;
    let length = contact.length_offset_m;
    let speed = |bat, power, depth, thickness, length| {
        launch_speed_terms(pitch, bat, power, depth, thickness, length).exit_speed_ms()
    };
    let live = launch_speed_terms(pitch, bat, power, depth, thickness, length);
    let observed = live.exit_speed_ms();
    ensure!(
        observed.is_finite() && (observed - launch.exit_speed_ms).abs() <= 1e-8,
        "exit-speed formula mismatch at count {}: {}, expected {}",
        launch.count_seq,
        observed,
        launch.exit_speed_ms
    );

    let reference = launch_speed_terms(pitch, REFERENCE_BAT_SPEED_MS, 0.0, 0.0, 0.0, 0.0);
    let bat_only = speed(bat, 0.0, 0.0, 0.0, 0.0);
    let with_power = speed(bat, power, 0.0, 0.0, 0.0);
    let with_depth = speed(bat, power, depth, 0.0, 0.0);
    let with_thickness = speed(bat, power, depth, thickness, 0.0);
    let contributions = [
        ("pitch", reference.pitch_term_ms),
        ("reference_bat", reference.swing_term_ms),
        ("bat_speed_delta", bat_only - reference.exit_speed_ms()),
        ("swing_power_delta", with_power - bat_only),
        ("timing_depth_delta", with_depth - with_power),
        ("thickness_loss", with_thickness - with_depth),
        ("length_loss_after_thickness", observed - with_thickness),
    ];

    let timing_geometry = |depth| {
        let Some(bat_angle) = contact
            .actual_bat_angle_deg
            .filter(|angle| angle.is_finite())
        else {
            return Alternative::MissingGeometry;
        };
        let (length, thickness) = project_contact_offsets(
            contact.offset_x_m - contact.timing_impact_x_m,
            contact.offset_z_m,
            bat_angle,
        );
        let (length, thickness) = (length.abs(), thickness.abs());
        if contact_geometry_is_miss(length, thickness) {
            Alternative::GeometryMiss
        } else {
            Alternative::ExitSpeed(speed(bat, power, depth, thickness, length))
        }
    };
    let ev = Alternative::ExitSpeed;
    let alternatives = [
        (
            "reference_bat_speed",
            ev(speed(
                REFERENCE_BAT_SPEED_MS,
                power,
                depth,
                thickness,
                length,
            )),
        ),
        (
            "neutral_swing_power",
            ev(speed(bat, 0.0, depth, thickness, length)),
        ),
        (
            "zero_timing_depth_only",
            ev(speed(bat, power, 0.0, thickness, length)),
        ),
        ("zero_timing_geometry_only", timing_geometry(depth)),
        ("zero_timing_both", timing_geometry(0.0)),
        (
            "no_thickness_loss",
            ev(speed(bat, power, depth, 0.0, length)),
        ),
        (
            "no_length_loss",
            ev(speed(bat, power, depth, thickness, 0.0)),
        ),
    ];
    Ok(Terms {
        inputs: [
            ("pitch_speed_ms", pitch),
            ("bat_speed_ms", bat),
            ("swing_power", power),
            ("timing_offset_ms", contact.timing_offset_sec * 1000.0),
            ("timing_impact_x_cm", depth * 100.0),
            ("thickness_offset_cm", thickness * 100.0),
            ("length_offset_cm", length * 100.0),
            ("thickness_retention", live.thickness_retention),
            ("length_retention", live.length_retention),
        ],
        contributions,
        alternatives,
    })
}

#[derive(Default, Serialize)]
struct Sensitivity {
    contact_samples: u64,
    hypothetical_geometric_misses: u64,
    missing_geometry: u64,
    sum_difference_ms: f64,
    ev_at_least_45: u64,
    crossed_below_45: u64,
    crossed_to_45_or_above: u64,
}

impl Sensitivity {
    fn observe(&mut self, alternative: Alternative, reference: f64) {
        let speed = match alternative {
            Alternative::ExitSpeed(speed) => speed,
            Alternative::GeometryMiss => {
                self.hypothetical_geometric_misses += 1;
                return;
            }
            Alternative::MissingGeometry => {
                self.missing_geometry += 1;
                return;
            }
        };
        self.contact_samples += 1;
        self.sum_difference_ms += speed - reference;
        self.ev_at_least_45 += u64::from(speed >= HARD_EV_MS);
        self.crossed_below_45 += u64::from(reference >= HARD_EV_MS && speed < HARD_EV_MS);
        self.crossed_to_45_or_above += u64::from(reference < HARD_EV_MS && speed >= HARD_EV_MS);
    }

    fn report(&self) -> Value {
        let mut report = json!(self);
        report["mean_difference_ms"] = json!(
            (self.contact_samples > 0)
                .then(|| self.sum_difference_ms / self.contact_samples as f64)
        );
        report
    }
}

#[derive(Default)]
struct Profile {
    launches: u64,
    ev_at_least_45: u64,
    thickness_retention_is_one: u64,
    exit_speed_sum_ms: f64,
    input_sums: BTreeMap<&'static str, f64>,
    contribution_sums: BTreeMap<&'static str, f64>,
    alternatives: BTreeMap<&'static str, Sensitivity>,
}

impl Profile {
    fn observe(&mut self, launch: &ContactFirstLaunch, terms: &Terms) {
        self.launches += 1;
        self.ev_at_least_45 += u64::from(launch.exit_speed_ms >= HARD_EV_MS);
        self.exit_speed_sum_ms += launch.exit_speed_ms;
        for (name, value) in terms.inputs {
            *self.input_sums.entry(name).or_default() += value;
            self.thickness_retention_is_one +=
                u64::from(name == "thickness_retention" && value == 1.0);
        }
        for (name, value) in terms.contributions {
            *self.contribution_sums.entry(name).or_default() += value;
        }
        for (name, alternative) in terms.alternatives {
            self.alternatives
                .entry(name)
                .or_default()
                .observe(alternative, launch.exit_speed_ms);
        }
    }

    fn report(&self) -> Value {
        let mean = |value| (self.launches != 0).then(|| value / self.launches as f64);
        let means = |sums: &BTreeMap<&'static str, f64>| {
            sums.iter()
                .map(|(&key, &value)| (key, mean(value)))
                .collect::<BTreeMap<_, _>>()
        };
        json!({
            "launches": self.launches, "ev_at_least_45": self.ev_at_least_45,
            "ev_at_least_45_rate": mean(self.ev_at_least_45 as f64),
            "thickness_retention_is_one": self.thickness_retention_is_one,
            "mean_exit_speed_ms": mean(self.exit_speed_sum_ms),
            "mean_inputs": means(&self.input_sums),
            "mean_ordered_contributions_ms": means(&self.contribution_sums),
            "one_at_a_time": self.alternatives.iter().map(|(&key, stats)| (key, stats.report()))
                .collect::<BTreeMap<_, _>>(),
        })
    }
}

#[derive(Default)]
struct Band {
    all: Profile,
    hard: Profile,
}

impl Band {
    fn observe(&mut self, launch: &ContactFirstLaunch, terms: &Terms) {
        self.all.observe(launch, terms);
        if launch.exit_speed_ms >= HARD_EV_MS {
            self.hard.observe(launch, terms);
        }
    }

    fn report(&self) -> Value {
        json!({"all":self.all.report(),"baseline_ev_at_least_45":self.hard.report()})
    }
}

#[derive(Default)]
struct Report {
    all: Band,
    target: Band,
    bands: BTreeMap<&'static str, Band>,
}

impl Report {
    fn observe(&mut self, launch: &ContactFirstLaunch, terms: &Terms) {
        let angle = launch.launch_angle_deg;
        self.all.observe(launch, terms);
        if (5.0..10.0).contains(&angle) {
            self.target.observe(launch, terms);
        }
        self.bands
            .entry(angle_band(angle))
            .or_default()
            .observe(launch, terms);
    }

    fn report(&self) -> Value {
        json!({
            "all_angles": self.all.report(), "target_5_to_10_deg": self.target.report(),
            "by_launch_angle_deg":self.bands.iter().map(|(&key, value)| (key,value.report()))
                .collect::<BTreeMap<_, _>>(),
        })
    }
}

pub(super) fn run(args: &Args, games: &[GameSchedule]) -> Result<Value> {
    let mut total = Report::default();
    let mut reports = Vec::new();
    for &seed in &args.seeds {
        let mut report = Report::default();
        for game in games {
            let (result, launches) = with_contact_first_launch_audit(|| {
                simulate_game(
                    game.clone(),
                    Box::new(RealRng::from_seed(derived_seed(seed, game.id))),
                    BattingStrategySettings::player_driven(),
                    Some(args.max_pitches),
                    false,
                )
            });
            result.with_context(|| {
                format!(
                    "ContactFirst launch capture failed in game {}, seed {seed}",
                    game.id
                )
            })?;
            for launch in launches {
                let terms =
                    terms(&launch).with_context(|| format!("game {}, seed {seed}", game.id))?;
                total.observe(&launch, &terms);
                report.observe(&launch, &terms);
            }
        }
        eprintln!(
            "ContactFirst exit-speed terms, seed {seed}: {} games complete",
            games.len()
        );
        reports.push(json!({"seed":seed.to_string(),"summary":report.report()}));
    }
    Ok(json!({
        "definitions": {
            "population":"All baseline fair ContactFirst launches, including launches with no eventual batting ruling. All comparisons keep the original angle and cohort.",
            "ordered_contributions":"Pitch and reference bat at neutral power and depth, followed by the actual bat speed, power, timing depth, thickness and length in that order. These signed terms add to live EV. Interaction effects depend on this order and reference.",
            "one_at_a_time":"Change one factor in the live EV formula. Neutral swing power means the input value zero. Removing timing from geometry subtracts its X displacement, then reprojects onto the bat axes. Geometry misses and unavailable inputs are counted separately from threshold crossings. These are launch-speed sensitivities at fixed contact selection, without outcome or trajectory replay.",
        },
        "reference_bat_speed_ms": REFERENCE_BAT_SPEED_MS,
        "hard_exit_speed_ms": HARD_EV_MS,
        "pooled": total.report(),
        "reports": reports,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use saberbb::domain::resolver::batting_resolver::SwingContactResult;

    #[test]
    fn ordered_terms_reconstruct_live_ev_and_keep_timing_routes_separate() {
        let launch = ContactFirstLaunch {
            count_seq: 9,
            exit_speed_ms: launch_speed_terms(40.0, 34.0, 1.0, 0.1, 0.025, 0.035).exit_speed_ms(),
            launch_angle_deg: 7.0,
            pitch_speed_ms: 40.0,
            bat_speed_ms: 34.0,
            swing_power: 1.0,
            contact: SwingContactResult {
                actual_bat_angle_deg: Some(0.0),
                timing_impact_x_m: 0.1,
                offset_x_m: 0.035,
                offset_z_m: 0.025,
                thickness_offset_m: 0.025,
                length_offset_m: 0.035,
                ..SwingContactResult::default()
            },
        };
        let result = terms(&launch).unwrap();
        let sum: f64 = result.contributions.iter().map(|(_, value)| value).sum();
        assert!((sum - launch.exit_speed_ms).abs() < 1e-12);
        let alternatives: BTreeMap<_, _> = result.alternatives.into_iter().collect();
        for (name, depth, length) in [
            ("zero_timing_depth_only", 0.0, 0.035),
            ("zero_timing_geometry_only", 0.1, 0.065),
            ("zero_timing_both", 0.0, 0.065),
        ] {
            let Alternative::ExitSpeed(speed) = alternatives[name] else {
                panic!("expected a contact")
            };
            let expected =
                launch_speed_terms(40.0, 34.0, 1.0, depth, 0.025, length).exit_speed_ms();
            assert!((expected - speed).abs() < 1e-12);
        }
    }

    #[test]
    fn threshold_crossings_keep_geometric_misses_and_missing_inputs_separate() {
        let mut test = Sensitivity::default();
        test.observe(Alternative::ExitSpeed(44.5), 45.0);
        test.observe(Alternative::ExitSpeed(45.0), 44.0);
        test.observe(Alternative::GeometryMiss, 50.0);
        test.observe(Alternative::MissingGeometry, 50.0);
        assert_eq!(test.contact_samples, 2);
        assert_eq!(test.crossed_below_45, 1);
        assert_eq!(test.crossed_to_45_or_above, 1);
        assert_eq!(test.hypothetical_geometric_misses, 1);
        assert_eq!(test.missing_geometry, 1);
    }
}
