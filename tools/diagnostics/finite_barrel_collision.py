"""Coupled sphere / axisymmetric finite-radius bat terminal impulse diagnostic.

Side contacts only. A Coulomb cone caps a ray toward the restitution target;
this is not a time-resolved elastic contact law or a maximum-dissipation solver.
Targets with tensile normal impulse or energy gain are explicitly unavailable.
"""
import math

from contact_3d import vector
from contact_collision_probe import BALL_MASS, angles, cross, dot
from contact_spin import BALL_RADIUS_M, energy, parameters
from rotating_bat_probe import add, sub, mul, norm


def inertia_apply(v, axis, transverse, axial):
    return add(mul(v, transverse), mul(axis, (axial-transverse)*dot(v,axis)))


def unavailable(reason, **details):
    return dict(collision_status='not_evaluated:'+reason,ev_ms=None,hla_deg=None,vla_deg=None,**details)


def solve(geometry, vin, centerline_velocity, ball_omega, bat_omega, mass, transverse_inertia,
          axial_inertia, cm_offset, barrel_radius, restitution=.5, mu=.2, et=0.,
          ball_mass=BALL_MASS, ball_radius=BALL_RADIUS_M, inertia_factor=.4):
    geometry.validate(); parameters(restitution,mu,et)
    if geometry.region != 'side':raise ValueError('unsupported_cap')
    if not all(vector(v) for v in (vin,centerline_velocity,ball_omega,bat_omega)):
        raise ValueError('invalid_finite_velocity')
    if (not all(math.isfinite(x) for x in (mass,transverse_inertia,axial_inertia,cm_offset,barrel_radius))
        or min(mass,transverse_inertia,axial_inertia)<=0 or barrel_radius<0):
        raise ValueError('invalid_finite_parameters')
    if (not all(math.isfinite(x) for x in (ball_mass,ball_radius,inertia_factor))
        or min(ball_mass,ball_radius,inertia_factor)<=0):raise ValueError('invalid_sphere_parameters')
    ball_inertia = inertia_factor*ball_mass*ball_radius**2
    if not math.isfinite(ball_inertia) or ball_inertia<=0:raise ValueError('invalid_sphere_inertia')
    a,n = geometry.axis_world,geometry.normal_world
    t = cross(a,n);d = geometry.closest_axial_m-cm_offset
    line_arm = mul(a,d);arm = add(line_arm,mul(n,barrel_radius));rb = mul(n,-ball_radius)
    surface = add(centerline_velocity,cross(bat_omega,mul(n,barrel_radius)))
    center = sub(centerline_velocity,cross(bat_omega,line_arm))
    u = sub(add(vin,cross(ball_omega,rb)),surface)
    un,ua,ut = (dot(u,b) for b in (n,a,t))
    if un>=0:return dict(collision_status='non_approaching',ev_ms=None,hla_deg=None,vla_deg=None)
    # Contact-space inverse effective mass, in basis (n, a, a cross n).
    kn = 1/ball_mass+1/mass+d*d/transverse_inertia
    ka = 1/ball_mass+1/mass+barrel_radius**2/transverse_inertia+ball_radius**2/ball_inertia
    kt = kn+barrel_radius**2/axial_inertia+ball_radius**2/ball_inertia
    c = -d*barrel_radius/transverse_inertia
    sa = ka-c*c/kn
    if not all(math.isfinite(v) for v in (kn,ka,kt,c,sa)) or min(kn,sa,kt)<=0:
        raise ValueError('invalid_contact_mass_matrix')
    j0 = -(1+restitution)*un/kn
    # Eliminate Jn using normal restitution, then solve both tangent targets.
    desired_a = (-(1+et)*ua-c*j0)/sa
    desired_t = -(1+et)*ut/kt
    size = math.hypot(desired_a,desired_t)
    if size==0:
        scale=1.
    else:
        # Jn(lambda)=j0-(c/kn)*lambda*desired_a. Solve cone along this ray.
        denominator = size+mu*c*desired_a/kn
        scale = min(1.,mu*j0/denominator) if denominator>0 else 1.
    ja,jt = scale*desired_a,scale*desired_t
    jn = j0-c*ja/kn
    if not all(math.isfinite(v) for v in (jn,ja,jt,scale)):
        raise ValueError('non_finite_coupled_impulse')
    if jn<=0:return unavailable('tensile_normal_target')
    impulse = add(mul(n,jn),add(mul(a,ja),mul(t,jt)))
    kj = (kn*jn+c*ja,c*jn+ka*ja,kt*jt)
    change = un*jn+ua*ja+ut*jt+.5*(jn*kj[0]+ja*kj[1]+jt*kj[2])
    # Different restitution factors on coupled axes need not be passive.
    # Never hide an energy-producing target by adjusting outgoing EV/spin.
    if change>1e-9:return unavailable('energy_increasing_target',candidate_energy_gain_j=change)
    loss = max(0.,-change)
    out = add(vin,mul(impulse,1/ball_mass))
    spin = add(ball_omega,mul(cross(rb,impulse),1/ball_inertia))
    dv = mul(impulse,-1/mass)
    dw = inertia_apply(mul(cross(arm,impulse),-1),a,1/transverse_inertia,1/axial_inertia)
    after_omega = add(bat_omega,dw)
    def bat_energy(v,w):
        return .5*mass*dot(v,v)+.5*dot(w,inertia_apply(w,a,transverse_inertia,axial_inertia))
    before_bat = bat_energy(center,bat_omega);after_bat = bat_energy(add(center,dv),after_omega)
    before_ball = energy(vin,ball_omega,ball_mass,ball_inertia);after_ball = energy(out,spin,ball_mass,ball_inertia)
    post = sub(add(out,cross(spin,rb)),add(surface,add(dv,cross(dw,arm))))
    post_components = tuple(dot(post,b) for b in (n,a,t))
    delta_spin_L = mul(sub(spin,ball_omega),ball_inertia)
    delta_bat_L = inertia_apply(dw,a,transverse_inertia,axial_inertia)
    total_delta_L = add(cross(sub(arm,rb),mul(sub(out,vin),ball_mass)),add(delta_spin_L,delta_bat_L))
    checks = dict(energy_balance_j=abs(after_bat+after_ball-before_bat-before_ball+loss),
        normal_restitution_ms=abs(post_components[0]+restitution*un),
        matrix_response_ms=norm(sub(post,add(u,add(mul(n,kj[0]),add(mul(a,kj[1]),mul(t,kj[2])))))),
        linear_impulse_kg_ms=norm(add(mul(sub(out,vin),ball_mass),mul(dv,mass))),
        ball_angular_impulse=norm(sub(delta_spin_L,cross(rb,impulse))),
        bat_angular_impulse=norm(add(delta_bat_L,cross(arm,impulse))),
        total_angular_impulse=norm(total_delta_L),
        friction_cone_excess=max(0.,math.hypot(ja,jt)-mu*jn),
        negative_dissipation_j=max(0.,-loss))
    regime = 'coulomb_limited' if scale<1-1e-12 else 'restitution_target'
    tangent_error = math.hypot(post_components[1]+et*ua,post_components[2]+et*ut)
    if regime=='restitution_target':checks['tangent_restitution_ms']=tangent_error
    ev,hla,vla = angles(out)
    if not all(math.isfinite(v) for v in (*out,*spin,*post,loss,before_bat,after_bat,*checks.values())):
        raise ValueError('non_finite_finite_result')
    return dict(collision_status='impulse_contact',ev_ms=ev,hla_deg=hla,vla_deg=vla,
        outgoing_world_ms=out,outgoing_spin_rad_s=spin,spin_rpm=norm(spin)*30/math.pi,
        bat_omega_after_rad_s=after_omega,bat_axial_spin_after_rad_s=dot(after_omega,a),
        surface_speed_ms=norm(surface),centerline_speed_ms=norm(centerline_velocity),
        barrel_radius_m=barrel_radius,axial_inertia_kg_m2=axial_inertia,normal_axial_coupling_kg_inv=c,
        ball_translation_before_j=.5*ball_mass*dot(vin,vin),ball_translation_after_j=.5*ball_mass*dot(out,out),
        ball_rotation_before_j=.5*ball_inertia*dot(ball_omega,ball_omega),ball_rotation_after_j=.5*ball_inertia*dot(spin,spin),
        bat_before_j=before_bat,bat_after_j=after_bat,total_loss_j=loss,
        system_before_j=before_bat+before_ball,system_after_j=after_bat+after_ball,
        normal_impulse_kg_ms=jn,tangent_impulse_kg_ms=math.hypot(ja,jt),
        post_tangent_slip_ms=math.hypot(post_components[1],post_components[2]),
        tangent_target_residual_ms=tangent_error,tangent_regime=regime,checks=checks)
