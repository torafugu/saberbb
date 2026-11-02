"""Analytic normal-restitution feasibility on frozen contact geometry (no e grid)."""
import math
from dataclasses import dataclass

from contact_3d import vector
from contact_collision_probe import BALL_MASS, dot
from rotating_bat_probe import add, sub, mul, norm


@dataclass(frozen=True)
class Interval:
    lo: float
    hi: float
    lo_open: bool = False
    hi_open: bool = False

    def nonempty(self):
        return self.lo<self.hi or (self.lo==self.hi and not (self.lo_open or self.hi_open))

    def contains(self, x):
        return (x>self.lo or (x==self.lo and not self.lo_open)) and (x<self.hi or (x==self.hi and not self.hi_open))

    def intersect(self, other):
        lo=max(self.lo,other.lo);hi=min(self.hi,other.hi)
        r=Interval(lo,hi,(self.lo_open and lo==self.lo) or (other.lo_open and lo==other.lo),
                   (self.hi_open and hi==self.hi) or (other.hi_open and hi==other.hi))
        return r if r.nonempty() else None


def positive(interval, a, b):
    """Intersect with a+b*e > 0. Equality is a foul boundary, not fair."""
    if interval is None:return None
    if b==0:return interval if a>0 else None
    root=-a/b
    return interval.intersect(Interval(root,math.inf,True,True) if b>0 else Interval(-math.inf,root,True,True))


def fields(prefix, interval):
    return {prefix+'_'+k:(getattr(interval,k) if interval else None) for k in ('lo','hi','lo_open','hi_open')}


def analyze(vin,vbat,normal,inverse_mass,threshold=25.):
    if (not all(vector(v) for v in (vin,vbat,normal)) or abs(dot(normal,normal)-1)>1e-8
        or not math.isfinite(inverse_mass) or inverse_mass<0 or not math.isfinite(threshold) or threshold<=0):
        raise ValueError('invalid_restitution_bounds_input')
    un=dot(sub(vin,vbat),normal)
    if un>=0:raise ValueError('bounds_require_approaching_contact')
    k=-un/(1+BALL_MASS*inverse_mass)
    # v(e)=A+B*e: the outgoing vector is affine, speed squared is convex quadratic.
    b=mul(normal,k);a=add(vin,b);bb=dot(b,b)
    if bb<=0 or not math.isfinite(bb):raise ValueError('degenerate_restitution_response')
    at=lambda e:add(a,mul(b,e))
    def direction(e):
        x,y,_=at(e)
        return 'fair_direction' if y>abs(x) else 'undefined_direction' if x==y==0 else 'foul_direction'
    e_star=-dot(a,b)/bb
    e_min=max(0.,min(.5,e_star));min_any=norm(at(e_min))
    fair=positive(positive(Interval(0.,.5),a[1]-a[0],b[1]-b[0]),a[1]+a[0],b[1]+b[0])
    # Solve |A+B*e|<threshold via the squared norm at its unbounded minimum.
    radius_squared=(threshold**2-dot(at(e_star),at(e_star)))/bb
    weak=None
    if radius_squared>0:
        d=math.sqrt(radius_squared)
        weak=Interval(e_star-d,e_star+d,True,True).intersect(Interval(0.,.5))
        weak=fair.intersect(weak) if fair is not None and weak is not None else None
    tangent=sub(vin,mul(normal,dot(vin,normal)/dot(normal,normal)))
    floor=norm(tangent)
    if weak is not None:diagnosis='weak_fair_possible'
    elif fair is None:diagnosis='no_fair_in_range'
    elif floor>=threshold:diagnosis='tangent_floor_ge_threshold'
    elif min_any>=threshold:diagnosis='normal_response_floor_ge_threshold'
    else:diagnosis='fair_constraint_blocks_weak'
    limit_e=max(fair.lo,min(fair.hi,e_star)) if fair else None
    result=dict(diagnosis=diagnosis,weak_fair_possible=int(weak is not None),
        baseline_fair=int(fair.contains(.5)) if fair else 0,
        baseline_weak_fair=int(weak.contains(.5)) if weak else 0,
        possible_only_after_reduction=int(weak is not None and not weak.contains(.5)),
        e0_ev_ms=norm(at(0)),e0_fair=int(fair.contains(0)) if fair else 0,e0_direction=direction(0),
        baseline_ev_ms=norm(at(.5)),baseline_direction=direction(.5),tangent_floor_ms=floor,
        min_any_ev_ms=min_any,min_any_e=e_min,
        fair_ev_infimum_ms=norm(at(limit_e)) if fair else None,
        fair_infimum_e=limit_e,fair_min_attained=int(fair.contains(limit_e)) if fair else None,
        fair_exists=int(fair is not None),**fields('fair_e',fair),**fields('weak_e',weak))
    return result
