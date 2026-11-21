use super::outcome_stage;
use saberbb::domain::{
    resolver::batting_resolver::{
        CONTACT_LENGTH_LIMIT_M, CONTACT_THICKNESS_LIMIT_M, calculate_vertical_contact_deflection,
        project_contact_offsets, vertical_offset_boundary_to_exceed_deflection,
    },
    shared::game_state::{SwingReplay, SwingReplayLaunchGeometry},
    strategy::batting_strategy::BattedBallIntent,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

pub(super) const HIGH_VLA_DEG: f64 = 50.0;

fn axis_contact_range(intercept: f64, slope: f64, limit: f64) -> Option<(f64, f64)> {
    if slope == 0.0 {
        return (intercept.abs() <= limit).then_some((f64::NEG_INFINITY, f64::INFINITY));
    }
    let first = (-limit - intercept) / slope;
    let second = (limit - intercept) / slope;
    Some((first.min(second), first.max(second)))
}

#[derive(Debug, PartialEq)]
enum Reachability {
    NoContactInterval,
    ThicknessLimit,
    LengthLimit,
    Reachable {
        threshold_gap_m: f64,
        contact_gap_m: f64,
    },
}

fn reachability(
    geometry: SwingReplayLaunchGeometry,
    current_z_m: f64,
    boundary: f64,
) -> Reachability {
    let (length_at_zero, thickness_at_zero) =
        project_contact_offsets(geometry.contact_x_m, 0.0, geometry.bat_angle_deg);
    let (length_slope, thickness_slope) = project_contact_offsets(0.0, 1.0, geometry.bat_angle_deg);
    let Some((length_min, length_max)) =
        axis_contact_range(length_at_zero, length_slope, CONTACT_LENGTH_LIMIT_M)
    else {
        return Reachability::NoContactInterval;
    };
    let Some((thickness_min, thickness_max)) = axis_contact_range(
        thickness_at_zero,
        thickness_slope,
        CONTACT_THICKNESS_LIMIT_M,
    ) else {
        return Reachability::NoContactInterval;
    };
    let min = length_min.max(thickness_min);
    let max = length_max.min(thickness_max);
    if min > max {
        return Reachability::NoContactInterval;
    }
    // The contact interval is closed; the high-VLA region starts strictly above the boundary.
    if max <= boundary || max < current_z_m {
        return if thickness_max <= length_max {
            Reachability::ThicknessLimit
        } else {
            Reachability::LengthLimit
        };
    }
    Reachability::Reachable {
        threshold_gap_m: (boundary - current_z_m).max(0.0),
        contact_gap_m: (boundary.max(min) - current_z_m).max(0.0),
    }
}

#[derive(Default)]
struct HighLaunches {
    swings: u64,
    by_abs_hla: [u64; 4],
    attack_angle_sum_deg: f64,
    contact_z_sum_m: f64,
}

impl HighLaunches {
    fn observe(&mut self, play: SwingReplay, geometry: SwingReplayLaunchGeometry) {
        self.swings += 1;
        self.attack_angle_sum_deg += geometry.attack_angle_deg;
        self.contact_z_sum_m += play.contact_z_m;
        let index = match play
            .launch_hla_deg
            .filter(|angle| angle.is_finite())
            .map(f64::abs)
        {
            Some(angle) if angle <= 30.0 => 0,
            Some(angle) if angle <= 45.0 => 1,
            Some(_) => 2,
            None => 3,
        };
        self.by_abs_hla[index] += 1;
    }

    fn report(&self) -> Value {
        let mean = |sum: f64| (self.swings > 0).then(|| sum / self.swings as f64);
        let by_abs_hla: BTreeMap<_, _> = ["at_most_30", "over_30_to_45", "over_45", "missing"]
            .into_iter()
            .zip(self.by_abs_hla)
            .collect();
        json!({
            "swings": self.swings,
            "by_absolute_launch_hla_deg": by_abs_hla,
            "mean_attack_angle_deg": mean(self.attack_angle_sum_deg),
            "mean_world_vertical_offset_mm": mean(self.contact_z_sum_m * 1000.0),
        })
    }
}

const GAP_BANDS: [&str; 4] = ["at_most_5", "over_5_to_10", "over_10_to_20", "over_20"];

fn gap_band(gap_m: f64) -> usize {
    if gap_m <= 0.005 {
        0
    } else if gap_m <= 0.010 {
        1
    } else if gap_m <= 0.020 {
        2
    } else {
        3
    }
}

#[derive(Default)]
struct ReachabilityCounts {
    swings: u64,
    missing_geometry: u64,
    modeled_already_above: u64,
    modeled_already_above_without_launch: u64,
    below_deflection_cap: u64,
    below_no_contact_interval: u64,
    below_thickness_limit: u64,
    below_length_limit: u64,
    below_reachable: u64,
    reachable_already_contacting: u64,
    by_required_gap: [u64; GAP_BANDS.len()],
    threshold_gap_sum_m: f64,
    feasible_gap_sum_m: f64,
    feasible_by_gap: [u64; GAP_BANDS.len()],
    high_launches_by_outcome: BTreeMap<&'static str, HighLaunches>,
}

impl ReachabilityCounts {
    fn observe(&mut self, play: SwingReplay) {
        self.swings += 1;
        let Some(geometry) = play.launch_geometry else {
            self.missing_geometry += 1;
            return;
        };
        if play.flight.is_some_and(|(_, _, vla)| vla > HIGH_VLA_DEG) {
            self.high_launches_by_outcome
                .entry(outcome_stage(play))
                .or_default()
                .observe(play, geometry);
        }
        let modeled_vla =
            geometry.attack_angle_deg + calculate_vertical_contact_deflection(play.contact_z_m);
        if modeled_vla > HIGH_VLA_DEG {
            self.modeled_already_above += 1;
            self.modeled_already_above_without_launch += u64::from(play.flight.is_none());
            return;
        }
        let Some(boundary) =
            vertical_offset_boundary_to_exceed_deflection(HIGH_VLA_DEG - geometry.attack_angle_deg)
        else {
            self.below_deflection_cap += 1;
            return;
        };
        self.by_required_gap[gap_band((boundary - play.contact_z_m).max(0.0))] += 1;
        match reachability(geometry, play.contact_z_m, boundary) {
            Reachability::NoContactInterval => self.below_no_contact_interval += 1,
            Reachability::ThicknessLimit => self.below_thickness_limit += 1,
            Reachability::LengthLimit => self.below_length_limit += 1,
            Reachability::Reachable {
                threshold_gap_m,
                contact_gap_m,
            } => {
                self.below_reachable += 1;
                self.threshold_gap_sum_m += threshold_gap_m;
                self.feasible_gap_sum_m += contact_gap_m;
                self.reachable_already_contacting += u64::from(play.flight.is_some());
                self.feasible_by_gap[gap_band(contact_gap_m)] += 1;
            }
        }
    }

    fn report(&self) -> Value {
        let mean_mm = |sum: f64| {
            (self.below_reachable > 0).then(|| sum * 1000.0 / self.below_reachable as f64)
        };
        let gap_counts = |counts| {
            GAP_BANDS
                .into_iter()
                .zip(counts)
                .collect::<BTreeMap<_, _>>()
        };
        let launches: BTreeMap<_, _> = self
            .high_launches_by_outcome
            .iter()
            .map(|(&name, counts)| (name, counts.report()))
            .collect();
        json!({
            "swings": self.swings,
            "missing_geometry": self.missing_geometry,
            "modeled_above_at_original_z": self.modeled_already_above,
            "modeled_above_at_original_z_without_launch": self.modeled_already_above_without_launch,
            "at_or_below_threshold": {
                "deflection_cap": self.below_deflection_cap,
                "no_contact_interval_at_fixed_x": self.below_no_contact_interval,
                "thickness_limit": self.below_thickness_limit,
                "length_limit": self.below_length_limit,
                "reachable_with_contact": self.below_reachable,
                "mean_threshold_gap_mm_when_reachable": mean_mm(self.threshold_gap_sum_m),
                "mean_feasible_gap_mm_when_reachable": mean_mm(self.feasible_gap_sum_m),
                "required_gap_mm_before_contact_limits": gap_counts(self.by_required_gap),
                "feasible_gap_mm": gap_counts(self.feasible_by_gap),
                "reachable_from_existing_contact": self.reachable_already_contacting,
            },
            "actual_above_threshold_by_outcome": launches,
        })
    }
}

#[derive(Default)]
pub(super) struct HighVlaReachability {
    all: ReachabilityCounts,
    contact_first: ReachabilityCounts,
    line_drive: ReachabilityCounts,
}

impl HighVlaReachability {
    pub(super) fn observe(&mut self, intent: BattedBallIntent, play: SwingReplay) {
        self.all.observe(play);
        match intent {
            BattedBallIntent::ContactFirst => self.contact_first.observe(play),
            BattedBallIntent::LineDrive => self.line_drive.observe(play),
        }
    }

    pub(super) fn report(&self) -> Value {
        json!({
            "all": self.all.report(),
            "contact_first": self.contact_first.report(),
            "line_drive": self.line_drive.report(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use saberbb::domain::resolver::batting_resolver::contact_geometry_is_miss;

    #[test]
    fn solves_the_strict_launch_boundary_and_checks_both_contact_axes() {
        let attack_angle_deg = 15.0;
        let boundary =
            vertical_offset_boundary_to_exceed_deflection(50.0 - attack_angle_deg).unwrap();
        assert!(
            (calculate_vertical_contact_deflection(boundary) + attack_angle_deg - 50.0).abs()
                < 1e-10
        );
        assert!(calculate_vertical_contact_deflection(boundary + 1e-8) + attack_angle_deg > 50.0);
        assert_eq!(vertical_offset_boundary_to_exceed_deflection(54.0), None);
        assert_eq!(
            vertical_offset_boundary_to_exceed_deflection(-55.0),
            Some(f64::NEG_INFINITY)
        );

        let geometry = |x, bat_angle_deg| SwingReplayLaunchGeometry {
            contact_x_m: x,
            bat_angle_deg,
            attack_angle_deg,
        };
        let Reachability::Reachable {
            threshold_gap_m,
            contact_gap_m,
        } = reachability(geometry(0.0, 30.0), 0.05, boundary)
        else {
            panic!("expected a reachable high launch")
        };
        assert!((threshold_gap_m - 0.00958).abs() < 0.00001);
        assert_eq!(threshold_gap_m, contact_gap_m);
        let (length, thickness) = project_contact_offsets(0.0, 0.05 + contact_gap_m + 1e-8, 30.0);
        assert!(!contact_geometry_is_miss(length, thickness));
        assert_eq!(
            reachability(geometry(-0.05, 30.0), 0.05, boundary),
            Reachability::ThicknessLimit
        );
        assert_eq!(
            reachability(geometry(0.165, 30.0), 0.05, boundary),
            Reachability::LengthLimit
        );
        assert_eq!(
            reachability(geometry(0.20, 0.0), 0.05, boundary),
            Reachability::NoContactInterval
        );
    }
}
