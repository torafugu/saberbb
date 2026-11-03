"""Energy-audited centerline rigid-bat / spinning-sphere impulse prototype.

Side contacts only. The bat impulse acts at the saved centerline point (zero
barrel radius); normal recoil therefore exactly matches contact_recoil.solve.
This deliberately excludes barrel torsion and finite-radius cross-coupling.
"""
import math

from contact_3d import vector
from contact_collision_probe import BALL_MASS, angles, cross, dot
from rotating_bat_probe import add, sub, mul, norm

BALL_RADIUS_M = .036
BALL_INERTIA = .4 * BALL_MASS * BALL_RADIUS_M**2


def parameters(e, mu, et):
    if not all(math.isfinite(v) for v in (e, mu, et)) or not (0 <= e <= 1 and mu >= 0 and 0 <= et <= 1):
        raise ValueError('invalid_impulse_parameters')


def tangential_impulse(u, denominators, normal_impulse, mu, et):
    desired = tuple(-(1 + et) * v / k for v, k in zip(u, denominators))
    size = math.hypot(*desired)
    scale = min(1., mu * normal_impulse / size) if size else 1.
    return tuple(scale * j for j in desired), ('coulomb_limited' if scale < 1 else 'restitution_target')


def energy(v, omega, mass=BALL_MASS, inertia=BALL_INERTIA):
    return .5 * mass * dot(v, v) + .5 * inertia * dot(omega, omega)


def solve_side(geometry, vin, vbat, ball_omega, bat_omega, mass, inertia, cm_offset,
               restitution=.5, mu=.2, et=0.):
    geometry.validate()
    parameters(restitution, mu, et)
    if geometry.region != 'side':
        raise ValueError('unsupported_cap')
    if not all(vector(v) for v in (vin, vbat, ball_omega, bat_omega)):
        raise ValueError('invalid_spin_velocity')
    if not all(math.isfinite(x) for x in (mass, inertia, cm_offset)) or mass <= 0 or inertia <= 0:
        raise ValueError('invalid_bat_mass')
    axis, n = geometry.axis_world, geometry.normal_world
    if abs(dot(axis, bat_omega)) > 1e-7:
        raise ValueError('unsupported_bat_axial_spin')
    t = cross(axis, n)
    arm = mul(axis, geometry.closest_axial_m - cm_offset)
    rb = mul(n, -BALL_RADIUS_M)
    u = sub(add(vin, cross(ball_omega, rb)), vbat)
    un = dot(u, n)
    if un >= 0:
        return dict(collision_status='non_approaching', ev_ms=None, hla_deg=None, vla_deg=None)
    kn = 1 / BALL_MASS + 1 / mass + dot(arm, arm) / inertia
    jn = -(1 + restitution) * un / kn
    ut = (dot(u, axis), dot(u, t))
    kt = (1 / BALL_MASS + 1 / mass + BALL_RADIUS_M**2 / BALL_INERTIA,
          kn + BALL_RADIUS_M**2 / BALL_INERTIA)
    jt, regime = tangential_impulse(ut, kt, jn, mu, et)
    impulse = add(mul(n, jn), add(mul(axis, jt[0]), mul(t, jt[1])))
    out = add(vin, mul(impulse, 1 / BALL_MASS))
    spin = add(ball_omega, mul(cross(rb, impulse), 1 / BALL_INERTIA))
    dv = mul(impulse, -1 / mass)
    dw = mul(cross(arm, impulse), -1 / inertia)
    # Centerline torque has no axial component; transverse inertia suffices.
    center = sub(vbat, cross(bat_omega, arm))
    before = energy(vin, ball_omega) + energy(center, bat_omega, mass, inertia)
    after = energy(out, spin) + energy(add(center, dv), add(bat_omega, dw), mass, inertia)
    normal_loss = .5 * (1 - restitution**2) * un**2 / kn
    tangent_loss = -sum(a*j + .5*k*j*j for a, k, j in zip(ut, kt, jt))
    loss = normal_loss + tangent_loss
    u_after = sub(add(out, cross(spin, rb)), add(vbat, add(dv, cross(dw, arm))))
    checks = dict(energy_balance_j=abs(after - before + loss),
                  normal_restitution_ms=abs(dot(u_after, n) + restitution * un),
                  linear_impulse_kg_ms=norm(add(mul(sub(out, vin), BALL_MASS), mul(dv, mass))),
                  ball_angular_impulse=norm(sub(mul(sub(spin, ball_omega), BALL_INERTIA), cross(rb, impulse))),
                  bat_angular_impulse=norm(add(mul(dw, inertia), cross(arm, impulse))),
                  friction_cone_excess=max(0., math.hypot(*jt) - mu * jn),
                  negative_dissipation_j=max(0., -loss),
                  normal_tangent_loss_error_j=abs(loss + dot(u, impulse) + .5*(kn*jn*jn + sum(k*j*j for k,j in zip(kt,jt)))))
    if not all(math.isfinite(x) for x in (*out, *spin, before, after, loss, *checks.values())):
        raise ValueError('non_finite_spin_result')
    ev, hla, vla = angles(out)
    return dict(collision_status='impulse_contact', outgoing_world_ms=out, outgoing_spin_rad_s=spin,
                ev_ms=ev, hla_deg=hla, vla_deg=vla, spin_rpm=norm(spin)*30/math.pi,
                ball_translation_before_j=.5*BALL_MASS*dot(vin,vin),
                ball_translation_after_j=.5*BALL_MASS*dot(out,out),
                ball_rotation_before_j=.5*BALL_INERTIA*dot(ball_omega,ball_omega),
                ball_rotation_after_j=.5*BALL_INERTIA*dot(spin,spin),
                system_before_j=before, system_after_j=after, normal_loss_j=normal_loss,
                tangent_loss_j=tangent_loss, total_loss_j=loss, normal_impulse_kg_ms=jn,
                tangent_impulse_kg_ms=math.hypot(*jt),
                normal_impulse_world_kg_ms=mul(n,jn),
                tangent_impulse_world_kg_ms=add(mul(axis,jt[0]),mul(t,jt[1])),
                tangent_regime=regime, checks=checks)


