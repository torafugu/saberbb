//! Full-game seeded offense diagnostics; reads SABERBB_DB and never saves game results.
use anyhow::{Context, Result, ensure};
use clap::Parser;
use rusqlite::{Connection, OpenFlags};
use saberbb::{
    I18nManager,
    domain::{
        game_service::simulate_game,
        random_provider::RealRng,
        resolver::batting_resolver::{
            CONTACT_TIMING_POSITION_SCALE, SwingContactType, with_contact_geometry_scale,
        },
        resolver::fielding_physics::{
            CatchType, with_direct_catch_margin_scale, with_pre_fence_pickup_trial,
        },
        seeded_diagnostic::derived_seed,
        shared::{
            ball::{BattedBall, OutboundResult},
            game::{BattingResult, GameResult, GameSchedule},
            game_state::{
                DEFAULT_GROUND_RULE_DOUBLE_CATCH_ENABLED, DEFAULT_LINE_DRIVE_LIFT_OFFSET_MM,
                FenceAudit, LineDriveLiftStats, PairedContactPlay, Ruling,
                VERTICAL_CONTACT_WINDOW_EV_SPLIT_MS, VERTICAL_CONTACT_WINDOW_MM,
                with_ground_rule_double_catch_trial, with_line_drive_lift_trial,
                with_paired_contact_fielding, with_right_field_first_trial,
                with_vertical_contact_trial, with_vertical_contact_window_ev_trial,
                with_vertical_contact_window_trial,
            },
            infield_opportunity_audit::InfieldOpportunityAudit,
            low_line_drive_fielding_audit::LowLineDriveFieldingAudit,
            no_bounce_catch_audit::NoBounceCatchAudit,
            outfield_advance_audit::OutfieldAdvanceAudit,
            team::Team,
            timing_scale_audit::TimingScaleAudit,
        },
        strategy::batting_strategy::BattingStrategySettings,
    },
    repositories::{
        batting_personnel_repository::load_team_manager,
        db::FromRow,
        game_repository::{GamePlayerReader, SqlGameRepository},
    },
};
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::PathBuf};

#[path = "support/bip_audit.rs"]
mod bip_audit;
#[path = "support/contact_direction_audit.rs"]
mod contact_direction_audit;
#[path = "support/contact_first_ev_comparison.rs"]
mod contact_first_ev_comparison;
#[path = "support/contact_first_ev_terms.rs"]
mod contact_first_ev_terms;
#[path = "support/count_intent_comparison.rs"]
mod count_intent_comparison;
#[path = "support/fair_launch_audit.rs"]
mod fair_launch_audit;
#[path = "support/intent_aim_comparison.rs"]
mod intent_aim_comparison;
#[path = "support/moving_collision_audit.rs"]
mod moving_collision_audit;
#[path = "support/pickup_timing_audit.rs"]
mod pickup_timing_audit;
#[path = "support/selected_intent_audit.rs"]
mod selected_intent_audit;
#[path = "support/swing_speed_comparison.rs"]
mod swing_speed_comparison;
#[path = "support/throw_origin_audit.rs"]
mod throw_origin_audit;
#[path = "support/vertical_contact_game_sweep.rs"]
mod vertical_contact_game_sweep;
#[path = "support/vertical_contact_replay.rs"]
mod vertical_contact_replay;

