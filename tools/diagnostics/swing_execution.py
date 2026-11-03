"""Diagnostic per-pitch execution state and an analytically integrated speed pulse."""
import math
from dataclasses import asdict, dataclass

from execution_frame_probe import FrameMotion
from rotating_bat_probe import add, sub, mul, cross, norm, rotate


@dataclass(frozen=True)
class SwingExecution:
    game_id: int
    count_seq: int
    reference_time_sec: float
    effort: float = 1.
    duration_sec: float = .18
    anchor_fraction: float = .5
    start_offset_sec: float = 0.

    def __post_init__(self):
        if (not all(math.isfinite(v) for v in (self.reference_time_sec,self.effort,self.duration_sec,
                self.anchor_fraction,self.start_offset_sec)) or self.reference_time_sec<=0
            or not 0<self.effort<=1 or self.duration_sec<=0 or not 0<self.anchor_fraction<1
            or min(self.game_id,self.count_seq)<0):
            raise ValueError('invalid_swing_execution')
        if self.start_time_sec<0:raise ValueError('execution_starts_before_release')

    @property
    def start_time_sec(self):
        return self.reference_time_sec-self.anchor_fraction*self.duration_sec+self.start_offset_sec

    def record(self):
        return dict(asdict(self),version=1,source='explicit_execution_sensitivity_v1',
            start_time_sec=self.start_time_sec,
            peak_time_sec=self.start_time_sec+self.duration_sec/2,
            note='State constructed from saved plan and explicit scenario; not inferred from actual outcome or calibrated effort distribution.')

    def gain(self,q):
        u=(q+self.anchor_fraction*self.duration_sec)/self.duration_sec
        return math.sin(math.pi*u)**2 if 0<u<1 else 0.

    def integral(self,q):
        """Integral of gain from pulse start to q, clamped outside the pulse."""
        u=max(0.,min(1.,(q+self.anchor_fraction*self.duration_sec)/self.duration_sec))
        return self.duration_sec*(u/2-math.sin(2*math.pi*u)/(4*math.pi))


class ExecutionMotion(FrameMotion):
    def __init__(self,state,plan,execution):
        if abs(plan['nominal_time_sec']-execution.reference_time_sec)>1e-10:
            raise ValueError('execution_reference_clock_mismatch')
        self.execution=execution
        super().__init__(state,plan['lever_m'],plan['half_length_m'],plan['pivot_world_m'],
            mul(plan['pivot_velocity_world_ms'],execution.effort),plan['actual_axis_world'],
            mul(plan['actual_omega_world_rad_s'],execution.effort))
        # omega stores peak angular velocity, never the instantaneous value sent to collision response.
        self.velocity=self.velocity_at_phase(0.,0.)
        self.speed_bound=norm(self.ball_velocity)+norm(self.pivot_velocity)+norm(self.omega)*(self.lever+self.half_length)

    def phase(self,t,delay):
        return t-delay-self.execution.start_offset_sec

    def pose(self,t,delay):
        q=self.phase(t,delay)
        integrated=self.execution.integral(q)-self.execution.integral(0.)
        axis=rotate(self.axis,self.omega,integrated)
        point=add(add(self.pivot,mul(self.pivot_velocity,integrated)),mul(axis,self.lever))
        return point,axis

    def angular_velocity(self,t,delay):
        return mul(self.omega,self.execution.gain(self.phase(t,delay)))

    def velocity_at_phase(self,t,delay,axial=0.):
        _,axis=self.pose(t,delay)
        gain=self.execution.gain(self.phase(t,delay))
        return add(mul(self.pivot_velocity,gain),cross(self.angular_velocity(t,delay),mul(axis,self.lever+axial)))

    def evaluate(self,t,delay):
        r=super().evaluate(t,delay)
        r['bat_velocity']=self.velocity_at_phase(t,delay,r['closest_axial_m'])
        return r
