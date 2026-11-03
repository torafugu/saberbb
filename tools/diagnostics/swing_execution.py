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


@dataclass(frozen=True)
class SwingPlan:
    nominal_time_sec: float
    start_time_sec: float
    planned_effort: float
    duration_sec: float
    anchor_fraction: float
    lever_m: float
    half_length_m: float
    forecast_position_world_m: tuple
    reference_point_world_m: tuple
    reference_axis_world: tuple
    start_pivot_world_m: tuple
    start_axis_world: tuple
    peak_pivot_velocity_world_ms: tuple
    peak_omega_world_rad_s: tuple

    @classmethod
    def from_saved(cls,saved,effort=1.,duration=.18,fraction=.5):
        clock=SwingExecution(0,0,saved['nominal_time_sec'],effort,duration,fraction,0.)
        omega=mul(saved['actual_omega_world_rad_s'],effort)
        vp=mul(saved['pivot_velocity_world_ms'],effort)
        progress=clock.integral(0.)
        start_axis=rotate(saved['actual_axis_world'],omega,-progress)
        start_pivot=sub(saved['pivot_world_m'],mul(vp,progress))
        return cls(saved['nominal_time_sec'],clock.start_time_sec,effort,duration,fraction,
            saved['lever_m'],saved['half_length_m'],tuple(saved['forecast_position_world_m']),
            tuple(saved['actual_point_world_m']),tuple(saved['actual_axis_world']),start_pivot,start_axis,vp,omega)

    @property
    def start_point_world_m(self):
        return add(self.start_pivot_world_m,mul(self.start_axis_world,self.lever_m))

    def record(self):
        return dict(asdict(self),version=1,source='forecast_target_and_planned_effort_v1',
            start_point_world_m=self.start_point_world_m)


@dataclass(frozen=True)
class ExecutionDeviation:
    acceleration_ratio: float = 1.
    start_delay_sec: float = 0.

    def __post_init__(self):
        if (not all(math.isfinite(v) for v in (self.acceleration_ratio,self.start_delay_sec))
            or not 0<self.acceleration_ratio<=1):raise ValueError('invalid_execution_deviation')


class PlanExecutionMotion(ExecutionMotion):
    """Execute forward from a fixed plan start pose; never re-anchor the endpoint."""
    def __init__(self,state,plan,deviation,game_id=0,count_seq=0):
        self.swing_plan=plan;self.deviation=deviation
        self.execution=SwingExecution(game_id,count_seq,plan.nominal_time_sec,
            plan.planned_effort*deviation.acceleration_ratio,plan.duration_sec,
            plan.anchor_fraction,deviation.start_delay_sec)
        FrameMotion.__init__(self,state,plan.lever_m,plan.half_length_m,plan.start_pivot_world_m,
            mul(plan.peak_pivot_velocity_world_ms,deviation.acceleration_ratio),plan.start_axis_world,
            mul(plan.peak_omega_world_rad_s,deviation.acceleration_ratio))
        self.velocity=self.velocity_at_phase(0.,0.)
        self.speed_bound=norm(self.ball_velocity)+norm(self.pivot_velocity)+norm(self.omega)*(self.lever+self.half_length)

    def pose(self,t,delay):
        progress=self.execution.integral(self.phase(t,delay))
        axis=rotate(self.axis,self.omega,progress)
        pivot=add(self.pivot,mul(self.pivot_velocity,progress))
        return add(pivot,mul(axis,self.lever)),axis