#[derive(Parser)]
struct Args {
    #[arg(long)]
    season: u16,
    /// Replay one batter's simulated swings at alternate speeds on the same pitches.
    #[arg(long, conflicts_with_all = [
        "vertical_contact_sweep", "vertical_contact_comparison", "vertical_contact_paired_fielding",
        "line_drive_lift_comparison", "timing_geometry_sweep", "right_field_first_comparison",
        "no_bounce_margin_comparison", "pre_fence_pickup_comparison"
    ])]
    swing_speed_batter_id: Option<i64>,
    /// Alternate swing speeds in m/s, comma separated. Defaults to 33.72,34.5,35.5.
    #[arg(long, value_delimiter = ',', requires = "swing_speed_batter_id")]
    swing_speed_ms: Vec<f64>,
    /// Compare 0, +10, +15 and +20 mm for swings with the existing LineDrive intent.
    #[arg(long, conflicts_with_all = ["timing_geometry_sweep", "right_field_first_comparison", "no_bounce_margin_comparison"])]
    line_drive_lift_comparison: bool,
    /// Comma-separated vertical offsets in mm; defaults to 0,10,15,20.
    #[arg(long, value_delimiter = ',', requires = "line_drive_lift_comparison")]
    line_drive_lift_offsets: Vec<u16>,
    /// Compare intent-specific vertical aims using paired swings and full games.
    #[arg(long, conflicts_with_all = [
        "swing_speed_batter_id", "line_drive_lift_comparison", "vertical_contact_sweep",
        "vertical_contact_comparison", "vertical_contact_paired_fielding", "timing_geometry_sweep",
        "right_field_first_comparison", "no_bounce_margin_comparison", "pre_fence_pickup_comparison",
        "bip_diagnostics", "fair_launch_diagnostics", "contact_direction_diagnostics",
        "moving_collision_diagnostics", "selected_intent_diagnostics", "timing_scale_diagnostics",
        "infield_opportunity_diagnostics", "outfield_advance_diagnostics", "no_bounce_catch_diagnostics"
    ])]
    intent_aim_comparison: bool,
    /// Alternate absolute ContactFirst aim in mm. Defaults to +8 mm.
    #[arg(long, requires = "intent_aim_comparison", allow_negative_numbers = true)]
    contact_first_aim_mm: Option<i16>,
    /// Alternate absolute LineDrive aim in mm. Defaults to +32 mm.
    #[arg(long, requires = "intent_aim_comparison", allow_negative_numbers = true)]
    line_drive_aim_mm: Option<i16>,
    /// Replay hard fair ContactFirst swings at lower exit speeds with the same launch angles and spin.
    #[arg(long, conflicts_with_all = [
        "intent_aim_comparison", "swing_speed_batter_id", "line_drive_lift_comparison",
        "vertical_contact_sweep", "vertical_contact_comparison", "vertical_contact_paired_fielding",
        "timing_geometry_sweep", "right_field_first_comparison", "no_bounce_margin_comparison",
        "pre_fence_pickup_comparison", "bip_diagnostics", "fair_launch_diagnostics",
        "contact_direction_diagnostics", "moving_collision_diagnostics", "selected_intent_diagnostics",
        "timing_scale_diagnostics", "infield_opportunity_diagnostics", "outfield_advance_diagnostics",
        "no_bounce_catch_diagnostics"
    ])]
    contact_first_ev_comparison: bool,
    /// Positive exit-speed reductions in m/s, comma separated. Defaults to 2,4,6.
    #[arg(long, value_delimiter = ',', requires = "contact_first_ev_comparison")]
    exit_speed_reductions_ms: Vec<f64>,
    /// Decompose the live exit speed of fair ContactFirst launches.
    #[arg(long, conflicts_with_all = [
        "contact_first_ev_comparison", "intent_aim_comparison", "swing_speed_batter_id",
        "line_drive_lift_comparison", "vertical_contact_sweep", "vertical_contact_comparison",
        "vertical_contact_paired_fielding", "timing_geometry_sweep", "right_field_first_comparison",
        "no_bounce_margin_comparison", "pre_fence_pickup_comparison", "bip_diagnostics",
        "fair_launch_diagnostics", "contact_direction_diagnostics", "moving_collision_diagnostics",
        "selected_intent_diagnostics", "timing_scale_diagnostics", "infield_opportunity_diagnostics",
        "outfield_advance_diagnostics", "no_bounce_catch_diagnostics"
    ])]
    contact_first_ev_terms: bool,
    /// Historical drag control; applies to gameplay and shadow flights on this thread.
    #[arg(long)]
    legacy_flight_drag: bool,
    /// First N scheduled games, regardless of saved completion; omit for the full season.
    #[arg(long)]
    games: Option<usize>,
    #[arg(long, value_delimiter = ',', default_value = "7,42,20261108")]
    seeds: Vec<u64>,
    /// Replay the same swings with ContactFirst at 100%, 97%, and 94% of player bat speed.
    #[arg(long, conflicts_with_all = [
        "contact_first_ev_terms", "contact_first_ev_comparison", "intent_aim_comparison", "swing_speed_batter_id",
        "line_drive_lift_comparison", "vertical_contact_sweep", "vertical_contact_comparison",
        "vertical_contact_paired_fielding", "timing_geometry_sweep", "right_field_first_comparison",
        "no_bounce_margin_comparison", "pre_fence_pickup_comparison", "bip_diagnostics",
        "fair_launch_diagnostics", "contact_direction_diagnostics", "moving_collision_diagnostics",
        "selected_intent_diagnostics", "timing_scale_diagnostics", "infield_opportunity_diagnostics",
        "outfield_advance_diagnostics", "no_bounce_catch_diagnostics"
    ])]
    contact_first_swing_speed_comparison: bool,
    /// Replay fixed contacts with added length-direction loss on both intents.
    #[arg(long, conflicts_with_all = [
        "contact_first_swing_speed_comparison", "contact_first_ev_terms", "contact_first_ev_comparison", "intent_aim_comparison", "swing_speed_batter_id",
        "line_drive_lift_comparison", "vertical_contact_sweep", "vertical_contact_comparison",
        "vertical_contact_paired_fielding", "timing_geometry_sweep", "right_field_first_comparison",
        "no_bounce_margin_comparison", "pre_fence_pickup_comparison", "bip_diagnostics",
        "fair_launch_diagnostics", "contact_direction_diagnostics", "moving_collision_diagnostics",
        "selected_intent_diagnostics", "timing_scale_diagnostics", "infield_opportunity_diagnostics",
        "outfield_advance_diagnostics", "no_bounce_catch_diagnostics"
    ])]
    contact_length_comparison: bool,
    /// Compare ContactFirst lift and undercut with and without vertical timing geometry.
    #[arg(long, conflicts_with_all = [
        "contact_length_comparison", "contact_first_swing_speed_comparison", "contact_first_ev_terms", "contact_first_ev_comparison", "intent_aim_comparison", "swing_speed_batter_id",
        "line_drive_lift_comparison", "vertical_contact_sweep", "vertical_contact_comparison",
        "vertical_contact_paired_fielding", "timing_geometry_sweep", "right_field_first_comparison",
        "no_bounce_margin_comparison", "pre_fence_pickup_comparison", "bip_diagnostics",
        "fair_launch_diagnostics", "contact_direction_diagnostics", "moving_collision_diagnostics",
        "selected_intent_diagnostics", "timing_scale_diagnostics", "infield_opportunity_diagnostics",
        "outfield_advance_diagnostics", "no_bounce_catch_diagnostics"
    ])]
    pitch_bat_timing_comparison: bool,
    /// Compare a steeper ContactFirst swing with reduced vertical correction.
    #[arg(long, conflicts_with_all = [
        "pitch_bat_timing_comparison", "contact_length_comparison", "contact_first_swing_speed_comparison", "contact_first_ev_terms", "contact_first_ev_comparison", "intent_aim_comparison", "swing_speed_batter_id",
        "line_drive_lift_comparison", "vertical_contact_sweep", "vertical_contact_comparison",
        "vertical_contact_paired_fielding", "timing_geometry_sweep", "right_field_first_comparison",
        "no_bounce_margin_comparison", "pre_fence_pickup_comparison", "bip_diagnostics",
        "fair_launch_diagnostics", "contact_direction_diagnostics", "moving_collision_diagnostics",
        "selected_intent_diagnostics", "timing_scale_diagnostics", "infield_opportunity_diagnostics",
        "outfield_advance_diagnostics", "no_bounce_catch_diagnostics"
    ])]
    vertical_bat_control_comparison: bool,
    /// Compare current intent selection with a weighted early-count approach in full games.
    #[arg(long, conflicts_with_all = [
        "vertical_bat_control_comparison", "pitch_bat_timing_comparison", "contact_length_comparison", "contact_first_swing_speed_comparison", "contact_first_ev_terms", "contact_first_ev_comparison", "intent_aim_comparison", "swing_speed_batter_id",
        "line_drive_lift_comparison", "vertical_contact_sweep", "vertical_contact_comparison",
        "vertical_contact_paired_fielding", "timing_geometry_sweep", "right_field_first_comparison",
        "no_bounce_margin_comparison", "pre_fence_pickup_comparison", "bip_diagnostics",
        "fair_launch_diagnostics", "contact_direction_diagnostics", "moving_collision_diagnostics",
        "selected_intent_diagnostics", "timing_scale_diagnostics", "infield_opportunity_diagnostics",
        "outfield_advance_diagnostics", "no_bounce_catch_diagnostics"
    ])]
    count_intent_comparison: bool,
    #[arg(long, default_value_t = 5000)]
    max_pitches: u32,
    /// Include BIP quality, fielding/running timing groups and bounded example plays.
    #[arg(long)]
    bip_diagnostics: bool,
    /// Compare exit-speed and launch-angle frequencies over all fair batting outcomes.
    #[arg(long)]
    fair_launch_diagnostics: bool,
    /// Replay simulated swings at -4, -2, 0, +2, and +4 mm vertical contact offsets.
    #[arg(long)]
    vertical_contact_sweep: bool,
    /// Simulate full games at 0 and +4 mm relative to current vertical contact positions.
    #[arg(long, conflicts_with_all = [
        "vertical_contact_sweep", "line_drive_lift_comparison", "timing_geometry_sweep",
        "right_field_first_comparison", "no_bounce_margin_comparison", "pre_fence_pickup_comparison"
    ])]
    vertical_contact_comparison: bool,
    /// Run each contact policy with and without clean catches taking precedence over ground-rule doubles.
    #[arg(long, requires = "vertical_contact_comparison")]
    ground_rule_double_catch_comparison: bool,
    /// Add a +4 mm trial restricted to pre-trial vertical contact positions in [10, 30) mm.
    #[arg(long, requires = "vertical_contact_comparison")]
    vertical_contact_window: bool,
    /// Compare baseline, +4 mm in the contact window, and the exit-speed-dependent window.
    #[arg(long, requires = "vertical_contact_window")]
    vertical_contact_window_ev: bool,
    /// Replay two window policies on the same swing with live fielding and baserunning.
    #[arg(long, conflicts_with_all = [
        "vertical_contact_comparison", "vertical_contact_sweep", "line_drive_lift_comparison",
        "timing_geometry_sweep", "right_field_first_comparison", "no_bounce_margin_comparison",
        "pre_fence_pickup_comparison", "timing_scale_diagnostics", "infield_opportunity_diagnostics",
        "outfield_advance_diagnostics", "no_bounce_catch_diagnostics"
    ])]
    vertical_contact_paired_fielding: bool,
    /// Diagnose catch and first-base out opportunities on 40–47.5 m/s, 10–15 degree singles.
    #[arg(long, requires = "vertical_contact_comparison")]
    vertical_contact_fielding_diagnostics: bool,
    /// Replay frozen contacts with individual HLA components removed; no game changes.
    #[arg(long)]
    contact_direction_diagnostics: bool,
    /// Compare finite collision responses at frozen legacy contact coordinates.
    #[arg(long)]
    moving_collision_diagnostics: bool,
    /// Collect per-game shadow states in memory and audit only the selected Intent 3-D path.
    #[arg(long)]
    selected_intent_diagnostics: bool,
    /// Compare timing-to-position scales on the same realized swings.
    #[arg(long)]
    timing_scale_diagnostics: bool,
    /// Run each scheduled game at geometry scales 0.75, 0.875 and 1.00.
    #[arg(long, conflicts_with = "timing_scale_diagnostics")]
    timing_geometry_sweep: bool,
    /// Compare baseline games with the experimental right-field force play at first.
    #[arg(long, conflicts_with = "timing_geometry_sweep")]
    right_field_first_comparison: bool,
    /// Audit earlier infield interception opportunities on outfield pickup singles.
    #[arg(long)]
    infield_opportunity_diagnostics: bool,
    /// Compare batter arrivals with direct and second-to-third throws on the same outfield play.
    #[arg(long)]
    outfield_advance_diagnostics: bool,
    /// Replay baseline singles at 100%, 50%, and 0% of the direct-catch margin.
    #[arg(long)]
    no_bounce_catch_diagnostics: bool,
    /// Simulate full games with 100% and 50% of the pre-adjustment direct-catch margin.
    #[arg(long, conflicts_with_all = ["timing_geometry_sweep", "right_field_first_comparison", "no_bounce_catch_diagnostics"])]
    no_bounce_margin_comparison: bool,
    /// Compare current games with outfielders allowed to pick up a bounced ball before wall impact.
    #[arg(long, conflicts_with_all = ["line_drive_lift_comparison", "timing_geometry_sweep", "right_field_first_comparison", "no_bounce_margin_comparison"])]
    pre_fence_pickup_comparison: bool,
}
#[derive(Default, Serialize)]
struct Counts {
    games: u64,
    pitches: u64,
    pa: u64,
    ab: u64,
    single: u64,
    double: u64,
    triple: u64,
    hr: u64,
    strikeouts: u64,
    walks: u64,
    hit_by_pitch: u64,
    runs: u64,
    runs_allowed: u64,
    pitching_outs: u64,
    non_strikeout_outs: u64,
}
impl Counts {
    fn merge(&mut self, other: Self) {
        self.games += other.games;
        self.pitches += other.pitches;
        self.pa += other.pa;
        self.ab += other.ab;
        self.single += other.single;
        self.double += other.double;
        self.triple += other.triple;
        self.hr += other.hr;
        self.strikeouts += other.strikeouts;
        self.walks += other.walks;
        self.hit_by_pitch += other.hit_by_pitch;
        self.runs += other.runs;
        self.runs_allowed += other.runs_allowed;
        self.pitching_outs += other.pitching_outs;
        self.non_strikeout_outs += other.non_strikeout_outs;
    }

    fn batting(&mut self, result: BattingResult) {
        use BattingResult::*;
        self.pitches += 1;
        match result {
            Single => self.single += 1,
            Double => self.double += 1,
            Triple => self.triple += 1,
            HomeRun => self.hr += 1,
            Strikeout => self.strikeouts += 1,
            Walk => self.walks += 1,
            HitByPitch => self.hit_by_pitch += 1,
            _ => {}
        }
        if matches!(
            result,
            Single | Double | Triple | HomeRun | Strikeout | Out | FieldersChoice | DoublePlay
        ) {
            self.ab += 1;
            self.pa += 1;
        }
        if matches!(result, Walk | HitByPitch) {
            self.pa += 1;
        }
    }
    fn json(&self) -> serde_json::Value {
        let mut v = serde_json::to_value(self).unwrap();
        v["ba"] = if self.ab > 0 {
            serde_json::json!(
                (self.single + self.double + self.triple + self.hr) as f64 / self.ab as f64
            )
        } else {
            serde_json::Value::Null
        };
        v["hr_per_pa"] = if self.pa > 0 {
            serde_json::json!(self.hr as f64 / self.pa as f64)
        } else {
            serde_json::Value::Null
        };
        let in_play = self.ab - self.strikeouts - self.hr;
        v["babip_balls_in_play"] = serde_json::json!(in_play);
        v["babip"] = serde_json::json!(
            (in_play > 0)
                .then(|| (self.single + self.double + self.triple) as f64 / in_play as f64)
        );
        v["strikeouts_per_pa"] =
            serde_json::json!((self.pa > 0).then(|| self.strikeouts as f64 / self.pa as f64));
        v["singles_per_pa"] =
            serde_json::json!((self.pa > 0).then(|| self.single as f64 / self.pa as f64));
        v["ra9_reference"] = if self.pitching_outs > 0 {
            serde_json::json!(self.runs_allowed as f64 * 27.0 / self.pitching_outs as f64)
        } else {
            serde_json::Value::Null
        };
        v
    }

