"""Independent SciPy DOP853 oracle for the diagnostic model; not measured ball records."""
import json
import math
from pathlib import Path
import numpy as np
import scipy
from scipy.integrate import solve_ivp

MASS = 5.125 * 0.028349523125
RADIUS = 9.125 * 0.0254 / (2 * math.pi)
G = 9.81

def flight(l, settings, model):
    a, h = np.radians([l['vla_deg'], l['hla_deg']])
    v0 = l['speed_ms'] * np.array([math.cos(a)*math.sin(h), math.cos(a)*math.cos(h), math.sin(a)])
    u0 = v0 / np.linalg.norm(v0)
    up0 = np.array([0., 0., 1.]) - u0*u0[2]
    up0 /= np.linalg.norm(up0)
    spin_angle = math.radians(l['spin_angle_deg'])
    lift0 = up0*math.cos(spin_angle) + np.cross(u0, up0)*math.sin(spin_angle)
    omega0 = np.cross(u0, lift0)*l['spin_rpm']*math.pi/30
    w = math.radians(l['wind_direction_deg'])
    wind = l['wind_speed_ms'] * np.array([math.sin(w), math.cos(w), 0.])
    k = .5*settings['density_kg_m3']*math.pi*RADIUS**2/MASS
    def rhs(t, state):
        v = state[3:]; air = v-wind; speed = np.linalg.norm(air)
        if model == 'vacuum': return np.r_[v, [0., 0., -G]]
        decay = math.exp(-t/settings['spin_decay_tau_sec']) if settings['spin_decay_tau_sec'] else 1.
        omega = omega0*decay; rpm = l['spin_rpm']*decay
        spin_factor = RADIUS*rpm*math.pi/30/speed
        cd = .297 + .0292*rpm/1000
        cl = 1.120*spin_factor/(.583+2.333*spin_factor)
        drag = .0012 if model in ('live', 'live_drag_published_lift') else k*cd
        u = air/speed
        if model in ('live', 'published_drag_live_lift'):
            up = np.array([0., 0., 1.])-u*u[2]; up /= np.linalg.norm(up)
            lift = (up*math.cos(spin_angle)+np.cross(u,up)*math.sin(spin_angle))*.0000336*rpm*speed if rpm>10 and speed>.1 else np.zeros(3)
        else:
            lift = np.cross(omega,u)/np.linalg.norm(omega)*k*cl*speed**2 if np.linalg.norm(omega)>1e-12 else np.zeros(3)
        return np.r_[v, -drag*speed*air+lift-np.array([0.,0.,G])]
    def ground(t, s): return s[2]
    ground.terminal = True; ground.direction = -1
    def apex(t, s): return s[5]
    apex.direction = -1
    def marker(t, s): return s[1]-settings['marker_y_m']
    marker.direction = 1
    result = solve_ivp(rhs, [0, settings['max_time_sec']], np.r_[[0.,0.,l['height_m']],v0], method='DOP853', rtol=1e-12, atol=1e-12, max_step=.02, events=[ground,apex,marker])
    assert result.success and len(result.t_events[0]) == 1
    pos = result.y_events[0][0][:3]
    return dict(carry_m=float(np.linalg.norm(pos[:2])), landing_position_m=[float(pos[0]),float(pos[1]),0.], landing_time_sec=float(result.t_events[0][0]), max_height_m=max([l['height_m']]+[float(s[2]) for s in result.y_events[1]]), marker_crossing_height_m=float(result.y_events[2][0][2]) if len(result.t_events[2]) else None)

if __name__ == '__main__':
    rows=[]
    specs=[(40.2336,20,2600,'published2017',None,0,0),(45,20,0,'published2017',None,0,0),(45,30,4000,'published2017',None,0,0),(45,20,2600,'live',None,0,0),(45,20,2600,'published_drag_live_lift',None,0,0),(45,20,2600,'live_drag_published_lift',None,0,0),(45,30,2600,'published2017',30.,0,0),(45,20,2600,'published2017',None,8.,90.),(45,30,0,'vacuum',None,0,0)]
    for speed,vla,rpm,model,tau,wind,direction in specs:
        launch=dict(speed_ms=speed,vla_deg=vla,hla_deg=0.,spin_rpm=rpm,spin_angle_deg=0.,height_m=.9,wind_speed_ms=wind,wind_direction_deg=direction)
        settings=dict(dt_sec=.001,max_time_sec=30.,density_kg_m3=1.194,spin_decay_tau_sec=tau,marker_y_m=120.)
        rows.append(dict(launch=launch,settings=settings,model=model,expected=flight(launch,settings,model)))
    target=Path(__file__).resolve().parents[2]/'tests/fixtures/flight_reference_contract.json'
    target.write_text(json.dumps(dict(source='independent_scipy_DOP853_integration_not_individual_measurements',scipy_version=scipy.__version__,rtol=1e-12,atol=1e-12,paper='https://baseball.physics.illinois.edu/TrajectoryAnalysis.pdf',equations='7,10,11',gravity_ms2=G,mass_kg=MASS,radius_m=RADIUS,cases=rows),indent=2)+'\n')
