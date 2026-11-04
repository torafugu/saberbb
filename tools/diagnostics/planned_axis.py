"""Diagnostic intended yaw with fixed forecast and ideal velocity; legacy yaw=0 identity."""
import math

from rotating_bat_probe import add,sub,mul,rotate,cross,dot
from swing_prediction_probe import plan_swing


def plan_axis_swing(prediction,lever,half_length,yaw_batter_deg=0.,aim_offset_world_m=None):
    if not math.isfinite(yaw_batter_deg) or abs(yaw_batter_deg)>45:raise ValueError('invalid_planned_yaw')
    base=plan_swing(prediction,lever,half_length)
    if yaw_batter_deg==0:return with_planned_aim(base,aim_offset_world_m)
    h=prediction['handedness_sign']
    if h not in (-1,1):raise ValueError('invalid_handedness')
    # Mirror the world Z rotation between batting sides: axis=(h*cos(t)*cos(y),cos(t)*sin(y),sin(t)).
    yaw=(0.,0.,h*math.radians(yaw_batter_deg))
    axis=rotate(base['ideal_axis_world'],yaw,1.)
    velocity=base['ideal_velocity_world_ms']
    pivot=sub(base['forecast_position_world_m'],mul(axis,lever))
    vp=mul(axis,dot(velocity,axis))
    omega=mul(cross(axis,sub(velocity,vp)),1/lever)
    # The saved inclination error belongs to the yawed local tilt plane, not a fixed world Y axis.
    error=rotate(base['execution_rotation_world_rad'],yaw,1.)
    actual_axis=rotate(axis,error,1.);actual_omega=rotate(omega,error,1.)
    return with_planned_aim(dict(base,pivot_world_m=pivot,pivot_velocity_world_ms=vp,
        ideal_axis_world=axis,actual_axis_world=actual_axis,
        actual_point_world_m=add(pivot,mul(actual_axis,lever)),
        actual_velocity_world_ms=add(vp,cross(actual_omega,mul(actual_axis,lever))),
        ideal_omega_world_rad_s=omega,actual_omega_world_rad_s=actual_omega,
        execution_rotation_world_rad=error),aim_offset_world_m)


def with_planned_aim(plan,offset):
    """Keep forecast as the predicted ball center; translate the intended bat reference only."""
    if offset is None:return plan
    if (not isinstance(offset,(tuple,list)) or len(offset)!=3
        or any(isinstance(x,bool) or not isinstance(x,(int,float)) or not math.isfinite(x) for x in offset)):
        raise ValueError('invalid_planned_aim_offset')
    point=add(plan['forecast_position_world_m'],offset)
    return dict(plan,planned_aim_offset_world_m=tuple(offset),planned_aim_point_world_m=point,
        ideal_point_world_m=add(plan['ideal_point_world_m'],offset),
        actual_point_world_m=add(plan['actual_point_world_m'],offset),
        pivot_world_m=add(plan['pivot_world_m'],offset))


def with_planned_speed(plan,ratio):
    """Change intended velocities before constructing the pulse's initial pose."""
    if isinstance(ratio,bool) or not isinstance(ratio,(int,float)) or not math.isfinite(ratio) or not 0<ratio<=1:
        raise ValueError('invalid_planned_speed_ratio')
    fields=('ideal_velocity_world_ms','actual_velocity_world_ms','pivot_velocity_world_ms',
            'ideal_omega_world_rad_s','actual_omega_world_rad_s')
    return dict(plan,planned_speed_ratio=ratio,**{k:mul(plan[k],ratio) for k in fields})