    fn vertical_contact_json(&self, include_window_metrics: bool) -> serde_json::Value {
        let mut summary = self.json();
        if include_window_metrics {
            let total_bases = self.single + 2 * self.double + 3 * self.triple + 4 * self.hr;
            let extra_base_hits = self.double + self.triple + self.hr;
            summary["slugging"] =
                serde_json::json!((self.ab > 0).then(|| total_bases as f64 / self.ab as f64));
            summary["extra_base_hits_per_pa"] =
                serde_json::json!((self.pa > 0).then(|| extra_base_hits as f64 / self.pa as f64));
            summary["doubles_per_pa"] =
                serde_json::json!((self.pa > 0).then(|| self.double as f64 / self.pa as f64));
        }
        summary
    }
}

#[derive(Default, Serialize)]
struct FairBallOutcomes {
    plays: u64,
    single: u64,
    double: u64,
    triple: u64,
    home_run: u64,
    out: u64,
    fielders_choice: u64,
    double_play: u64,
}

impl FairBallOutcomes {
    fn observe(&mut self, result: BattingResult) {
        let count = match result {
            BattingResult::Single => &mut self.single,
            BattingResult::Double => &mut self.double,
            BattingResult::Triple => &mut self.triple,
            BattingResult::HomeRun => &mut self.home_run,
            BattingResult::Out => &mut self.out,
            BattingResult::FieldersChoice => &mut self.fielders_choice,
            BattingResult::DoublePlay => &mut self.double_play,
            _ => return,
        };
        *count += 1;
        self.plays += 1;
    }

    fn merge(&mut self, other: Self) {
        self.plays += other.plays;
        self.single += other.single;
        self.double += other.double;
        self.triple += other.triple;
        self.home_run += other.home_run;
        self.out += other.out;
        self.fielders_choice += other.fielders_choice;
        self.double_play += other.double_play;
    }
}

#[derive(Default, Serialize)]
struct GroundRuleDoubleOutcomes {
    awarded_doubles: u64,
    caught_before_bounce: u64,
}

impl GroundRuleDoubleOutcomes {
    fn merge(&mut self, other: Self) {
        self.awarded_doubles += other.awarded_doubles;
        self.caught_before_bounce += other.caught_before_bounce;
    }
}

#[derive(Default)]
struct VerticalContactOutcomes {
    swings: u64,
    whiffs: u64,
    low: FairBallOutcomes,
    next: FairBallOutcomes,
    low_line_drive: FairBallOutcomes,
    mid_speed_20_to_25_deg: FairBallOutcomes,
    home_runs_20_to_30_deg: u64,
    high_ev_home_runs_20_to_25_deg: u64,
    by_launch_band: BTreeMap<String, FairBallOutcomes>,
    ground_rule_double: GroundRuleDoubleOutcomes,
}

impl VerticalContactOutcomes {
    fn observe_ball(&mut self, speed: f64, angle: f64, result: BattingResult) {
        if LowLineDriveFieldingAudit::matches_launch(speed, angle) {
            self.low_line_drive.observe(result);
        }
        if result == BattingResult::HomeRun && (20.0..30.0).contains(&angle) {
            self.home_runs_20_to_30_deg += 1;
        }
        if (20.0..25.0).contains(&angle) {
            if (fair_launch_audit::HIGH_EV_MIN_MS..VERTICAL_CONTACT_WINDOW_EV_SPLIT_MS)
                .contains(&speed)
            {
                self.mid_speed_20_to_25_deg.observe(result);
            }
            if result == BattingResult::HomeRun
                && speed.is_finite()
                && speed >= VERTICAL_CONTACT_WINDOW_EV_SPLIT_MS
            {
                self.high_ev_home_runs_20_to_25_deg += 1;
            }
        }
        if !speed.is_finite() || speed < fair_launch_audit::HIGH_EV_MIN_MS {
            return;
        }
        if fair_launch_audit::TARGET_VLA.contains(&angle) {
            self.low.observe(result);
        } else if (10.0..15.0).contains(&angle) {
            self.next.observe(result);
        }
    }

    fn observe_launch(
        &mut self,
        speed: f64,
        angle: f64,
        stadium_direction: Option<f64>,
        result: BattingResult,
    ) {
        if !bip_audit::is_bip(result) && result != BattingResult::HomeRun {
            return;
        }
        let ev = bip_audit::band(
            if speed > 0.0 { speed } else { f64::NAN },
            vertical_contact_game_sweep::EXIT_SPEED_BANDS_MS,
        );
        let vla = bip_audit::band(angle, vertical_contact_game_sweep::FLIGHT_ANGLE_BANDS_DEG);
        let hla = bip_audit::band(
            stadium_direction.unwrap_or(f64::NAN),
            bip_audit::CONVERSION_HLA_EDGES_DEG,
        );
        self.by_launch_band
            .entry(format!("EV_ms:{ev}|VLA_deg:{vla}|HLA_deg:{hla}"))
            .or_default()
            .observe(result);
    }

    fn observe_game(&mut self, game: &GameResult, include_launch_bands: bool) {
        for batting in &game.player_battings {
            if batting.ball.outbound_result == OutboundResult::GroundRuleDouble {
                self.ground_rule_double.awarded_doubles +=
                    u64::from(batting.result == BattingResult::Double);
                self.ground_rule_double.caught_before_bounce +=
                    u64::from(batting.result == BattingResult::Out);
            }
            if batting.swing_decision.is_some_and(|decision| decision.swung) {
                self.swings += 1;
                self.whiffs += u64::from(
                    batting.adjusted_contact_type == SwingContactType::SwungAndMiss,
                );
            }
            self.observe_ball(
                batting.ball.launch_speed,
                batting.ball.launch_angle,
                batting.result,
            );
            if include_launch_bands {
                self.observe_launch(
                    batting.ball.launch_speed,
                    batting.ball.launch_angle,
                    (batting.hla_model_version == Some(2)).then_some(batting.final_hla_deg),
                    batting.result,
                );
            }
        }
    }

    fn merge(&mut self, other: Self) {
        self.swings += other.swings;
        self.whiffs += other.whiffs;
        self.low.merge(other.low);
        self.next.merge(other.next);
        self.low_line_drive.merge(other.low_line_drive);
        self.mid_speed_20_to_25_deg.merge(other.mid_speed_20_to_25_deg);
        self.home_runs_20_to_30_deg += other.home_runs_20_to_30_deg;
        self.high_ev_home_runs_20_to_25_deg += other.high_ev_home_runs_20_to_25_deg;
        self.ground_rule_double.merge(other.ground_rule_double);
        for (band, outcomes) in other.by_launch_band {
            self.by_launch_band.entry(band).or_default().merge(outcomes);
        }
    }

    fn json(&self, include_window_metrics: bool) -> serde_json::Value {
        let mut report = serde_json::json!({
            "swings": self.swings,
            "whiffs": self.whiffs,
            "whiffs_per_swing": (self.swings > 0)
                .then(|| self.whiffs as f64 / self.swings as f64),
            "fast_fair_by_launch_angle_deg": {
                "[7,10)": self.low,
                "[10,15)": self.next,
            },
        });
        if include_window_metrics {
            report["low_line_drive"] = serde_json::json!(self.low_line_drive);
            report["home_runs_20_to_30_deg"] = serde_json::json!(self.home_runs_20_to_30_deg);
            report["mid_speed_20_to_25_deg"] = serde_json::json!(self.mid_speed_20_to_25_deg);
            report["high_ev_home_runs_20_to_25_deg"] =
                serde_json::json!(self.high_ev_home_runs_20_to_25_deg);
            report["by_exit_speed_launch_angle_direction"] = serde_json::json!(self.by_launch_band);
        }
        report
    }
}

#[derive(Default)]
struct VerticalContactSummary {
    total: Counts,
    outcomes: VerticalContactOutcomes,
    low_line_drive_fielding: LowLineDriveFieldingAudit,
}
#[derive(Default, Serialize)]
struct PairedContactSummary {
    pairs: u64,
    /// Outer keys describe the +4 mm result and inner keys the EV-dependent result.
    transitions: BTreeMap<String, BTreeMap<String, u64>>,
    fence_audit: PairedFenceSummary,
}

impl PairedContactSummary {
    fn observe(&mut self, pair: &PairedContactPlay) {
        let flat = format!("{}:{}", pair.flat.launch_band(), pair.flat.result_label());
        let by_ev = format!(
            "{}:{}",
            pair.by_baseline_ev.launch_band(),
            pair.by_baseline_ev.result_label()
        );
        self.pairs += 1;
        self.fence_audit.observe(pair);
        *self
            .transitions
            .entry(flat)
            .or_default()
            .entry(by_ev)
            .or_default() += 1;
    }

    fn merge(&mut self, seed: Self) {
        self.pairs += seed.pairs;
        self.fence_audit.merge(seed.fence_audit);
        for (flat, outcomes) in seed.transitions {
            let row = self.transitions.entry(flat).or_default();
            for (by_ev, count) in outcomes {
                *row.entry(by_ev).or_default() += count;
            }
        }
    }
}

#[derive(Default, Serialize)]
struct PairedFenceSummary {
    pairs_with_ground_rule_double: u64,
    flat_ground_rule_doubles: FenceCatchCounts,
    by_baseline_ev_ground_rule_doubles: FenceCatchCounts,
    // Count opportunities on the flat branch of this transition.
    flat_ground_rule_double_to_ev_direct_catch: FenceCatchCounts,
}

