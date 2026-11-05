//! Causal three-position estimator and Python-compatible diagnostic sensor noise.
use super::swing_prediction::{Observation, Forecast, SwingPredictionInput, forecast_plate_crossing};
use super::forecast_swing_plan::{IntentPlanDiagnostics, diagnostic_intent_plans};
use crate::domain::strategy::batting_strategy::BattingIntentDecision;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const UPDATE_SOURCE: &str = "observed_intent_shadow_v1";
pub const SENSOR_SIGMA_M: f64 = 0.005;

// CPython Random(int).random()/gauss() compatibility, isolated from the game RNG.
struct SensorRandom { state: [u32; 624], index: usize, spare: Option<f64> }
impl SensorRandom {
    fn new(game_id: u32, count_seq: u16) -> Self {
        let bytes = Sha256::digest(format!("observation_sensor_v1:{game_id}:{count_seq}").as_bytes());
        let key: Vec<u32> = bytes.chunks_exact(4).rev().map(|b| u32::from_be_bytes(b.try_into().unwrap())).collect();
        let mut state = [0u32; 624]; state[0] = 19650218;
        for i in 1..624 { state[i] = (state[i-1] ^ (state[i-1] >> 30)).wrapping_mul(1812433253).wrapping_add(i as u32); }
        let (mut i, mut j) = (1usize, 0usize);
        for _ in 0..624 {
            state[i] = (state[i] ^ (state[i-1] ^ (state[i-1] >> 30)).wrapping_mul(1664525))
                .wrapping_add(key[j]).wrapping_add(j as u32);
            i += 1; j += 1;
            if i >= 624 { state[0] = state[623]; i = 1; }
            if j >= key.len() { j = 0; }
        }
        for _ in 0..623 {
            state[i] = (state[i] ^ (state[i-1] ^ (state[i-1] >> 30)).wrapping_mul(1566083941)).wrapping_sub(i as u32);
            i += 1;
            if i >= 624 { state[0] = state[623]; i = 1; }
        }
        state[0] = 0x80000000;
        Self { state, index: 624, spare: None }
    }
    fn word(&mut self) -> u32 {
        if self.index == 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x80000000) | (self.state[(i+1)%624] & 0x7fffffff);
                self.state[i] = self.state[(i+397)%624] ^ (y >> 1) ^ if y & 1 != 0 { 0x9908b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.state[self.index]; self.index += 1;
        y ^= y >> 11; y ^= (y << 7) & 0x9d2c5680; y ^= (y << 15) & 0xefc60000; y ^= y >> 18;
        y
    }
    fn random(&mut self) -> f64 {
        let a = self.word() >> 5; let b = self.word() >> 6;
        (a as f64 * 67108864.0 + b as f64) / 9007199254740992.0
    }
    fn gauss(&mut self) -> f64 {
        if let Some(z) = self.spare.take() { return z; }
        let angle = self.random() * (2.0 * std::f64::consts::PI);
        let radius = (-2.0 * (1.0 - self.random()).ln()).sqrt();
        self.spare = Some(angle.sin() * radius);
        angle.cos() * radius
    }
}

