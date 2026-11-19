use super::{Flight, fork_rng};
use crate::domain::{
    resolver::{
        batting_resolver::{FairFenceEvent, replay_fence_flight},
        fielding_physics::{
            AirCatchProbeDiagnostics, CatchType, FieldingCatchDiagnostics,
            probe_integrated_airborne_catches,
        },
        fielding_resolver::process_fielding,
    },
    shared::{
        ball::OutboundResult,
        game_state::{ActiveFielder, GameError, GameState},
        player::Position,
    },
};
use serde::Serialize;

#[derive(Serialize)]
pub struct PairedFenceAudit {
    pub flat: Option<FenceAudit>,
    pub by_baseline_ev: Option<FenceAudit>,
}

impl PairedFenceAudit {
    pub(super) fn compare(
        live: &GameState,
        flat: Flight,
        by_baseline_ev: Flight,
    ) -> Result<Option<Self>, GameError> {
        if ![flat, by_baseline_ev]
            .iter()
            .flatten()
            .any(|(ball, _)| ball.outbound_result == OutboundResult::GroundRuleDouble)
        {
            return Ok(None);
        }
        let fielders = live.current_fielders();
        Ok(Some(Self {
            flat: inspect(live, flat, &fielders)?,
            by_baseline_ev: inspect(live, by_baseline_ev, &fielders)?,
        }))
    }
}

#[derive(Serialize)]
pub struct FenceAudit {
    fair_fence_events: Vec<FairFenceEvent>,
    pub clean_direct_catch_in_fielding_replay: bool,
    pub integrated_path_reachable: bool,
    pub integrated_path_within_risk_margin: bool,
    pub integrated_path_within_risk_margin_and_eligible: bool,
    selected_by_fielding_replay: FieldingCatchDiagnostics,
    fielders: Vec<FielderProbe>,
}

#[derive(Serialize)]
struct FielderProbe {
    fielder_id: i64,
    fielder_position: Position,
    lane_eligible: bool,
    distance_eligible: bool,
    required_margin_sec: f64,
    estimated_path: Option<AirCatchProbeDiagnostics>,
    integrated_path: AirCatchProbeDiagnostics,
}