impl PairedFenceSummary {
    fn observe(&mut self, pair: &PairedContactPlay) {
        let Some(audit) = &pair.fence_audit else {
            return;
        };
        self.pairs_with_ground_rule_double += 1;
        let is_ground_rule_double =
            |ball: BattedBall| ball.outbound_result == OutboundResult::GroundRuleDouble;
        if let Some(flat) = &audit.flat
            && pair.flat.ball.is_some_and(is_ground_rule_double)
        {
            self.flat_ground_rule_doubles.observe(flat);
            if pair.by_baseline_ev.batting_result == Some(BattingResult::Out)
                && pair
                    .by_baseline_ev
                    .fielding
                    .and_then(|fielding| fielding.catch_diagnostics)
                    .is_some_and(|catch| {
                        catch.catch_type == CatchType::DirectFly && catch.error_type.is_none()
                    })
            {
                self.flat_ground_rule_double_to_ev_direct_catch
                    .observe(flat);
            }
        }
        if let Some(by_ev) = &audit.by_baseline_ev
            && pair.by_baseline_ev.ball.is_some_and(is_ground_rule_double)
        {
            self.by_baseline_ev_ground_rule_doubles.observe(by_ev);
        }
    }

    fn merge(&mut self, other: Self) {
        self.pairs_with_ground_rule_double += other.pairs_with_ground_rule_double;
        self.flat_ground_rule_doubles
            .merge(other.flat_ground_rule_doubles);
        self.by_baseline_ev_ground_rule_doubles
            .merge(other.by_baseline_ev_ground_rule_doubles);
        self.flat_ground_rule_double_to_ev_direct_catch
            .merge(other.flat_ground_rule_double_to_ev_direct_catch);
    }
}

#[derive(Default, Serialize)]
struct FenceCatchCounts {
    plays: u64,
    integrated_path_reachable: u64,
    integrated_path_within_risk_margin: u64,
    integrated_path_within_risk_margin_and_eligible: u64,
    clean_direct_catch_in_fielding_replay: u64,
}

impl FenceCatchCounts {
    fn observe(&mut self, audit: &FenceAudit) {
        self.plays += 1;
        self.integrated_path_reachable += u64::from(audit.integrated_path_reachable);
        self.integrated_path_within_risk_margin +=
            u64::from(audit.integrated_path_within_risk_margin);
        self.integrated_path_within_risk_margin_and_eligible +=
            u64::from(audit.integrated_path_within_risk_margin_and_eligible);
        self.clean_direct_catch_in_fielding_replay +=
            u64::from(audit.clean_direct_catch_in_fielding_replay);
    }

    fn merge(&mut self, other: Self) {
        self.plays += other.plays;
        self.integrated_path_reachable += other.integrated_path_reachable;
        self.integrated_path_within_risk_margin += other.integrated_path_within_risk_margin;
        self.integrated_path_within_risk_margin_and_eligible +=
            other.integrated_path_within_risk_margin_and_eligible;
        self.clean_direct_catch_in_fielding_replay += other.clean_direct_catch_in_fielding_replay;
    }
}

#[derive(Serialize)]
struct ScheduledPairedContactPlay {
    game_id: u32,
    #[serde(flatten)]
    play: PairedContactPlay,
}

#[derive(Default, Serialize)]
struct FieldingCounts {
    fielded_balls: u64,
    direct_catches: u64,
    candidate_attempts: u64,
    candidate_errors: u64,
    direct_candidate_attempts: u64,
    direct_candidate_errors: u64,
}

impl FieldingCounts {
    fn observe(&mut self, result: &GameResult) {
        for fielding in &result.player_fieldings {
            if let Some(catch) = fielding.catch_diagnostics {
                self.fielded_balls += 1;
                if catch.catch_type == CatchType::DirectFly && catch.error_type.is_none() {
                    self.direct_catches += 1;
                }
            }
        }
        for candidate in &result.player_fielding_candidates {
            let d = candidate.diagnostics;
            if d.interception_catch_type.is_none() {
                continue;
            }
            self.candidate_attempts += 1;
            let error = u64::from(d.interception_error_type.is_some());
            self.candidate_errors += error;
            if d.search.is_some_and(|s| s.risk_ok_samples > 0) {
                self.direct_candidate_attempts += 1;
                self.direct_candidate_errors += error;
            }
        }
    }
}

