//! Diagnostic port of finite_barrel_collision.py. No RNG, strategy or live results.
use super::forecast_swing_plan::{add, sub, mul, dot, cross};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::f64::consts::PI;
type V3 = [f64; 3];
fn norm(v: V3) -> f64 { dot(v,v).sqrt() }
fn inertia(v: V3, a: V3, transverse: f64, axial: f64) -> V3 {
    add(mul(v,transverse),mul(a,(axial-transverse)*dot(v,a)))
}

pub const MODEL: &str = "finite_I0.00015_et0.4";
pub const PROFILE_KNOTS: [(f64,f64);16] = [(0.54,0.22),(0.56,0.24),(0.58,0.28),(0.60,0.31),
    (0.62,0.35),(0.64,0.40),(0.66,0.45),(0.68,0.48),(0.70,0.50),(0.72,0.50),(0.74,0.49),(0.76,0.43),
    (0.78,0.38),(0.80,0.31),(0.82,0.24),(0.84,0.21)];

pub fn profile_e(z: f64) -> Option<f64> {
    if !z.is_finite() || z < PROFILE_KNOTS[0].0 || z > PROFILE_KNOTS[15].0 { return None; }
    for (i, &(x,y)) in PROFILE_KNOTS.iter().enumerate() {
        if z == x { return Some(y); }
        if z < x {
            let (left,value) = PROFILE_KNOTS[i-1];
            return Some(value+(y-value)*(z-left)/(x-left));
        }
    }
    None
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CollisionGeometry {
    pub time_since_release_sec: f64,
    pub axis_world: V3,
    pub normal_world: V3,
    pub axial_center_m: f64,
    pub closest_axial_m: f64,
    pub half_length_m: f64,
    pub region: String,
}
impl CollisionGeometry {
    pub fn validate(&self) -> Result<(), &'static str> {
        if !self.axis_world.iter().chain(self.normal_world.iter()).all(|v|v.is_finite()) { return Err("invalid_contact_vector"); }
        if ![self.time_since_release_sec,self.axial_center_m,self.closest_axial_m,self.half_length_m].iter().all(|v|v.is_finite())
            || self.time_since_release_sec<0.0 || self.half_length_m<=0.0 { return Err("invalid_contact_scalar"); }
        if (dot(self.axis_world,self.axis_world)-1.0).abs()>1e-8 || (dot(self.normal_world,self.normal_world)-1.0).abs()>1e-8 {
            return Err("non_unit_contact_frame");
        }
        let expected = self.axial_center_m.clamp(-self.half_length_m,self.half_length_m);
        let region = if self.axial_center_m.abs()<=self.half_length_m { "side" } else { "cap" };
        if (self.closest_axial_m-expected).abs()>1e-8 || self.region!=region { return Err("inconsistent_contact_region"); }
        let normal_axis = dot(self.axis_world,self.normal_world);
        if self.region=="side" && normal_axis.abs()>1e-8 { return Err("side_normal_not_perpendicular"); }
        if self.region=="cap" && normal_axis*(self.axial_center_m-self.closest_axial_m)< -1e-8 { return Err("cap_normal_wrong_side"); }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CollisionParameters {
    pub mass: f64,
    pub transverse_inertia: f64,
    pub axial_inertia: f64,
    pub cm_offset: f64,
    pub barrel_radius: f64,
    pub restitution: f64,
    pub mu: f64,
    pub et: f64,
    pub ball_mass: f64,
    pub ball_radius: f64,
    pub inertia_factor: f64,
}
impl CollisionParameters {
    pub fn frozen(restitution: f64, effective_radius: f64) -> Self {
        Self { mass:0.885, transverse_inertia:0.885*0.23_f64.powi(2), axial_inertia:0.00015,
            cm_offset:0.564-0.71, barrel_radius:effective_radius-0.036, restitution, mu:0.2, et:0.4,
            ball_mass:0.145, ball_radius:0.036, inertia_factor:0.4 }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FiniteCollisionInput {
    pub geometry: CollisionGeometry,
    pub vin: V3,
    pub centerline_velocity: V3,
    pub ball_omega: V3,
    pub bat_omega: V3,
    pub parameters: CollisionParameters,
}

fn unavailable(reason: &str) -> Value {
    json!({"collision_status":reason,"ev_ms":null,"hla_deg":null,"vla_deg":null})
}

pub fn solve(input: &FiniteCollisionInput) -> Result<Value, &'static str> {
    let g=&input.geometry; let p=input.parameters;
    g.validate()?;
    if ![p.restitution,p.mu,p.et].iter().all(|x|x.is_finite()) || !(0.0..=1.0).contains(&p.restitution)
        || p.mu<0.0 || !(0.0..=1.0).contains(&p.et) { return Err("invalid_impulse_parameters"); }
    if g.region!="side" { return Err("unsupported_cap"); }
    if !input.vin.iter().chain(input.centerline_velocity.iter()).chain(input.ball_omega.iter())
        .chain(input.bat_omega.iter()).all(|x|x.is_finite()) { return Err("invalid_finite_velocity"); }
    if ![p.mass,p.transverse_inertia,p.axial_inertia,p.cm_offset,p.barrel_radius].iter().all(|x|x.is_finite())
        || p.mass<=0.0 || p.transverse_inertia<=0.0 || p.axial_inertia<=0.0 || p.barrel_radius<0.0 { return Err("invalid_finite_parameters"); }
    if ![p.ball_mass,p.ball_radius,p.inertia_factor].iter().all(|x|x.is_finite())
        || p.ball_mass<=0.0 || p.ball_radius<=0.0 || p.inertia_factor<=0.0 { return Err("invalid_sphere_parameters"); }
    let bi=p.inertia_factor*p.ball_mass*p.ball_radius.powi(2);
    if !bi.is_finite() || bi<=0.0 { return Err("invalid_sphere_inertia"); }
    let a=g.axis_world; let n=g.normal_world; let t=cross(a,n);
    let d=g.closest_axial_m-p.cm_offset;
    let line_arm=mul(a,d); let arm=add(line_arm,mul(n,p.barrel_radius)); let rb=mul(n,-p.ball_radius);
    let surface=add(input.centerline_velocity,cross(input.bat_omega,mul(n,p.barrel_radius)));
    let center=sub(input.centerline_velocity,cross(input.bat_omega,line_arm));
    let u=sub(add(input.vin,cross(input.ball_omega,rb)),surface);
    let un=dot(u,n); let ua=dot(u,a); let ut=dot(u,t);
    if un>=0.0 { return Ok(unavailable("non_approaching")); }
    let kn=1.0/p.ball_mass+1.0/p.mass+d*d/p.transverse_inertia;
    let ka=1.0/p.ball_mass+1.0/p.mass+p.barrel_radius.powi(2)/p.transverse_inertia+p.ball_radius.powi(2)/bi;
    let kt=kn+p.barrel_radius.powi(2)/p.axial_inertia+p.ball_radius.powi(2)/bi;
    let c=-d*p.barrel_radius/p.transverse_inertia; let sa=ka-c*c/kn;
    if ![kn,ka,kt,c,sa].iter().all(|x|x.is_finite()) || kn<=0.0 || sa<=0.0 || kt<=0.0 { return Err("invalid_contact_mass_matrix"); }
    let j0=-(1.0+p.restitution)*un/kn;
    let desired_a=(-(1.0+p.et)*ua-c*j0)/sa;
    let desired_t=-(1.0+p.et)*ut/kt;
    let size=desired_a.hypot(desired_t);
    let scale=if size==0.0 { 1.0 } else {
        let denominator=size+p.mu*c*desired_a/kn;
        if denominator>0.0 { (p.mu*j0/denominator).min(1.0) } else { 1.0 }
    };
    let ja=scale*desired_a; let jt=scale*desired_t; let jn=j0-c*ja/kn;
    if ![jn,ja,jt,scale].iter().all(|x|x.is_finite()) { return Err("non_finite_coupled_impulse"); }
    if jn<=0.0 { return Ok(unavailable("not_evaluated:tensile_normal_target")); }
    let impulse=add(mul(n,jn),add(mul(a,ja),mul(t,jt)));
    let kj=[kn*jn+c*ja,c*jn+ka*ja,kt*jt];
    let change=un*jn+ua*ja+ut*jt+0.5*(jn*kj[0]+ja*kj[1]+jt*kj[2]);
    if change>1e-9 {
        let mut result=unavailable("not_evaluated:energy_increasing_target");
        result["candidate_energy_gain_j"]=json!(change); return Ok(result);
    }
    let loss=(-change).max(0.0);
    let out=add(input.vin,mul(impulse,1.0/p.ball_mass));
    let spin=add(input.ball_omega,mul(cross(rb,impulse),1.0/bi));
    let dv=mul(impulse,-1.0/p.mass);
    let dw=inertia(mul(cross(arm,impulse),-1.0),a,1.0/p.transverse_inertia,1.0/p.axial_inertia);
    let after_omega=add(input.bat_omega,dw);
    let bat_energy=|v,w| 0.5*p.mass*dot(v,v)+0.5*dot(w,inertia(w,a,p.transverse_inertia,p.axial_inertia));
    let energy=|v,w| 0.5*p.ball_mass*dot(v,v)+0.5*bi*dot(w,w);
    let before_bat=bat_energy(center,input.bat_omega); let after_bat=bat_energy(add(center,dv),after_omega);
    let before_ball=energy(input.vin,input.ball_omega); let after_ball=energy(out,spin);
    let post=sub(add(out,cross(spin,rb)),add(surface,add(dv,cross(dw,arm))));
    let post_components=[dot(post,n),dot(post,a),dot(post,t)];
    let delta_spin_l=mul(sub(spin,input.ball_omega),bi);
    let delta_bat_l=inertia(dw,a,p.transverse_inertia,p.axial_inertia);
    let total_delta_l=add(cross(sub(arm,rb),mul(sub(out,input.vin),p.ball_mass)),add(delta_spin_l,delta_bat_l));
    let mut checks=BTreeMap::from([
        ("energy_balance_j",(after_bat+after_ball-before_bat-before_ball+loss).abs()),
        ("normal_restitution_ms",(post_components[0]+p.restitution*un).abs()),
        ("matrix_response_ms",norm(sub(post,add(u,add(mul(n,kj[0]),add(mul(a,kj[1]),mul(t,kj[2]))))))),
        ("linear_impulse_kg_ms",norm(add(mul(sub(out,input.vin),p.ball_mass),mul(dv,p.mass)))),
        ("ball_angular_impulse",norm(sub(delta_spin_l,cross(rb,impulse)))),
        ("bat_angular_impulse",norm(add(delta_bat_l,cross(arm,impulse)))),
        ("total_angular_impulse",norm(total_delta_l)),
        ("friction_cone_excess",(ja.hypot(jt)-p.mu*jn).max(0.0)),
        ("negative_dissipation_j",(-loss).max(0.0)),
    ]);
    let regime=if scale<1.0-1e-12 { "coulomb_limited" } else { "restitution_target" };
    let tangent_error=(post_components[1]+p.et*ua).hypot(post_components[2]+p.et*ut);
    if regime=="restitution_target" { checks.insert("tangent_restitution_ms",tangent_error); }
    let ev=norm(out); let horizontal=out[0].hypot(out[1]);
    let hla=if horizontal>1e-12 { Some(out[0].atan2(out[1]).to_degrees()) } else { None };
    let vla=if ev>1e-12 { Some(out[2].atan2(horizontal).to_degrees()) } else { None };
    if !out.iter().chain(spin.iter()).chain(post.iter()).chain([loss,before_bat,after_bat].iter())
        .chain(checks.values()).all(|x|x.is_finite()) { return Err("non_finite_finite_result"); }
    Ok(json!({"collision_status":"impulse_contact","ev_ms":ev,"hla_deg":hla,"vla_deg":vla,
        "outgoing_world_ms":out,"outgoing_spin_rad_s":spin,"spin_rpm":norm(spin)*30.0/PI,
        "bat_omega_after_rad_s":after_omega,"bat_axial_spin_after_rad_s":dot(after_omega,a),
        "surface_speed_ms":norm(surface),"centerline_speed_ms":norm(input.centerline_velocity),
        "barrel_radius_m":p.barrel_radius,"axial_inertia_kg_m2":p.axial_inertia,"normal_axial_coupling_kg_inv":c,
        "ball_translation_before_j":0.5*p.ball_mass*dot(input.vin,input.vin),"ball_translation_after_j":0.5*p.ball_mass*dot(out,out),
        "ball_rotation_before_j":0.5*bi*dot(input.ball_omega,input.ball_omega),"ball_rotation_after_j":0.5*bi*dot(spin,spin),
        "bat_before_j":before_bat,"bat_after_j":after_bat,"total_loss_j":loss,
        "system_before_j":before_bat+before_ball,"system_after_j":after_bat+after_ball,
        "normal_impulse_kg_ms":jn,"tangent_impulse_kg_ms":ja.hypot(jt),
        "normal_impulse_world_kg_ms":mul(n,jn),"tangent_impulse_world_kg_ms":add(mul(a,ja),mul(t,jt)),
        "post_tangent_slip_ms":post_components[1].hypot(post_components[2]),
        "tangent_target_residual_ms":tangent_error,"tangent_regime":regime,"checks":checks}))
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PitchSpinInput {
    pub spin_rate: Option<f64>,
    pub spin_angle: Option<f64>,
    pub spin_efficiency: Option<f64>,
    pub gyro_sign: i8,
}
pub fn reconstruct_spin(p: PitchSpinInput, velocity: V3) -> Result<V3, &'static str> {
    let rate=p.spin_rate.filter(|x|x.is_finite()).ok_or("non_finite_spin_rate")?;
    let angle=p.spin_angle.filter(|x|x.is_finite()).ok_or("non_finite_spin_angle")?;
    let efficiency=p.spin_efficiency.filter(|x|x.is_finite()).ok_or("non_finite_spin_efficiency")?;
    if rate<0.0 { return Err("negative_spin_rate"); }
    if !(0.0..=1.0).contains(&efficiency) { return Err("spin_efficiency_out_of_range"); }
    if !velocity.iter().all(|x|x.is_finite()) || norm(velocity)<=0.0 { return Err("invalid_incoming_velocity"); }
    if ![-1,0,1].contains(&p.gyro_sign) { return Err("invalid_gyro_sign"); }
    let vhat=mul(velocity,1.0/norm(velocity)); let theta=angle.to_radians();
    let lift=[theta.sin(),0.0,theta.cos()]; let projected=sub(lift,mul(vhat,dot(lift,vhat)));
    let omega=rate*PI/30.0;
    if efficiency>0.0 && norm(projected)<1e-12 { return Err("degenerate_pitch_spin_frame"); }
    let transverse=if efficiency>0.0 { mul(cross(vhat,projected),omega*efficiency/norm(projected)) } else { [0.0;3] };
    let gyro=mul(vhat,p.gyro_sign as f64*omega*(1.0-efficiency.powi(2)).max(0.0).sqrt());
    Ok(add(transverse,gyro))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    pub fn same(expected:&Value,actual:&Value,path:&str) {
        match (expected,actual) {
            (Value::Object(a),Value::Object(b))=>{
                assert_eq!(a.len(),b.len(),"{path}: object size");
                for (k,v) in a { assert!(b.contains_key(k),"{path}.{k}: missing");same(v,&b[k],&format!("{path}.{k}")); }
            }
            (Value::Array(a),Value::Array(b))=>{
                assert_eq!(a.len(),b.len(),"{path}: array size");
                for (x,y) in a.iter().zip(b) { same(x,y,path); }
            }
            (Value::Number(a),Value::Number(b))=>assert!((a.as_f64().unwrap()-b.as_f64().unwrap()).abs()<=1e-8,"{path}: {a} != {b}"),
            _=>assert_eq!(expected,actual,"{path}"),
        }
    }
    pub fn fixture() -> Value {
        serde_json::from_str(include_str!("../../../tests/fixtures/finite_collision_contract.json")).unwrap()
    }
    #[test]
    fn finite_response_and_exclusions_match_python() {
        let fixture=fixture();let mut regimes=std::collections::BTreeSet::new();
        for c in fixture["core"].as_array().unwrap() {
            let input:FiniteCollisionInput=serde_json::from_value(c["input"].clone()).unwrap();
            match solve(&input) {
                Ok(value)=>{
                    assert!(c["error"].is_null(),"{}",c["name"]);
                    same(&c["expected"],&value,c["name"].as_str().unwrap());
                    if let Some(regime)=value["tangent_regime"].as_str() { regimes.insert(regime.to_string()); }
                    if let Some(checks)=value["checks"].as_object() {
                        assert!(checks.values().all(|v|v.as_f64().is_some_and(|x|x.is_finite()&&x<=1e-7)));
                    }
                }
                Err(error)=>assert_eq!(c["error"].as_str(),Some(error)),
            }
        }
        assert!(regimes.contains("coulomb_limited"));assert!(regimes.contains("restitution_target"));
    }
    #[test]
    fn spin_and_profile_match_python() {
        let fixture=fixture();
        for c in fixture["spin"].as_array().unwrap() {
            let input:PitchSpinInput=serde_json::from_value(c["input"].clone()).unwrap();
            let velocity:V3=serde_json::from_value(c["velocity"].clone()).unwrap();
            match reconstruct_spin(input,velocity) {
                Ok(value)=>same(&c["expected"],&json!(value),"spin"),
                Err(error)=>assert_eq!(c["error"].as_str(),Some(error)),
            }
        }
        for c in fixture["profile"].as_array().unwrap() { same(&c["expected"],&json!(profile_e(c["z"].as_f64().unwrap())),"profile"); }
    }
    #[test]
    fn nonfinite_inputs_are_explicit_errors() {
        let mut input:FiniteCollisionInput=serde_json::from_value(fixture()["core"][0]["input"].clone()).unwrap();
        input.vin[0]=f64::NAN;
        assert_eq!(solve(&input).unwrap_err(),"invalid_finite_velocity");
        input.vin[0]=8.0;input.parameters.mass=f64::INFINITY;
        assert_eq!(solve(&input).unwrap_err(),"invalid_finite_parameters");
        let spin=PitchSpinInput {spin_rate:Some(f64::NAN),spin_angle:Some(0.0),spin_efficiency:Some(0.9),gyro_sign:0};
        assert_eq!(reconstruct_spin(spin,[0.0,-40.0,0.0]).unwrap_err(),"non_finite_spin_rate");
    }
}