fn inspect(
    live: &GameState,
    flight: Flight,
    fielders: &[ActiveFielder],
) -> Result<Option<FenceAudit>, GameError> {
    let Some((ball, metrics)) = flight else {
        return Ok(None);
    };
    let mut air = Vec::new();
    let fair_fence_events =
        replay_fence_flight(&ball, metrics.final_hla_deg, &live.stadium, |sample| {
            if !sample.after_bounce {
                air.push(sample);
            }
        })?;
    let integrated = probe_integrated_airborne_catches(&ball, &air, fielders);

    // Start from the same post-launch RNG state used by the official paired play.
    let replay = process_fielding(fork_rng(live.rng.as_ref())?.as_mut(), fielders, &ball)?;
    let selected = replay.diagnostics();
    let fielders: Vec<_> = replay
        .candidate_diagnostics
        .into_iter()
        .zip(integrated)
        .map(|(candidate, integrated_path)| FielderProbe {
            fielder_id: candidate.fielder_id,
            fielder_position: candidate.fielder_position,
            lane_eligible: candidate.lane_eligible,
            distance_eligible: candidate.distance_eligible,
            required_margin_sec: candidate.required_margin_sec,
            estimated_path: candidate.air_probe,
            integrated_path,
        })
        .collect();

    Ok(Some(FenceAudit {
        fair_fence_events,
        clean_direct_catch_in_fielding_replay: selected.catch_type == CatchType::DirectFly
            && selected.error_type.is_none(),
        integrated_path_reachable: fielders
            .iter()
            .any(|f| f.integrated_path.reachable_samples > 0),
        integrated_path_within_risk_margin: fielders
            .iter()
            .any(|f| f.integrated_path.risk_ok_samples > 0),
        integrated_path_within_risk_margin_and_eligible: fielders.iter().any(|f| {
            f.lane_eligible && f.distance_eligible && f.integrated_path.risk_ok_samples > 0
        }),
        selected_by_fielding_replay: selected,
        fielders,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{
        random_provider::RealRng,
        resolver::batting_resolver::{GrounderFlightSample, calculate_trajectory},
        shared::{ball::BattedBall, game_stats::PlayerGameBattingMetrics, stadium::Stadium},
        test_support::{active_fielder, game_schedule},
        util::PolarPosition,
    };

    #[test]
    fn measures_fence_height_before_clearance_or_reflection() {
        let mut stadium = Stadium::new(1, "Low fence".into(), 20.0, 25.0, 0.0);
        let launch =
            |stadium: &Stadium| calculate_trajectory(40.0, -10.0, 0.0, 0.0, 0.0, stadium).unwrap();
        let ball = launch(&stadium);
        let mut samples = Vec::new();
        let events = replay_fence_flight(&ball, 0.0, &stadium, |s| samples.push(s)).unwrap();
        assert_eq!(ball.outbound_result, OutboundResult::GroundRuleDouble);
        assert_eq!(events.len(), 1);
        let clearance = &events[0];
        assert_eq!(clearance.time_sec, ball.total_time);
        assert!(clearance.bounce_count > 0);
        let first_bounce = samples.iter().find(|sample| sample.after_bounce).unwrap();
        assert_eq!(Some(first_bounce.time_sec), ball.first_bounce_time);
        assert!(first_bounce.time_sec < clearance.time_sec);
        assert!(clearance.height_above_fence_m > 0.0);

        stadium.fence_height = clearance.ball_height_m + 0.5;
        let ball = launch(&stadium);
        let events = replay_fence_flight(&ball, 0.0, &stadium, |_| {}).unwrap();
        let impact = &events[0];
        assert_eq!(Some(impact.time_sec), ball.fence_impact_time);
        assert_eq!(impact.time_sec, clearance.time_sec);
        assert_eq!(impact.ball_height_m, clearance.ball_height_m);
        assert!((impact.height_above_fence_m + 0.5).abs() < 1e-12);
    }

    #[test]
    fn integrated_probe_stops_at_the_first_fence_contact() {
        let position = PolarPosition::new(80.0, 0.0);
        let ball = BattedBall::new(
            46.0,
            18.0,
            80.0,
            0.0,
            3.0,
            Some(position),
            Some(2.0),
            Some(position),
            Some(1.5),
            OutboundResult::InField,
        );
        let mut fielder = active_fielder(Position::CF, 80.0, 0.0);
        fielder.info.prep_time = 0.0;
        let flight = [0.09, 1.49, 1.50, 1.51].map(|time_sec| GrounderFlightSample {
            time_sec,
            position,
            height_m: if time_sec < 1.5 { 10.0 } else { 1.0 },
            horizontal_speed_ms: 20.0,
            after_bounce: false,
        });
        let probes = probe_integrated_airborne_catches(&ball, &flight, &[fielder]);
        assert_eq!(probes[0].air_samples, 1);
        assert_eq!(probes[0].risk_ok_samples, 0);
    }

    #[test]
    fn inspects_both_paths_without_consuming_live_rng() {
        let mut schedule = game_schedule();
        schedule.stadium = Stadium::new(1, "Low fence".into(), 20.0, 25.0, 0.0);
        let mut live = GameState::new(Box::new(RealRng::from_seed(113)), schedule).unwrap();
        live.advance_half_inning().unwrap();
        let mut control = fork_rng(live.rng.as_ref()).unwrap();

        let flight = |direction| {
            let ball =
                calculate_trajectory(40.0, -10.0, direction, 0.0, 0.0, &live.stadium).unwrap();
            let metrics = PlayerGameBattingMetrics {
                final_hla_deg: direction,
                ..PlayerGameBattingMetrics::from_contact(&Default::default())
            };
            Some((ball, metrics))
        };
        let flat = flight(0.0);
        let foul = flight(60.0);
        assert!(
            PairedFenceAudit::compare(&live, foul, foul)
                .unwrap()
                .is_none()
        );
        let paired = PairedFenceAudit::compare(&live, flat, foul)
            .unwrap()
            .unwrap();
        let flat_audit = paired.flat.unwrap();
        let foul_audit = paired.by_baseline_ev.unwrap();
        assert_eq!(flat_audit.fair_fence_events.len(), 1);
        assert!(foul_audit.fair_fence_events.is_empty());
        let first_bounce_time = flat.unwrap().0.first_bounce_time.unwrap();
        for fielder in flat_audit.fielders {
            if let Some(catch_time) = fielder.integrated_path.first_catch_time_sec {
                assert!(catch_time < first_bounce_time);
            }
        }
        assert_eq!(live.rng.random(), control.random());
    }
}