fn schedules(
    conn: &Connection,
    repo: &SqlGameRepository,
    args: &Args,
) -> Result<Vec<GameSchedule>> {
    let mut query=conn.prepare("SELECT g.id,g.season,g.round_seq,g.seq,g.planned_date,g.away_team_id,g.home_team_id,
        a.name AS away_team_name,h.name AS home_team_name,g.game_type,
        st.id AS stadium_id,st.name AS stadium_name,st.foul_pole_distance AS stadium_foul_pole_distance,
        st.center_fence_distance AS stadium_center_fence_distance,st.fence_height AS stadium_fence_height,
        COALESCE(l.max_inning,12) AS max_inning,COALESCE(l.base_four_seam_speed,40.833) AS base_four_seam_speed
        FROM game g JOIN team a ON a.id=g.away_team_id JOIN team h ON h.id=g.home_team_id
        JOIN stadium st ON st.id=g.stadium_id JOIN league l ON l.id=a.league_id
        WHERE g.season=?1 AND (?3 IS NULL OR
            (SELECT team_id FROM player_info WHERE id=?3) IN (g.away_team_id,g.home_team_id))
        ORDER BY g.round_seq,g.seq,g.id LIMIT ?2")?;
    let mut rows = query.query(rusqlite::params![
        args.season,
        args.games.map(|v| v as i64).unwrap_or(-1),
        args.swing_speed_batter_id
    ])?;
    let mut teams = BTreeMap::<u16, Team>::new();
    let mut games = Vec::new();
    while let Some(row) = rows.next()? {
        let mut game = GameSchedule::from_row(row)?;
        for team in [&mut game.away_team, &mut game.home_team] {
            if !teams.contains_key(&team.id) {
                team.players = repo.load_team_players(team.id)?;
                team.manager = load_team_manager(conn, team.id)?;
                teams.insert(team.id, team.clone());
            }
            *team = teams[&team.id].clone();
        }
        games.push(game);
    }
    ensure!(!games.is_empty(), "no schedule rows for requested season");
    if let Some(n) = args.games {
        ensure!(
            games.len() == n,
            "requested {n} games but only {} are available",
            games.len()
        );
    }
    Ok(games)
}
fn accumulate(
    total: &mut Counts,
    teams: &mut BTreeMap<u16, Counts>,
    players: &mut BTreeMap<i64, Counts>,
    game: &GameSchedule,
    result: &GameResult,
) -> Result<()> {
    total.games += 1;
    total.runs += u64::from(result.away_total_point) + u64::from(result.home_total_point);
    total.runs_allowed = total.runs;
    let roster: BTreeMap<_, _> = game
        .away_team
        .players
        .iter()
        .map(|p| (p.info.id, game.away_team.id))
        .chain(
            game.home_team
                .players
                .iter()
                .map(|p| (p.info.id, game.home_team.id)),
        )
        .collect();
    for (id, scored, allowed) in [
        (
            game.away_team.id,
            result.away_total_point,
            result.home_total_point,
        ),
        (
            game.home_team.id,
            result.home_total_point,
            result.away_total_point,
        ),
    ] {
        let t = teams.entry(id).or_default();
        t.games += 1;
        t.runs += u64::from(scored);
        t.runs_allowed += u64::from(allowed);
    }
    let mut appearing = std::collections::BTreeSet::new();
    for b in &result.player_battings {
        if appearing.insert(b.batter_id) {
            players.entry(b.batter_id).or_default().games += 1;
        }
        total.batting(b.result);
        teams
            .entry(roster[&b.batter_id])
            .or_default()
            .batting(b.result);
        players.entry(b.batter_id).or_default().batting(b.result);
    }
    for inning in &result.innings {
        let defense = if inning.tb == saberbb::domain::shared::game::TB::Top {
            game.home_team.id
        } else {
            game.away_team.id
        };
        let outs = u64::from(
            inning
                .counts
                .iter()
                .map(|c| c.out)
                .max()
                .unwrap_or(0)
                .min(3),
        );
        total.pitching_outs += outs;
        teams.entry(defense).or_default().pitching_outs += outs;
    }
    ensure!(
        total.pitching_outs >= total.strikeouts,
        "recorded outs less than strikeouts"
    );
    total.non_strikeout_outs = total.pitching_outs - total.strikeouts;
    // Team strikeouts above are batting SO; compute defensive non-K outs from actual pitcher records.
    for id in [game.away_team.id, game.home_team.id] {
        let defensive_so = result
            .player_battings
            .iter()
            .filter(|b| b.result == BattingResult::Strikeout && roster[&b.batter_id] != id)
            .count() as u64;
        let team = teams.get_mut(&id).unwrap();
        let game_outs = result
            .innings
            .iter()
            .filter(|i| {
                (i.tb == saberbb::domain::shared::game::TB::Top) == (id == game.home_team.id)
            })
            .map(|i| u64::from(i.counts.iter().map(|c| c.out).max().unwrap_or(0).min(3)))
            .sum::<u64>();
        ensure!(
            game_outs >= defensive_so,
            "invalid defensive strikeout count"
        );
        team.non_strikeout_outs += game_outs - defensive_so;
    }
    Ok(())
}
fn main() -> Result<()> {
    let args = Args::parse();
    use saberbb::domain::shared::flight_drag::{FlightDragModel, with_model};
    let model = if args.legacy_flight_drag {
        FlightDragModel::Legacy
    } else {
        FlightDragModel::Published2017
    };
    with_model(model, || {
        if args.vertical_contact_sweep {
            with_vertical_contact_trial(0, || run(args))
        } else {
            run(args)
        }
    })
}
fn line_drive_lift_offsets(args: &Args) -> Result<&[u16]> {
    let offsets = if args.line_drive_lift_offsets.is_empty() {
        &[0, 10, 15, 20]
    } else {
        args.line_drive_lift_offsets.as_slice()
    };
    ensure!(
        offsets
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == offsets.len(),
        "duplicate LineDrive lift offsets"
    );
    Ok(offsets)
}

fn run(args: Args) -> Result<()> {
    let lift_offsets = line_drive_lift_offsets(&args)?;
    ensure!(
        args.games != Some(0) && !args.seeds.is_empty(),
        "games/seeds must be nonempty"
    );
    ensure!(
        args.seeds
            .iter()
            .collect::<std::collections::BTreeSet<_>>()
            .len()
            == args.seeds.len(),
        "duplicate master seeds"
    );
    ensure!(
        args.max_pitches > 0 && args.max_pitches < 65535,
        "invalid pitch budget"
    );
    let path = std::env::var("SABERBB_DB").context("SABERBB_DB is not set")?;
    ensure!(!path.trim().is_empty(), "SABERBB_DB is blank");
    let path = PathBuf::from(path);
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    conn.execute_batch("PRAGMA query_only=ON;BEGIN;")?;
    let repo = SqlGameRepository::read_only(&path)?;
    let games = schedules(&conn, &repo, &args)?;
    let input_hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&games)?));
    I18nManager::init("en-US");
    if let Some(batter_id) = args.swing_speed_batter_id {
        let report = swing_speed_comparison::run(&conn, &args, &games, batter_id, &input_hash)?;
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }
    let mut output = serde_json::json!({
        "engine_source_sha256": saberbb::domain::seeded_diagnostic::ENGINE,
        "flight_drag_model": saberbb::domain::shared::flight_drag::current_model(),
        "flight_drag_settings": {
            "density_kg_m3": saberbb::domain::shared::flight_drag::DENSITY_KG_M3,
            "mass_kg": saberbb::domain::shared::flight_drag::MASS_KG,
            "radius_m": saberbb::domain::shared::flight_drag::RADIUS_M,
            "published_cd": "0.297 + 0.0292 * total_spin_rpm / 1000",
            "lift": "unchanged legacy transverse-spin model"
        },
        "default_direct_catch_margin_percent": 50,
        "default_ground_rule_double_catch_enabled": DEFAULT_GROUND_RULE_DOUBLE_CATCH_ENABLED,
        "default_line_drive_lift_offset_mm": DEFAULT_LINE_DRIVE_LIFT_OFFSET_MM,
        "default_vertical_contact_policy": "contact_z_window_by_baseline_ev",
        "inputs_sha256": input_hash, "season": args.season, "games_per_seed": games.len(),
        "definitions": {
            "ra9_reference": "Runs allowed * 27 / recorded outs; earned runs not recorded",
            "non_strikeout_outs": "All recorded outs minus defensive strikeouts, including baserunning outs",
            "rosters": "Current source rosters, freshly initialized per game; historical lineups/seeds are not reproduced",
            "babip": "(1B + 2B + 3B) / (AB - K - HR); sacrifice flies have no separate result category"
        }
    });
    if args.vertical_contact_sweep {
        output["definitions"]["vertical_contact_sweep"] = serde_json::json!(
            "Fixed simulated swings replayed at signed world-vertical contact offsets in mm. Live games in this sweep use zero additional offset so that the replay starts from the uncorrected contact geometry. Positive shifts lift the vertical deflection. All shifts use the same baseline-matched swing pool. Recovered swings share one deterministic set of face-angle draws. Fair and home-run counts are trajectory outcomes; official baseline counts use recorded game outcomes."
        );
        output["definitions"]["paired_zero_to_plus_four_mm"] = serde_json::json!(
            "Paired trajectories on the same comparable swings. Low line drives are fair at exit speed [40,47.5) m/s and angle [10,15) degrees. The home-run cohort requires a home-run trajectory and angle [20,30) degrees. Input cells group by baseline world-vertical contact displacement, actual bat angle and baseline exit speed. A geometric miss has no baseline exit speed."
        );
        output["definitions"]["window_exit_speed_comparison"] = serde_json::json!(
            "Three policies on the same baseline-matched swings: zero shift, +4 mm inside the baseline contact Z window [10,30) mm, and +2 mm within that window when baseline exit speed is below 47.5 m/s with +4 mm otherwise. For a baseline geometric miss there is no exit speed, so the third policy keeps the +4 mm window shift. Exit-speed and angle bands in each policy's outcome use the resulting flight. Counts are trajectories before fielding or baserunning."
        );
    }
    if args.count_intent_comparison {
        output["count_intent_comparison"] = count_intent_comparison::run(&args, &games)?;
    } else if args.vertical_bat_control_comparison {
        output["vertical_bat_control_comparison"] =
            intent_aim_comparison::run_vertical_bat_control(&args, &games)?;
    } else if args.pitch_bat_timing_comparison {
        output["pitch_bat_timing_comparison"] =
            intent_aim_comparison::run_timing_geometry(&args, &games)?;
    } else if args.contact_length_comparison {
        output["contact_length_comparison"] = intent_aim_comparison::run_length(&args, &games)?;
    } else if args.contact_first_swing_speed_comparison {
        output["contact_first_swing_speed_comparison"] =
            intent_aim_comparison::run_speed(&args, &games)?;
    } else if args.contact_first_ev_terms {
        output["contact_first_ev_terms"] = contact_first_ev_terms::run(&args, &games)?;
    } else if args.contact_first_ev_comparison {
        output["contact_first_ev_comparison"] = contact_first_ev_comparison::run(&args, &games)?;
    } else if args.intent_aim_comparison {
        output["intent_aim_comparison"] = intent_aim_comparison::run(&args, &games)?;
    } else if args.vertical_contact_paired_fielding {
        let mut summary = PairedContactSummary::default();
        let reports = simulate_reports_inner(
            &args,
            &games,
            CONTACT_TIMING_POSITION_SCALE,
            false,
            None,
            None,
            Some(&mut summary),
        )?;
        output["vertical_contact_paired_fielding"] = serde_json::json!({
            "flat_offset_mm": 4,
            "ground_rule_double_catch_enabled": false,
            "baseline_contact_z_window_mm": VERTICAL_CONTACT_WINDOW_MM,
            "baseline_ev_split_ms": VERTICAL_CONTACT_WINDOW_EV_SPLIT_MS,
            "target_exit_speed_ms": 45.0..47.5,
            "target_launch_angle_deg": 15.0..25.0,
            "total": summary,
            "reports": reports,
        });
        output["definitions"]["vertical_contact_paired_fielding"] = serde_json::json!(
            "Live +4 mm window games supply the swings and initial play states. A pair differs by +4 versus +2 mm and has a fair trajectory at 45–47.5 m/s and 15–25 degrees in either branch. Ranges are half open. Each phase starts from the same RNG state for both branches. Both resolve through normal fielding and baserunning with the historical automatic ground-rule double award. Only the +4 mm result advances the game. Transition keys are launch band and official batting result. NoBattingResult indicates an interrupted play with no batting ruling."
        );
        output["definitions"]["paired_contact_fence_audit"] = serde_json::json!(
            "For pairs with a ground-rule double in either branch, record each fair fence decision at the integration tick, before reflection or clearance. A positive height_above_fence_m means the ball cleared the fence. Each branch reports catch opportunities from the integrated pre-bounce path with the normal height, movement, risk and eligibility criteria, plus a normal fielding replay from the copied post-launch RNG state using fielding's estimated path. The transition cohort counts opportunities on the +4 mm ground-rule-double branch when the +2 mm branch officially caught the ball on the fly."
        );
    } else if args.vertical_contact_comparison {
        let mut trials = Vec::new();
        let policies: &[&str] = if args.vertical_contact_window_ev {
            &["baseline", "contact_z_window", "contact_z_window_by_baseline_ev"]
        } else if args.vertical_contact_window {
            &["baseline", "uniform", "contact_z_window"]
        } else {
            &["baseline", "uniform"]
        };
        let catch_modes: &[bool] = if args.ground_rule_double_catch_comparison {
            &[false, true]
        } else {
            &[DEFAULT_GROUND_RULE_DOUBLE_CATCH_ENABLED]
        };
        let trials_to_run = policies
            .iter()
            .flat_map(|&policy| catch_modes.iter().map(move |&enabled| (policy, enabled)));
        for (policy, catch_enabled) in trials_to_run {
            eprintln!(
                "Full-game vertical contact comparison: {policy}, ground-rule-double catches: {catch_enabled}"
            );
            let mut summary = VerticalContactSummary::default();
            let run_trial = || {
                with_ground_rule_double_catch_trial(catch_enabled, || {
                    simulate_reports_inner(
                        &args,
                        &games,
                        CONTACT_TIMING_POSITION_SCALE,
                        false,
                        None,
                        Some(&mut summary),
                        None,
                    )
                })
            };
            let by_exit_speed = policy == "contact_z_window_by_baseline_ev";
            let windowed = policy == "contact_z_window" || by_exit_speed;
            let offset_mm = if policy == "baseline" { 0 } else { 4 };
            let reports = match policy {
                "contact_z_window_by_baseline_ev" => with_vertical_contact_window_ev_trial(run_trial),
                "contact_z_window" => with_vertical_contact_window_trial(offset_mm, run_trial),
                _ => with_vertical_contact_trial(offset_mm, run_trial),
            }
            .with_context(|| {
                format!("vertical contact trial {policy}, ground-rule-double catches {catch_enabled}")
            })?;
            let mut trial = serde_json::json!({
                "total": summary.total.vertical_contact_json(
                    args.vertical_contact_window || args.ground_rule_double_catch_comparison
                ),
                "vertical_contact_outcomes": summary.outcomes.json(args.vertical_contact_window),
                "reports": reports,
            });
            if by_exit_speed {
                trial["pre_trial_exit_speed_split_ms"] =
                    serde_json::json!(VERTICAL_CONTACT_WINDOW_EV_SPLIT_MS);
                trial["additional_vertical_contact_offsets_mm"] = serde_json::json!({
                    "below_split": 2,
                    "at_or_above_split": 4,
                    "no_pre_trial_exit_speed": 4,
                });
            } else {
                trial["additional_vertical_contact_offset_mm"] = serde_json::json!(offset_mm);
            }
            if args.vertical_contact_window || args.ground_rule_double_catch_comparison {
                trial["policy"] = serde_json::json!(policy);
                if windowed {
                    trial["pre_trial_contact_z_range_mm"] = serde_json::json!({
                        "min_inclusive": VERTICAL_CONTACT_WINDOW_MM.start,
                        "max_exclusive": VERTICAL_CONTACT_WINDOW_MM.end,
                    });
                }
            }
            trial["ground_rule_double_catch_enabled"] = serde_json::json!(catch_enabled);
            trial["ground_rule_double"] = serde_json::json!(summary.outcomes.ground_rule_double);
            if args.vertical_contact_fielding_diagnostics {
                trial["low_line_drive_fielding"] =
                    serde_json::to_value(summary.low_line_drive_fielding)?;
            }
            trials.push(trial);
        }
        output["vertical_contact_trials"] = serde_json::json!(trials);
        if args.ground_rule_double_catch_comparison {
            output["definitions"]["ground_rule_double_catch_comparison"] = serde_json::json!(
                "Each contact policy runs the same schedule, rosters and initial game seeds with the catch rule off and on. The rule applies to trajectories that clear the fence after a bounce. A clean direct catch selected by the ordinary fielding resolver before the first bounce uses the normal fly-out resolution. Missed opportunities still award two bases and leave the random stream at its pre-probe state. Awarded doubles plus caught-before-bounce counts give the number of qualifying trajectories. Changed results can alter later plays and random draws."
            );
        }
        output["definitions"]["vertical_contact_trials"] = serde_json::json!(if args.vertical_contact_window_ev {
            "Full games under the baseline, +4 mm within the contact-Z window, and the exit-speed-dependent policy within the same window. The window uses the live pre-trial world-vertical contact coordinate after the normal swing intent offset. The exit speed comes from that same unshifted swing's contact geometry. Geometric misses in the window keep +4 mm. Trials share schedule, rosters and starting RNG seeds, and pool counts before calculating rates. Changed plays can alter later decisions and RNG draws."
        } else if args.vertical_contact_window {
            "Full games under the baseline, uniform +4 mm, and [10,30) mm contact-Z window +4 mm policies. The window uses the live pre-trial world-vertical contact coordinate after the normal swing intent offset. Trials share schedule, rosters and starting RNG seeds, and pool counts before calculating rates. Changed plays can alter later decisions and RNG draws."
        } else {
            "Full games with an additional world-vertical contact offset on every swing, on top of the selected intent's normal offset. Trials share the schedule, rosters and starting RNG seed per game. Changed plays can change subsequent decisions and RNG draws. Trial totals pool all seeds before calculating rates."
        });
        if args.vertical_contact_window {
            output["definitions"]["low_line_drive"] = serde_json::json!(
                "Official fair batting outcomes at exit speed [40,47.5) m/s and vertical launch angle [10,15) degrees."
            );
            output["definitions"]["mid_speed_20_to_25_deg"] = serde_json::json!(
                "Official fair batting outcomes at exit speed [45,47.5) m/s and vertical launch angle [20,25) degrees, measured after the policy is applied. high_ev_home_runs_20_to_25_deg counts official home runs at exit speed at least 47.5 m/s in the same angle band."
            );
            output["definitions"]["by_exit_speed_launch_angle_direction"] = serde_json::json!(
                "Official fair batting outcomes by initial exit speed (EV), vertical launch angle (VLA) and stadium-relative horizontal launch angle (HLA). Negative HLA points toward third base, positive toward first base. Unavailable direction is counted as unknown. For doubles per 100 plate appearances, divide each cell's doubles by the corresponding trial or seed's total PA and multiply by 100. For the share of doubles among fair balls in a cell, use that cell's plays as the denominator."
            );
        }
        output["definitions"]["fast_fair_by_launch_angle_deg"] = serde_json::json!(
            "Recorded fair batting outcomes with exit speed at least 45 m/s. Launch-angle intervals include the lower bound and exclude the upper bound. Each batted ball counts once, including a double play."
        );
        if args.vertical_contact_fielding_diagnostics {
            output["definitions"]["low_line_drive_fielding"] = serde_json::json!(
                "Official singles with exit speed [40,47.5) m/s and vertical launch angle [10,15) degrees. Direct catches replay eligible live candidates with 100%, 50%, or 0% of the current required catch margin. First-base scenarios use the recorded fielding point and ready time, the modeled batter time, and the faster of a direct throw with cover and an infield carry. For a throw, the receiver starts either at first or from the live position at ball control. The combined zero-margin catch or late-cover first-base count counts each single once. These are fixed-play opportunities."
            );
        }
    } else if args.pre_fence_pickup_comparison {
        eprintln!("Pre-fence pickup comparison: baseline");
        let baseline = simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, false, None)?;
        eprintln!("Pre-fence pickup comparison: enabled");
        let enabled = with_pre_fence_pickup_trial(|| {
            simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, false, None)
        })?;
        output["pre_fence_pickup_trials"] = serde_json::json!([
            {"enabled": false, "reports": baseline},
            {"enabled": true, "reports": enabled},
        ]);
        output["definitions"]["pre_fence_pickup"] = serde_json::json!(
            "Selected pickups on recorded plays before projected wall impact. Throw timing starts at the pickup position and running is resolved with the new defensive clock. Both trials share schedule, rosters and starting seed per game; changed plays can shift later draws."
        );
        output["definitions"]["recorded_running_outs_by_event"] = serde_json::json!(
            "Recorded running rulings of Out grouped by event; GrounderPlay can include the batter-runner, and BaseSteal is counted separately."
        );
    } else if args.line_drive_lift_comparison {
        let mut trials = Vec::new();
        for &offset_mm in lift_offsets {
            eprintln!("LineDrive vertical contact offset: +{offset_mm} mm");
            let reports = simulate_reports(
                &args,
                &games,
                CONTACT_TIMING_POSITION_SCALE,
                false,
                Some(offset_mm),
            )?;
            trials.push(
                serde_json::json!({"vertical_contact_offset_mm":offset_mm,"reports":reports}),
            );
        }
        output["line_drive_lift_trials"] = serde_json::json!(trials);
        output["definitions"]["line_drive_lift_trials"] = serde_json::json!(
            "Positive world-vertical contact offset applies only to swings whose selected intent is LineDrive. All trials share rosters, schedule and per-game starting RNG seeds; outcomes can change later draws."
        );
    } else if args.no_bounce_margin_comparison {
        let mut trials = Vec::new();
        for percent in [100, 50] {
            eprintln!("Direct-catch margin: {percent}%");
            let reports = with_direct_catch_margin_scale(f64::from(percent) / 100.0, || {
                simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, false, None)
            })?;
            trials.push(serde_json::json!({"margin_percent": percent, "reports": reports}));
        }
        output["no_bounce_margin_trials"] = serde_json::json!(trials);
        output["definitions"]["candidate_errors"] = serde_json::json!(
            "Modeled interception errors across all evaluated fielder attempts; multiple candidates can be counted for one batted ball. Not official scoring errors."
        );
    } else if args.right_field_first_comparison {
        let mut trials = Vec::new();
        for enabled in [false, true] {
            trials.push(serde_json::json!({
                "enabled": enabled,
                "reports": simulate_reports(&args, &games, CONTACT_TIMING_POSITION_SCALE, enabled, None)?,
            }));
        }
        output["right_field_first_trials"] = serde_json::json!(trials);
    } else if args.timing_geometry_sweep {
        let mut trials = Vec::new();
        for scale in [CONTACT_TIMING_POSITION_SCALE, 0.875, 1.0] {
            let reports = with_contact_geometry_scale(scale, || {
                simulate_reports(&args, &games, scale, false, None)
            })?;
            trials.push(serde_json::json!({
                "geometry_scale": scale,
                "exit_speed_depth_scale": CONTACT_TIMING_POSITION_SCALE,
                "reports": reports,
            }));
        }
        output["timing_geometry_trials"] = serde_json::json!(trials);
    } else {
        output["reports"] = serde_json::json!(simulate_reports(
            &args,
            &games,
            CONTACT_TIMING_POSITION_SCALE,
            false,
            None,
        )?);
    }
    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn simulate_reports(
    args: &Args,
    games: &[GameSchedule],
    scale: f64,
    right_field_trial: bool,
    lift_offset_mm: Option<u16>,
) -> Result<Vec<serde_json::Value>> {
    simulate_reports_inner(
        args,
        games,
        scale,
        right_field_trial,
        lift_offset_mm,
        None,
        None,
    )
}

