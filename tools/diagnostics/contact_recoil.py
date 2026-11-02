"""Normal impulse with explicit rigid recoil assumptions (diagnostic only)."""
import math
from contact_3d import vector
from contact_collision_probe import BALL_MASS, angles, cross, dot
from rotating_bat_probe import add, sub, mul, norm


def inverse_mass(axis,normal,s,lever,model,body_mass=.6,gyration=.25,cm_offset=0.):
    if not all(vector(v) for v in (axis,normal)) or not all(math.isfinite(x) for x in (s,lever,body_mass,gyration,cm_offset)):
        raise ValueError('invalid_recoil_input')
    if body_mass<=0 or gyration<=0 or lever<=0:raise ValueError('invalid_recoil_parameters')
    if abs(dot(axis,axis)-1)>1e-8 or abs(dot(normal,normal)-1)>1e-8:raise ValueError('non_unit_recoil_frame')
    if model=='constant':return 1/.6,None,None
    distance=s-cm_offset if model=='free' else lever+s
    if model not in ('free','pivot'):raise ValueError('unknown_recoil_model')
    arm=mul(axis,distance);torque_arm=norm(cross(arm,normal))
    inertia_cm=body_mass*gyration*gyration
    inertia=inertia_cm if model=='free' else inertia_cm+body_mass*(lever+cm_offset)**2
    if not math.isfinite(inertia) or inertia<=0:raise ValueError('invalid_derived_inertia')
    inv=(1/body_mass if model=='free' else 0.)+torque_arm*torque_arm/inertia
    if not math.isfinite(inv):raise ValueError('non_finite_inverse_mass')
    return inv,arm,inertia


def solve(geometry,vin,vbat,inv_mass,restitution=.5):
    geometry.validate()
    if not vector(vin) or not vector(vbat) or not math.isfinite(inv_mass) or inv_mass<0 or not math.isfinite(restitution) or not 0<=restitution<=1:
        raise ValueError('invalid_inverse_mass_collision')
    n=geometry.normal_world;un=dot(sub(vin,vbat),n)
    delta=max(0.,-(1+restitution)*un/(1+BALL_MASS*inv_mass))
    if not math.isfinite(un) or not math.isfinite(delta):raise ValueError('non_finite_impulse')
    impulse=BALL_MASS*delta
    out=add(vin,mul(n,delta));ev,hla,vla=angles(out) if un<0 and delta>0 else (None,None,None)
    if not vector(out) or (ev is not None and not math.isfinite(ev)):raise ValueError('non_finite_outgoing_velocity')
    return dict(collision_status='impulse_contact' if ev is not None else 'non_approaching',
        ev_ms=ev,hla_deg=hla,vla_deg=vla,relative_normal_ms=un,impulse_delta_ms=delta,
        normal_impulse_kg_ms=impulse,outgoing_world_ms=out if ev is not None else None,
        inverse_effective_mass_kg_inv=inv_mass,effective_mass_kg=1/inv_mass if inv_mass>0 else None,
        infinite_effective_mass=int(inv_mass==0),incoming_z_ms=vin[2] if ev is not None else None,
        impulse_z_ms=delta*n[2] if ev is not None else None,outgoing_z_ms=out[2] if ev is not None else None)


def recoil_checks(metrics,vin,vbat,omega,normal,arm,inertia,model,body_mass=.6,restitution=.5):
    """Verify rigid-body impulse response and energy in the free/pivot translating frame."""
    if metrics['collision_status']!='impulse_contact':return {}
    j=metrics['normal_impulse_kg_ms'];out=metrics['outgoing_world_ms']
    if model=='constant':
        dv=mul(normal,-j/.6)
        result=dict(normal_restitution_ms=abs(dot(sub(out,add(vbat,dv)),normal)+restitution*metrics['relative_normal_ms']))
        return result
    torque=cross(arm,normal);dw=mul(torque,-j/inertia)
    dvcm=mul(normal,-j/body_mass) if model=='free' else (0.,0.,0.)
    dvpoint=add(dvcm,cross(dw,arm))
    # Center of mass for free body; prescribed moving pivot otherwise.
    center_velocity=sub(vbat,cross(omega,arm))
    ball_before=sub(vin,center_velocity)
    ball_after=sub(out,center_velocity)
    # Work in a frame translating with the pre-impact center/pivot velocity.
    kinetic_change=.5*BALL_MASS*(dot(ball_after,ball_after)-dot(ball_before,ball_before))
    kinetic_change+=.5*inertia*(dot(add(omega,dw),add(omega,dw))-dot(omega,omega))
    if model=='free':kinetic_change+=.5*body_mass*dot(dvcm,dvcm)
    expected_loss=.5*j*(1-restitution)*(-metrics['relative_normal_ms'])
    result=dict(normal_restitution_ms=abs(dot(sub(out,add(vbat,dvpoint)),normal)+restitution*metrics['relative_normal_ms']),
        recoil_normal_response_ms=abs(dot(dvpoint,normal)+j*metrics['inverse_effective_mass_kg_inv']),
        angular_impulse_kg_m2_s=norm(add(mul(dw,inertia),mul(torque,j))),
        energy_balance_j=abs(kinetic_change+expected_loss))
    if model=='free':result['linear_impulse_kg_ms']=norm(add(mul(sub(out,vin),BALL_MASS),mul(dvcm,body_mass)))
    return result
