//! Frozen finite-bat responses on diagnostic contacts only, never fed back to intent selection.
use super::ball::PitchedBall;
use super::contact_state::{ContactStateRecord, ContactStateSnapshot};
use super::finite_bat_collision::{self as finite, CollisionGeometry, CollisionParameters, FiniteCollisionInput, PitchSpinInput};
use super::forecast_swing_plan::{add, sub, mul, dot};
use super::intent_execution_contact::{execution_pose, IntentContactProbe};
use crate::domain::strategy::batting_strategy::{BattedBallIntent, BattingIntentDecision};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpinReconstruction {
    pub input: PitchSpinInput,
    pub invalid_reason: Option<String>,
    pub world_rad_s: Option<[f64;3]>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollisionEvaluation {
    pub input: Option<FiniteCollisionInput>,
    pub response: Value,
    pub control: Option<Value>,
    pub control_checks: BTreeMap<String,f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntentCollisionProbe {
    pub intent: BattedBallIntent,
    pub selected: bool,
    pub invalid_reason: Option<String>,
    pub evaluation: Option<CollisionEvaluation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IntentCollisionDiagnostics {
    pub version: u8,
    pub source: String,
    pub model: String,
    pub decision: BattingIntentDecision,
    pub spin: SpinReconstruction,
    pub probes: Vec<IntentCollisionProbe>,
}

fn skipped(reason: &str) -> CollisionEvaluation {
    CollisionEvaluation { input:None, response:json!({"collision_status":format!("not_evaluated:{reason}")}),
        control:None, control_checks:BTreeMap::new() }
}
fn norm(v: [f64;3]) -> f64 { dot(v,v).sqrt() }
fn validate_checks(result: &Value) -> Result<(), &'static str> {
    if let Some(checks)=result.get("checks").and_then(Value::as_object) {
        if !checks.values().all(|v|v.as_f64().is_some_and(|x|x.is_finite() && x>=0.0 && x<=1e-7)) {
            return Err("collision_identity_mismatch");
        }
    }
    Ok(())
}

pub fn evaluate(state: &ContactStateSnapshot, probe: &IntentContactProbe, spin: &SpinReconstruction)
    -> Result<CollisionEvaluation, &'static str>
{
    if let Some(reason)=probe.invalid_reason.as_deref() { return Ok(skipped(reason)); }
    let search=probe.search.as_ref().ok_or("missing_contact_search")?;
    if search.status!="contact" { return Ok(skipped(&search.status)); }
    let sample=search.geometry.as_ref().ok_or("missing_contact_geometry")?;
    let Some(normal)=sample.normal else { return Ok(skipped("missing_normal")); };
    let offset=search.time_offset_sec.ok_or("missing_contact_time")?;
    let plans=state.observed_intent_plans.as_ref().and_then(|o|o.intent_plans.as_ref()).ok_or("missing_observed_plans")?;
    let plan=plans.plans.iter().find(|p|p.intent==probe.intent).and_then(|p|p.execution_plan.as_ref()).ok_or("missing_execution_plan")?;
    let g=CollisionGeometry { time_since_release_sec:state.input.reference_time_sec_since_release+offset,
        axis_world:sample.axis, normal_world:normal, axial_center_m:sample.axial_m,
        closest_axial_m:sample.closest_axial_m, half_length_m:plan.half_length_m, region:sample.contact_region.clone() };
    g.validate()?;
    let vin=state.input.ball_velocity_world_ms; let vbat=sample.bat_velocity;
    if dot(sub(vin,vbat),normal)>=0.0 { return Ok(skipped("non_approaching")); }
    if g.region!="side" { return Ok(skipped("unsupported_cap")); }
    let Some(e)=finite::profile_e(0.71+g.closest_axial_m) else { return Ok(skipped("out_of_curve")); };
    let Some(incoming)=spin.world_rad_s else { return Ok(skipped("invalid_pitch_spin")); };
    let phase=offset-(plan.nominal_time_sec-state.input.reference_time_sec_since_release);
    let omega=execution_pose(plan,phase).angular_velocity_world_rad_s;
    let radius=state.execution_contact.as_ref().ok_or("missing_contact_diagnostics")?.settings.effective_radius_m;
    let input=FiniteCollisionInput { geometry:g,vin,centerline_velocity:vbat,ball_omega:incoming,bat_omega:omega,
        parameters:CollisionParameters::frozen(e,radius) };
    let response=finite::solve(&input)?;
    let mut control_input=input.clone(); control_input.parameters.mu=0.0; control_input.parameters.et=0.0;
    let control=finite::solve(&control_input)?;
    validate_checks(&response)?; validate_checks(&control)?;
    if control["collision_status"]!="impulse_contact" { return Err("normal_control_unavailable"); }
    let vector=|name:&str| -> Result<[f64;3], &'static str> {
        let a=control[name].as_array().ok_or("missing_control_vector")?;
        if a.len()!=3 { return Err("invalid_control_vector"); }
        Ok([a[0].as_f64().ok_or("invalid_control_vector")?,a[1].as_f64().ok_or("invalid_control_vector")?,a[2].as_f64().ok_or("invalid_control_vector")?])
    };
    let p=input.parameters;
    let inverse=1.0/p.mass+(input.geometry.closest_axial_m-p.cm_offset).powi(2)/p.transverse_inertia;
    let relative_normal=dot(sub(vin,vbat),normal);
    let delta=(-(1.0+e)*relative_normal/(1.0+p.ball_mass*inverse)).max(0.0);
    let expected=add(vin,mul(normal,delta));
    let control_checks=BTreeMap::from([
        ("outgoing_ms".into(),norm(sub(vector("outgoing_world_ms")?,expected))),
        ("unchanged_spin_rad_s".into(),norm(sub(vector("outgoing_spin_rad_s")?,incoming))),
    ]);
    if !control_checks.values().all(|x:&f64|x.is_finite() && *x<=1e-7) { return Err("normal_control_mismatch"); }
    Ok(CollisionEvaluation { input:Some(input),response,control:Some(control),control_checks })
}

pub fn attach_collision_responses(record: &mut ContactStateRecord, ball: &PitchedBall) {
    let Some(state)=record.state.as_mut() else { return; };
    let Some(contact)=state.execution_contact.as_ref() else { return; };
    let finite_value=|x:f64| if x.is_finite() { Some(x) } else { None };
    let input=PitchSpinInput { spin_rate:finite_value(ball.spin_rate),spin_angle:finite_value(ball.spin_angle),
        spin_efficiency:finite_value(ball.spin_efficiency),gyro_sign:0 };
    let (value,reason)=match finite::reconstruct_spin(input,state.input.ball_velocity_world_ms) {
        Ok(value)=>(Some(value),None), Err(reason)=>(None,Some(reason.into())),
    };
    let spin=SpinReconstruction { input,world_rad_s:value,invalid_reason:reason };
    let probes=contact.probes.iter().map(|probe| {
        let (evaluation,error)=match evaluate(state,probe,&spin) {
            Ok(value)=>(Some(value),None), Err(reason)=>(None,Some(reason.into())),
        };
        IntentCollisionProbe { intent:probe.intent, selected:probe.selected, invalid_reason:error, evaluation }
    }).collect();
    state.intent_collision=Some(IntentCollisionDiagnostics { version:1,source:"intent_collision_shadow_v1".into(),
        model:finite::MODEL.into(),decision:contact.decision,spin,probes });
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::finite_bat_collision::tests::{fixture,same};
    #[test]
    fn intent_decisions_execution_phase_and_exclusions_match_python() {
        for case in fixture()["integration"].as_array().unwrap() {
            let state:ContactStateSnapshot=serde_json::from_value(case["state"].clone()).unwrap();
            let spin:SpinReconstruction=serde_json::from_value(case["expected"]["spin"].clone()).unwrap();
            for probe in &state.execution_contact.as_ref().unwrap().probes {
                let expected=case["expected"]["probes"].as_array().unwrap().iter().find(|p|p["intent"]==json!(probe.intent)).unwrap();
                let evaluation=evaluate(&state,probe,&spin).unwrap();
                same(&expected["evaluation"],&serde_json::to_value(evaluation).unwrap(),"evaluation");
                assert_eq!(expected["selected"],json!(probe.selected));
            }
            let before=serde_json::to_value(&state).unwrap();
            let mut record=ContactStateRecord { count_seq:1,state_version:1,state_source:"test".into(),invalid_reason:None,state:Some(state) };
            use crate::domain::shared::{ball::BallLocation,player::PitchType};
            use crate::domain::strategy::pitching_strategy::TargetZone;
            use crate::domain::util::Vector3D;
            let ball=PitchedBall { pitch_type:PitchType::FourSeamFastball,speed:40.0,
                spin_rate:case["spin"][0].as_f64().unwrap(),spin_angle:case["spin"][1].as_f64().unwrap(),
                spin_efficiency:case["spin"][2].as_f64().unwrap(),
                release_point:Vector3D{x:0.0,y:16.0,z:1.8},flight_time:0.4,aim_zone:TargetZone::Center,
                aim_location:BallLocation{x:0.0,y:0.0},actual_location:BallLocation{x:0.0,y:0.0} };
            attach_collision_responses(&mut record,&ball);
            let state=record.state.unwrap();
            same(&case["expected"],&serde_json::to_value(&state.intent_collision).unwrap(),"attached");
            let mut after=serde_json::to_value(state).unwrap();after.as_object_mut().unwrap().remove("intent_collision");
            assert_eq!(before,after);
        }
    }
}