fn simulate_reports_inner(
    args: &Args,
    games: &[GameSchedule],
    scale: f64,
    right_field_trial: bool,
    lift_offset_mm: Option<u16>,
    mut trial_summary: Option<&mut VerticalContactSummary>,
    mut paired_summary: Option<&mut PairedContactSummary>,
) -> Result<Vec<serde_json::Value>> {
    let mut reports = Vec::new();
    for seed in &args.seeds {
        let mut vertical_outcomes = trial_summary
            .is_some()
            .then(VerticalContactOutcomes::default);
        let mut paired = paired_summary.is_some().then(PairedContactSummary::default);
        let mut paired_plays = Vec::new();
        let mut bip = args.bip_diagnostics.then(bip_audit::BipAudit::default);
        let mut vertical_contact = args
            .vertical_contact_sweep
            .then(vertical_contact_game_sweep::Sweep::default);
        let mut fair_launch = args
            .fair_launch_diagnostics
            .then(fair_launch_audit::FairLaunchAudit::default);
        let mut direction = args
            .contact_direction_diagnostics
            .then(contact_direction_audit::DirectionAudit::default);
        let mut throws = args
            .bip_diagnostics
            .then(throw_origin_audit::ThrowOriginAudit::default);
        let mut moving = args
            .moving_collision_diagnostics
            .then(moving_collision_audit::MovingAudit::default);
        let mut selected = args
            .selected_intent_diagnostics
            .then(selected_intent_audit::SelectedAudit::default);
        let mut timing = args
            .timing_scale_diagnostics
            .then(TimingScaleAudit::default);
        let mut infield = args
            .infield_opportunity_diagnostics
            .then(InfieldOpportunityAudit::default);
        let mut outfield = args
            .outfield_advance_diagnostics
            .then(OutfieldAdvanceAudit::default);
        let mut no_bounce = args
            .no_bounce_catch_diagnostics
            .then(NoBounceCatchAudit::default);
        let mut low_line_drive_fielding = args
            .vertical_contact_fielding_diagnostics
            .then(LowLineDriveFieldingAudit::default);
        let mut official_low_line_drive_singles = 0;
        let mut fielding = args
            .no_bounce_margin_comparison
            .then(FieldingCounts::default);
        let mut total = Counts::default();
        let mut running_outs = BTreeMap::new();
        let mut pre_fence_pickups = 0;
        let mut right_field_first_outs = 0;
        let mut lift_stats = LineDriveLiftStats::default();
        let mut teams = BTreeMap::new();
        let mut players = BTreeMap::new();
        for (index, game) in games.iter().enumerate() {
            let game_seed = derived_seed(*seed, game.id);
            let run = || {
                simulate_game(
                    game.clone(),
                    Box::new(RealRng::from_seed(game_seed)),
                    BattingStrategySettings::player_driven(),
                    Some(args.max_pitches),
                    args.selected_intent_diagnostics,
                )
            };
            let mut run_with_diagnostics = || {
                let mut run = || match low_line_drive_fielding.as_mut() {
                    Some(audit) => audit.observe_game(game_seed, run),
                    None => run(),
                };
                let mut run = || match no_bounce.as_mut() {
                    Some(audit) => audit.observe_game(game_seed, run),
                    None => run(),
                };
                let run = || match infield.as_mut() {
                    Some(audit) => audit.observe_game(run),
                    None => run(),
                };
                let run = || match outfield.as_mut() {
                    Some(audit) => audit.observe_game(run),
                    None => run(),
                };
                if let Some(timing) = timing.as_mut() {
                    timing.observe_game(game_seed, run)
                } else {
                    run()
                }
            };
            let result = if let Some(summary) = paired.as_mut() {
                let (result, plays) = with_paired_contact_fielding(run_with_diagnostics);
                for play in plays {
                    summary.observe(&play);
                    paired_plays.push(ScheduledPairedContactPlay { game_id: game.id, play });
                }
                result
            } else if right_field_trial {
                let (result, outs) = with_right_field_first_trial(run_with_diagnostics);
                right_field_first_outs += outs;
                result
            } else if let Some(offset_mm) = lift_offset_mm {
                let (result, stats) = with_line_drive_lift_trial(offset_mm, run_with_diagnostics);
                lift_stats.swings += stats.swings;
                lift_stats.whiffs += stats.whiffs;
                lift_stats.line_drive_swings += stats.line_drive_swings;
                lift_stats.line_drive_whiffs += stats.line_drive_whiffs;
                result
            } else {
                run_with_diagnostics()
            }
            .with_context(|| {
                format!(
                    "geometry scale {scale}, right-field trial {right_field_trial}, seed {seed}, game {} failed; no partial summary accepted",
                    game.id
                )
            })?;
            if low_line_drive_fielding.is_some() {
                official_low_line_drive_singles += result
                    .player_battings
                    .iter()
                    .filter(|b| {
                        b.result == BattingResult::Single
                            && LowLineDriveFieldingAudit::matches(&b.ball)
                    })
                    .count() as u64;
            }
            if let Some(direction) = direction.as_mut() {
                direction.observe(game, &result)?;
            }
            if let Some(throws) = throws.as_mut() {
                throws.observe(game, &result);
            }
            if let Some(moving) = moving.as_mut() {
                moving.observe(game, &result)?;
            }
            if let Some(selected) = selected.as_mut() {
                selected.observe(game, &result)?;
            }
            if let Some(fielding) = fielding.as_mut() {
                fielding.observe(&result);
            }
            if args.pre_fence_pickup_comparison {
                for running in &result.player_runnings {
                    if running.ruling == Ruling::Out {
                        *running_outs
                            .entry(running.event.as_ref().to_owned())
                            .or_insert(0u64) += 1;
                    }
                }
                let balls: BTreeMap<_, _> = result
                    .player_battings
                    .iter()
                    .map(|b| (b.count_seq, &b.ball))
                    .collect();
                pre_fence_pickups += result
                    .player_fieldings
                    .iter()
                    .filter(|f| {
                        f.catch_diagnostics
                            .zip(balls.get(&f.count_seq))
                            .is_some_and(|(catch, ball)| {
                                catch.pre_fence_pickup_position(ball).is_some()
                            })
                    })
                    .count();
            }
            accumulate(&mut total, &mut teams, &mut players, game, &result)?;
            if let Some(outcomes) = vertical_outcomes.as_mut() {
                outcomes.observe_game(&result, args.vertical_contact_window);
            }
            if let Some(sweep) = vertical_contact.as_mut() {
                sweep.observe_game(game, &result, game_seed, scale);
            }
            if let Some(audit) = fair_launch.as_mut() {
                audit.observe_launch_speed_terms(game, &result, scale);
                audit.observe(&result);
            }
            if let Some(bip) = bip.as_mut() {
                bip.observe(game, &result);
            }
            if (index + 1) % 12 == 0 || index + 1 == games.len() {
                eprintln!(
                    "geometry scale {scale}, right-field trial {right_field_trial}, seed {seed}: {}/{} games complete",
                    index + 1,
                    games.len()
                );
            }
        }
        let mut leaders: Vec<_> = players.into_iter().collect();
        leaders.sort_by(|a, b| b.1.hr.cmp(&a.1.hr).then_with(|| a.0.cmp(&b.0)));
        let mut report = serde_json::json!({"seed":seed.to_string(),"total":total.vertical_contact_json(args.vertical_contact_window || args.ground_rule_double_catch_comparison),
            "teams":teams.into_iter().map(|(id,t)|(id.to_string(),t.json())).collect::<BTreeMap<_,_>>(),
            "hr_leaders":leaders.iter().take(10).map(|(id,s)|serde_json::json!({"batter_id":id,"stats":s.json()})).collect::<Vec<_>>()});
        if args.pre_fence_pickup_comparison {
            report["pre_fence_pickup"] = serde_json::json!({
                "selected_pickups": pre_fence_pickups,
                "recorded_running_outs_by_event": running_outs,
                "doubles_per_pa": (total.pa > 0).then(|| total.double as f64 / total.pa as f64),
                "triples_per_pa": (total.pa > 0).then(|| total.triple as f64 / total.pa as f64),
            });
        }
        if lift_offset_mm.is_some() {
            report["line_drive_lift"] = serde_json::json!({
                "swings": lift_stats.swings,
                "whiffs": lift_stats.whiffs,
                "whiffs_per_swing": (lift_stats.swings > 0).then(|| lift_stats.whiffs as f64 / lift_stats.swings as f64),
                "line_drive_swings": lift_stats.line_drive_swings,
                "line_drive_whiffs": lift_stats.line_drive_whiffs,
                "line_drive_whiffs_per_swing": (lift_stats.line_drive_swings > 0)
                    .then(|| lift_stats.line_drive_whiffs as f64 / lift_stats.line_drive_swings as f64),
                "doubles_per_pa": (total.pa > 0).then(|| total.double as f64 / total.pa as f64),
                "triples_per_pa": (total.pa > 0).then(|| total.triple as f64 / total.pa as f64),
                "runs_per_game": total.runs as f64 / total.games as f64,
            });
        }
        if let Some(selected) = selected {
            report["selected_intent_diagnostics"] = selected.json();
        }
        if let Some(moving) = moving {
            report["moving_collision_diagnostics"] = moving.json();
        }
        if let Some(direction) = direction {
            report["contact_direction_diagnostics"] = direction.json();
        }
        if let Some(bip) = bip {
            report["bip_diagnostics"] = bip.json();
            report["bip_diagnostics"]["throw_origin_counterfactual"] = throws.unwrap().json();
        }
        if let Some(timing) = timing {
            report["timing_scale_diagnostics"] = timing.json();
        }
        if let Some(fielding) = fielding {
            report["fielding"] = serde_json::to_value(fielding)?;
            report["total"]["extra_base_hits_per_pa"] = serde_json::json!(
                (total.pa > 0)
                    .then(|| (total.double + total.triple + total.hr) as f64 / total.pa as f64)
            );
        }
        if let Some(no_bounce) = no_bounce {
            ensure!(
                no_bounce.baseline_singles() == total.single,
                "no-bounce audit single count does not match the official batting results"
            );
            report["no_bounce_catch_diagnostics"] = serde_json::to_value(no_bounce)?;
        }
        if let Some(outfield) = outfield {
            report["outfield_advance_diagnostics"] = serde_json::to_value(outfield)?;
        }
        if let (Some(total), Some(summary)) = (paired_summary.as_deref_mut(), paired) {
            report["paired_contact_fielding"] = serde_json::json!({
                "summary": summary,
                "plays": paired_plays,
            });
            total.merge(summary);
        }
        if let Some(sweep) = vertical_contact {
            report["vertical_contact_sweep"] = serde_json::to_value(sweep)?;
        }
        if let Some(audit) = fair_launch {
            ensure!(
                audit.non_hr_in_play == total.ab - total.strikeouts - total.hr
                    && audit.home_runs == total.hr,
                "fair launch audit counts do not match the official batting results"
            );
            report["fair_launch_diagnostics"] = serde_json::to_value(audit)?;
        }
        if let Some(infield) = infield {
            report["infield_opportunity_diagnostics"] = serde_json::to_value(infield)?;
        }
        if right_field_trial {
            report["right_field_first_outs"] = serde_json::json!(right_field_first_outs);
        }
        if let (Some(summary), Some(outcomes)) = (trial_summary.as_deref_mut(), vertical_outcomes) {
            if args.vertical_contact_window {
                let (plays, doubles) = outcomes.by_launch_band.values().fold(
                    (0, 0),
                    |(plays, doubles), cell| (plays + cell.plays, doubles + cell.double),
                );
                ensure!(
                    (plays, doubles) == (total.ab - total.strikeouts, total.double),
                    "launch-band fair balls or doubles do not match official batting results for seed {seed}"
                );
            }
            report["vertical_contact_outcomes"] = outcomes.json(args.vertical_contact_window);
            report["ground_rule_double"] = serde_json::json!(outcomes.ground_rule_double);
            summary.total.merge(total);
            summary.outcomes.merge(outcomes);
            if let Some(audit) = low_line_drive_fielding {
                ensure!(
                    audit.singles() == official_low_line_drive_singles,
                    "low line-drive fielding audit count does not match official singles for seed {seed}"
                );
                report["low_line_drive_fielding"] = serde_json::to_value(&audit)?;
                summary.low_line_drive_fielding.merge(audit);
            }
        }
        reports.push(report);
    }
    Ok(reports)
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ground_rule_double_trial_requires_a_vertical_contact_comparison() {
        let args = [
            "check",
            "--season",
            "2026",
            "--ground-rule-double-catch-comparison",
        ];
        assert!(Args::try_parse_from(args).is_err());
        let comparison = Args::try_parse_from(args.into_iter().chain([
            "--vertical-contact-comparison",
            "--vertical-contact-window",
            "--vertical-contact-window-ev",
        ]))
        .unwrap();
        assert!(comparison.ground_rule_double_catch_comparison);
        assert!(comparison.vertical_contact_window_ev);
    }

    #[test]
    fn paired_fielding_runs_as_its_own_diagnostic() {
        let args = ["check", "--season", "2026", "--vertical-contact-paired-fielding"];
        assert!(Args::try_parse_from(args).unwrap().vertical_contact_paired_fielding);
        assert!(
            Args::try_parse_from(args.into_iter().chain(["--vertical-contact-comparison"])).is_err()
        );
    }

    #[test]
    fn vertical_contact_comparison_separates_replay_and_counts_official_results() {
        let args = ["check", "--season", "2026", "--vertical-contact-comparison"];
        assert!(Args::try_parse_from(args).unwrap().vertical_contact_comparison);
        let fielding_flag = "--vertical-contact-fielding-diagnostics";
        let window_flag = "--vertical-contact-window";
        let window_ev_flag = "--vertical-contact-window-ev";
        assert!(
            Args::try_parse_from(args.into_iter().chain([fielding_flag]))
                .unwrap()
                .vertical_contact_fielding_diagnostics
        );
        assert!(Args::try_parse_from(["check", "--season", "2026", fielding_flag]).is_err());
        assert!(
            Args::try_parse_from(args.into_iter().chain([window_flag]))
                .unwrap()
                .vertical_contact_window
        );
        assert!(Args::try_parse_from(["check", "--season", "2026", window_flag]).is_err());
        assert!(
            Args::try_parse_from(args.into_iter().chain([window_flag, window_ev_flag]))
                .unwrap()
                .vertical_contact_window_ev
        );
        assert!(Args::try_parse_from(args.into_iter().chain([window_ev_flag])).is_err());
        assert!(
            Args::try_parse_from(args.into_iter().chain(["--vertical-contact-sweep"])).is_err()
        );
        assert!(
            Args::try_parse_from(args.into_iter().chain(["--line-drive-lift-comparison"])).is_err()
        );

        use BattingResult::*;
        let mut first = VerticalContactOutcomes::default();
        for (speed, angle, result) in [
            (45.0, 7.0, Single),
            (44.9, 8.0, Single),
            (46.0, 9.0, Foul),
            (45.0, 10.0, Double),
            (46.0, 14.9, DoublePlay),
            (46.0, 15.0, Triple),
            (f64::INFINITY, 12.0, Out),
            (40.0, 10.0, Single),
            (47.5, 10.0, Out),
            (49.0, 20.0, HomeRun),
            (46.0, 30.0, HomeRun),
            (45.0, 20.0, Double),
            (47.5, 20.0, Double),
            (45.0, 25.0, Double),
        ] {
            first.observe_ball(speed, angle, result);
        }
        let mut second = VerticalContactOutcomes::default();
        second.observe_ball(46.0, 8.0, Out);
        second.observe_ball(46.0, 12.0, FieldersChoice);
        first.merge(second);
        assert!(first.json(false).get("low_line_drive").is_none());
        let report = first.json(true);
        let low = &report["fast_fair_by_launch_angle_deg"]["[7,10)"];
        let next = &report["fast_fair_by_launch_angle_deg"]["[10,15)"];
        assert_eq!((low["plays"].as_u64(), next["plays"].as_u64()), (Some(2), Some(4)));
        assert_eq!(low["single"], 1);
        assert_eq!(low["out"], 1);
        assert_eq!(next["double"], 1);
        assert_eq!(next["double_play"], 1);
        assert_eq!(next["fielders_choice"], 1);
        assert_eq!(report["low_line_drive"]["plays"], 4);
        assert_eq!(report["low_line_drive"]["single"], 1);
        assert_eq!(report["home_runs_20_to_30_deg"], 1);
        assert_eq!(report["mid_speed_20_to_25_deg"]["double"], 1);
        assert_eq!(report["high_ev_home_runs_20_to_25_deg"], 1);
        assert!(report["whiffs_per_swing"].is_null());
    }

    #[test]
    fn vertical_contact_launch_bands_pool_doubles_and_keep_direction() {
        use BattingResult::*;
        let mut first = VerticalContactOutcomes::default();
        first.observe_launch(45.0, 15.0, Some(-15.0), Double);
        first.observe_launch(45.0, 15.0, Some(-15.0), Single);
        first.observe_launch(45.0, 15.0, Some(-15.0), Foul);
        first.observe_launch(45.0, 15.0, Some(15.0), Out);
        let mut second = VerticalContactOutcomes::default();
        second.observe_launch(46.0, 18.0, Some(-8.0), Double);
        second.observe_launch(46.0, 18.0, None, Double);
        first.merge(second);

        let report = first.json(true);
        let bands = &report["by_exit_speed_launch_angle_direction"];
        let third_base = &bands["EV_ms:[45,47.5)|VLA_deg:[15,20)|HLA_deg:[-15,-5)"];
        assert_eq!(third_base["plays"], 3);
        assert_eq!(third_base["double"], 2);
        assert_eq!(third_base["single"], 1);
        assert_eq!(bands["EV_ms:[45,47.5)|VLA_deg:[15,20)|HLA_deg:[15,30)"]["out"], 1);
        assert_eq!(bands["EV_ms:[45,47.5)|VLA_deg:[15,20)|HLA_deg:unknown"]["double"], 1);
        assert!(first.json(false).get("by_exit_speed_launch_angle_direction").is_none());
    }

    #[test]
    fn lift_offsets_keep_default_and_accept_full_schedule_shortlist() {
        let comparison = ["check", "--season", "2026", "--line-drive-lift-comparison"];
        let defaults = Args::try_parse_from(comparison).unwrap();
        assert_eq!(line_drive_lift_offsets(&defaults).unwrap(), [0, 10, 15, 20]);

        let select = |list| {
            Args::try_parse_from(
                comparison
                    .into_iter()
                    .chain(["--line-drive-lift-offsets", list]),
            )
        };
        let shortlist = select("0,10,20").unwrap();
        assert!(shortlist.games.is_none());
        assert_eq!(line_drive_lift_offsets(&shortlist).unwrap(), [0, 10, 20]);
        assert!(line_drive_lift_offsets(&select("0,10,10").unwrap()).is_err());
        assert!(
            Args::try_parse_from([
                "check",
                "--season",
                "2026",
                "--line-drive-lift-offsets",
                "0,10,20"
            ])
            .is_err()
        );
    }

    #[test]
    fn live_margin_override_restores_original_thresholds() {
        use saberbb::domain::resolver::fielding_physics::FielderRiskTolerance::*;
        assert_eq!(Aggressive.required_margin_sec(), 0.0);
        assert_eq!(Balanced.required_margin_sec(), 0.075);
        assert_eq!(Conservative.required_margin_sec(), 0.175);
        with_direct_catch_margin_scale(1.0, || {
            assert_eq!(Balanced.required_margin_sec(), 0.15);
            assert_eq!(Conservative.required_margin_sec(), 0.35);
        });
        assert_eq!(Balanced.required_margin_sec(), 0.075);
    }

    #[test]
    fn nonterminal_pitches_are_not_plate_appearances_or_at_bats() {
        let mut c = Counts::default();
        for r in [
            BattingResult::Take,
            BattingResult::Foul,
            BattingResult::StrikeSwung,
            BattingResult::HomeRun,
            BattingResult::Walk,
            BattingResult::Strikeout,
        ] {
            c.batting(r);
        }
        assert_eq!((c.pitches, c.pa, c.ab, c.hr, c.strikeouts), (6, 3, 2, 1, 1));
        assert_eq!(c.json()["ba"], 0.5);
    }
    #[test]
    fn batting_rates_use_their_own_denominators() {
        let empty = Counts::default().json();
        assert!(empty["babip"].is_null());
        assert!(empty["strikeouts_per_pa"].is_null());
        assert!(empty["singles_per_pa"].is_null());
        let mut counts = Counts::default();
        for result in [
            BattingResult::Single,
            BattingResult::Double,
            BattingResult::Out,
            BattingResult::FieldersChoice,
            BattingResult::HomeRun,
            BattingResult::Strikeout,
            BattingResult::Walk,
            BattingResult::Foul,
        ] {
            counts.batting(result);
        }
        let json = counts.json();
        assert_eq!(json["babip_balls_in_play"], 4);
        assert_eq!(json["babip"], 0.5);
        assert_eq!(json["strikeouts_per_pa"], 1.0 / 7.0);
        assert_eq!(json["singles_per_pa"], 1.0 / 7.0);
        assert!(counts.vertical_contact_json(false).get("slugging").is_none());
        let window_json = counts.vertical_contact_json(true);
        assert_eq!(window_json["slugging"], 7.0 / 6.0);
        assert_eq!(window_json["extra_base_hits_per_pa"], 2.0 / 7.0);
        assert_eq!(window_json["doubles_per_pa"], 1.0 / 7.0);
    }
}