pub fn standard_noise(game_id: u32, count_seq: u16) -> [[f64; 3]; 3] {
    let mut rng = SensorRandom::new(game_id, count_seq);
    std::array::from_fn(|_| std::array::from_fn(|_| rng.gauss()))
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UpdatedEstimate {
    pub observations: Vec<Observation>,
    pub velocity: [f64; 3],
    pub acceleration: [f64; 3],
    pub forecast: Forecast,
}

/// Observations and supplied noise only; simulation truth is not an estimator input.
pub fn update_forecast(history: &[Observation], sigma: f64, eye: f64, noise: [[f64; 3]; 3])
    -> Result<UpdatedEstimate, &'static str>
{
    if history.len() != 3 || !sigma.is_finite() || sigma < 0.0 || !eye.is_finite()
        || !noise.iter().flatten().all(|x| x.is_finite())
        || !history.iter().all(|o| o.time_sec.is_finite() && o.time_sec > 0.0 && o.position_world_m.iter().all(|x| x.is_finite()))
    { return Err("invalid_observation_update_input"); }
    let (t0, t1, t2) = (history[0].time_sec, history[1].time_sec, history[2].time_sec);
    if !(t0 < t1 && t1 < t2) { return Err("invalid_estimation_clock"); }
    let scale = sigma * (-0.5 * eye.clamp(-4.0, 4.0)).exp();
    let observations: Vec<_> = history.iter().zip(noise).map(|(o, n)| Observation {
        time_sec: o.time_sec, position_world_m: std::array::from_fn(|i| o.position_world_m[i] + scale * n[i]),
    }).collect();
    let mut acceleration = std::array::from_fn(|i| 2.0 * ((observations[2].position_world_m[i] - observations[1].position_world_m[i]) / (t2-t1)
        - (observations[1].position_world_m[i] - observations[0].position_world_m[i]) / (t1-t0)) / (t2-t0));
    let mut velocity = std::array::from_fn(|i| (observations[2].position_world_m[i] - observations[1].position_world_m[i]) / (t2-t1)
        + 0.5 * acceleration[i] * (t2-t1));
    acceleration[1] = 0.0;
    velocity[1] = (observations[2].position_world_m[1] - observations[0].position_world_m[1]) / (t2-t0);
    let forecast = forecast_plate_crossing(&observations[2], velocity, acceleration)
        .map_err(|reason| if reason == "invalid_forecast_observation_or_velocity" { "invalid_forecast_input" } else { reason })?;
    Ok(UpdatedEstimate { observations, velocity, acceleration, forecast })
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ObservedIntentDiagnostics {
    pub version: u8,
    pub source: String,
    pub game_id: u32,
    pub count_seq: u16,
    pub sensor_sigma_m: f64,
    pub eye: f64,
    pub standard_noise: [[f64; 3]; 3],
    pub decision: BattingIntentDecision,
    pub invalid_reason: Option<String>,
    pub estimate: Option<UpdatedEstimate>,
    pub intent_plans: Option<IntentPlanDiagnostics>,
}

pub fn diagnostic_observed_intents(p: &SwingPredictionInput, game_id: u32, count_seq: u16,
    decision: BattingIntentDecision) -> ObservedIntentDiagnostics
{
    let noise = standard_noise(game_id, count_seq);
    let result = (|| {
        let history = p.observation_history.as_deref().ok_or("missing_observation_history")?;
        if history.len() != 3 { return Err("invalid_history_count"); }
        if !history.iter().all(|o| o.time_sec.is_finite() && o.time_sec > 0.0) { return Err("invalid_history_clock"); }
        if !(history[0].time_sec < history[1].time_sec && history[1].time_sec < history[2].time_sec) { return Err("unordered_history"); }
        for (o, ratio) in history.iter().zip([0.2, 0.4, 0.6]) {
            if (o.time_sec - ratio * p.prior_flight_time_sec).abs() > 1e-10 || !o.position_world_m.iter().all(|x| x.is_finite()) {
                return Err("history_clock_or_position");
            }
            if o.position_world_m[1] <= 0.0 { return Err("history_not_before_plate"); }
        }
        if (history[2].time_sec - p.observation.time_sec).abs() > 1e-10
            || history[2].position_world_m.iter().zip(p.observation.position_world_m).any(|(a,b)| (a-b).abs() > 1e-10)
        { return Err("history_commit_mismatch"); }
        update_forecast(history, SENSOR_SIGMA_M, p.batting_eye, noise)
    })();
    let (estimate, intent_plans, invalid_reason) = match result {
        Ok(estimate) => {
            let mut updated = p.clone(); updated.forecast = estimate.forecast.clone();
            // Same angular execution error as the saved prior, as in the Python diagnostic.
            let plans = diagnostic_intent_plans(&updated);
            (Some(estimate), Some(plans), None)
        }
        Err(reason) => (None, None, Some(reason.into())),
    };
    ObservedIntentDiagnostics { version: 1, source: UPDATE_SOURCE.into(), game_id, count_seq,
        sensor_sigma_m: SENSOR_SIGMA_M, eye: p.batting_eye, standard_noise: noise, decision,
        invalid_reason, estimate, intent_plans }
}

pub fn attach_observed_intent_plans(record: &mut super::contact_state::ContactStateRecord,
    game_id: u32, decision: BattingIntentDecision)
{
    if let Some(state) = record.state.as_mut() {
        state.observed_intent_plans = state.swing_prediction.as_ref()
            .filter(|p| p.invalid_reason.is_none()).and_then(|p| p.input.as_ref())
            .map(|p| diagnostic_observed_intents(p, game_id, record.count_seq, decision));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn same(a: &Value, b: &Value) {
        match (a, b) {
            (Value::Object(a), Value::Object(b)) => {
                assert_eq!(a.len(), b.len());
                for (k, v) in a { same(v, &b[k]); }
            }
            (Value::Array(a), Value::Array(b)) => {
                assert_eq!(a.len(), b.len());
                for (x,y) in a.iter().zip(b) { same(x,y); }
            }
            (Value::Number(a), Value::Number(b)) => assert!((a.as_f64().unwrap()-b.as_f64().unwrap()).abs() < 1e-9, "{a} != {b}"),
            _ => assert_eq!(a,b),
        }
    }

    #[test]
    fn python_sensor_noise_and_updated_forecast_match() {
        let fixture: Value = serde_json::from_str(include_str!("../../../tests/fixtures/observation_update_contract.json")).unwrap();
        for c in fixture["cases"].as_array().unwrap() {
            let noise = standard_noise(c["game_id"].as_u64().unwrap() as u32, c["count_seq"].as_u64().unwrap() as u16);
            same(&c["noise"], &serde_json::to_value(noise).unwrap());
            let history: Vec<Observation> = serde_json::from_value(c["history"].clone()).unwrap();
            let result = update_forecast(&history, c["sigma"].as_f64().unwrap(), c["eye"].as_f64().unwrap(), noise).unwrap();
            same(&c["expected_estimate"], &serde_json::to_value(result).unwrap());
        }
    }

    #[test]
    fn estimator_rejects_missing_or_future_pointing_observations() {
        assert!(update_forecast(&[], SENSOR_SIGMA_M, 0.0, [[0.0;3];3]).is_err());
        let history = vec![Observation { time_sec: 0.1, position_world_m: [0.0,1.0,1.0] };
            3];
        assert_eq!(update_forecast(&history, 0.0, 0.0, [[0.0;3];3]).unwrap_err(), "invalid_estimation_clock");
    }
}
