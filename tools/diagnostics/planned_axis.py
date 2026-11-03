"""Diagnostic intended yaw with fixed forecast and ideal velocity; legacy yaw=0 identity."""
import math

from rotating_bat_probe import add,sub,mul,rotate,cross,dot
from swing_prediction_probe import plan_swing


def plan_axis_swing(prediction,lever,half_length,yaw_batter_deg=0.):
    if not math.isfinite(yaw_batter_deg) or abs(yaw_batter_deg)>45:raise ValueError('invalid_planned_yaw')
    base=plan_swing(prediction,lever,half_length)
    if yaw_batter_deg==0:return base
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
    return dict(base,pivot_world_m=pivot,pivot_velocity_world_ms=vp,
        ideal_axis_world=axis,actual_axis_world=actual_axis,
        actual_point_world_m=add(pivot,mul(actual_axis,lever)),
        actual_velocity_world_ms=add(vp,cross(actual_omega,mul(actual_axis,lever))),
        ideal_omega_world_rad_s=omega,actual_omega_world_rad_s=actual_omega,
        execution_rotation_world_rad=error)