def ground_impact(velocity, omega, restitution=.45, mu=.25):
    """Spinning sphere against a fixed horizontal ground, no tangential rebound."""
    parameters(restitution, mu, 0.)
    if not vector(velocity) or not vector(omega) or velocity[2] >= 0:
        raise ValueError('invalid_ground_approach')
    rb = (0., 0., -BALL_RADIUS_M)
    u = add(velocity, cross(omega, rb))
    jn = -(1 + restitution) * velocity[2] * BALL_MASS
    k = 1 / BALL_MASS + BALL_RADIUS_M**2 / BALL_INERTIA
    jt, regime = tangential_impulse(u[:2], (k,k), jn, mu, 0.)
    impulse = (jt[0], jt[1], jn)
    out = add(velocity, mul(impulse, 1 / BALL_MASS))
    spin = add(omega, mul(cross(rb, impulse), 1 / BALL_INERTIA))
    loss = -(dot(u, impulse) + .5*((jt[0]**2+jt[1]**2)*k+jn*jn/BALL_MASS))
    residual = energy(out, spin) - energy(velocity, omega) + loss
    return dict(velocity=out, omega=spin, loss_j=loss, energy_balance_j=abs(residual),
                negative_dissipation_j=max(0.,-loss), tangent_regime=regime)


def first_ground(velocity, omega, height_m=.9, restitution=.45, mu=.25, gravity=9.80665):
    """Analytic gravity-only flight to first impact and rebound apex/next impact.

    height_m is center travel to first ground contact (point-flight convention),
    not inferred from the source. No drag, Magnus, fences, fielders or rolling.
    """
    if not vector(velocity) or not vector(omega) or not all(math.isfinite(x) for x in (height_m,gravity)) or height_m <= 0 or gravity <= 0:
        raise ValueError('invalid_ground_flight')
    vx,vy,vz = velocity
    down_speed = math.sqrt(vz*vz+2*gravity*height_m)
    # Avoid cancellation for steep downward launches.
    time = 2*height_m/(down_speed-vz) if vz < 0 else (vz+down_speed)/gravity
    incoming = (vx,vy,-down_speed)
    hit = ground_impact(incoming, omega, restitution, mu)
    out = hit['velocity']; speed = math.hypot(out[0],out[1])
    return dict(first_ground_time_sec=time, first_ground_distance_m=math.hypot(vx,vy)*time,
                pre_ground_horizontal_ms=math.hypot(vx,vy), pre_ground_speed_ms=norm(incoming),
                post_ground_speed_ms=norm(out), post_ground_horizontal_ms=speed,
                post_ground_spin_rpm=norm(hit['omega'])*30/math.pi,
                rebound_height_m=out[2]**2/(2*gravity), second_ground_time_sec=2*out[2]/gravity,
                rebound_travel_m=speed*2*out[2]/gravity, ground_loss_j=hit['loss_j'],
                ground_energy_balance_j=hit['energy_balance_j'], ground_negative_dissipation_j=hit['negative_dissipation_j'],
                flight_energy_balance_j=abs(energy(incoming,omega)-energy(velocity,omega)-BALL_MASS*gravity*height_m))
