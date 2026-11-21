use super::outcome_stage;
use saberbb::domain::{
    resolver::batting_resolver::SwingContactType, shared::game_state::SwingReplay,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const TRIALS: [&str; 3] = ["edge_correction", "reduced_center_correction", "combined"];

#[derive(Default)]
struct Outcomes {
    contact_types: BTreeMap<String, u64>,
    stages: BTreeMap<&'static str, u64>,
}

impl Outcomes {
    fn observe(&mut self, play: SwingReplay) {
        *self
            .contact_types
            .entry(play.contact_type.as_ref().to_owned())
            .or_default() += 1;
        *self.stages.entry(outcome_stage(play)).or_default() += 1;
    }

    fn report(&self) -> Value {
        json!({"adjusted_contact_types": self.contact_types, "outcome_stages": self.stages})
    }
}

#[derive(Default)]
struct RetentionPairs {
    launches: u64,
    baseline_sum: f64,
    trial_sum: f64,
}

impl RetentionPairs {
    fn observe(&mut self, baseline: SwingReplay, trial: SwingReplay) {
        if let (Some(baseline), Some(trial)) =
            (baseline.exit_speed_retention, trial.exit_speed_retention)
        {
            self.launches += 1;
            self.baseline_sum += baseline;
            self.trial_sum += trial;
        }
    }

    fn report(&self) -> Value {
        let mean = |sum| (self.launches > 0).then(|| sum / self.launches as f64);
        json!({
            "paired_launches": self.launches,
            "baseline_mean_exit_speed_retention": mean(self.baseline_sum),
            "trial_mean_exit_speed_retention": mean(self.trial_sum),
        })
    }
}

#[derive(Default)]
struct TrialCounts {
    outcomes: Outcomes,
    rescued: Outcomes,
    transitions: BTreeMap<String, BTreeMap<String, u64>>,
    solid_to_weak: RetentionPairs,
    solid_to_marginal: RetentionPairs,
}

impl TrialCounts {
    fn observe(&mut self, baseline: SwingReplay, trial: SwingReplay) {
        self.outcomes.observe(trial);
        *self
            .transitions
            .entry(baseline.contact_type.as_ref().to_owned())
            .or_default()
            .entry(trial.contact_type.as_ref().to_owned())
            .or_default() += 1;
        match (baseline.contact_type, trial.contact_type) {
            (SwingContactType::SwungAndMiss, kind) if kind != SwingContactType::SwungAndMiss => {
                self.rescued.observe(trial);
            }
            (SwingContactType::SolidContact, SwingContactType::WeakContact) => {
                self.solid_to_weak.observe(baseline, trial);
            }
            (SwingContactType::SolidContact, SwingContactType::MarginalContact) => {
                self.solid_to_marginal.observe(baseline, trial);
            }
            _ => {}
        }
    }

    fn report(&self) -> Value {
        json!({
            "outcomes": self.outcomes.report(),
            "rescued_whiffs": self.rescued.report(),
            "baseline_to_trial_adjusted_contact_types": self.transitions,
            "solid_to_weak_retention": self.solid_to_weak.report(),
            "solid_to_marginal_retention": self.solid_to_marginal.report(),
        })
    }
}

#[derive(Default)]
pub(super) struct ContactProtection {
    swings: u64,
    baseline: Outcomes,
    trials: [TrialCounts; 3],
}

impl ContactProtection {
    pub(super) fn observe(&mut self, baseline: SwingReplay, trials: [SwingReplay; 3]) {
        self.swings += 1;
        self.baseline.observe(baseline);
        for (counts, trial) in self.trials.iter_mut().zip(trials) {
            counts.observe(baseline, trial);
        }
    }

    pub(super) fn report(&self) -> Value {
        let trials: BTreeMap<_, _> = TRIALS
            .into_iter()
            .zip(self.trials.iter().map(TrialCounts::report))
            .collect();
        json!({
            "definition": "Replays of the same live two-strike ContactFirst swings, with the original aim, speed, pitch and starting RNG. Edge correction moves realized bat-frame thickness error toward the middle by up to 12 mm, peaking at 70 mm absolute error and tapering to zero at 55 and 90 mm. Reduced center correction increases absolute thickness error by up to 14 mm, peaking at 25 mm and tapering to zero at 0 and 55 mm. The combined arm applies both. The live contact classifier and launch physics evaluate the resulting geometry. Retention pairs include fouls and use modeled exit speed after contact-offset losses relative to pre-offset exit speed. Subsequent pitches follow the live baseline.",
            "swings": self.swings,
            "baseline": self.baseline.report(),
            "trials": trials,
        })
    }
}
