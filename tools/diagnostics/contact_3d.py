"""Versioned, outcome-neutral 3D contact contract and descriptive quality metrics."""
import math
from collections import Counter, defaultdict
from dataclasses import dataclass

from contact_collision_probe import BALL_MASS, angles, collide, dot, write_csv

CONTRACT_VERSION = 1


def vector(value):
    return len(value)==3 and all(isinstance(x,(int,float)) and math.isfinite(x) for x in value)


@dataclass(frozen=True)
class ContactGeometry:
    time_since_release_sec: float
    axis_world: tuple
    normal_world: tuple
    axial_center_m: float
    closest_axial_m: float
    half_length_m: float
    region: str

    def validate(self):
        if not vector(self.axis_world) or not vector(self.normal_world):
            raise ValueError('invalid_contact_vector')
        scalars=(self.time_since_release_sec,self.axial_center_m,self.closest_axial_m,self.half_length_m)
        if not all(math.isfinite(x) for x in scalars) or self.time_since_release_sec<0 or self.half_length_m<=0:
            raise ValueError('invalid_contact_scalar')
        if abs(dot(self.axis_world,self.axis_world)-1)>1e-8 or abs(dot(self.normal_world,self.normal_world)-1)>1e-8:
            raise ValueError('non_unit_contact_frame')
        expected=max(-self.half_length_m,min(self.half_length_m,self.axial_center_m))
        region='side' if abs(self.axial_center_m)<=self.half_length_m else 'cap'
        if abs(self.closest_axial_m-expected)>1e-8 or self.region!=region:
            raise ValueError('inconsistent_contact_region')
        normal_axis=dot(self.normal_world,self.axis_world)
        if self.region=='side' and abs(normal_axis)>1e-8:
            raise ValueError('side_normal_not_perpendicular')
        if self.region=='cap' and normal_axis*(self.axial_center_m-self.closest_axial_m)<-1e-8:
            raise ValueError('cap_normal_wrong_side')


def solve_contact(geometry,ball_velocity,bat_point_velocity,restitution=.5,effective_mass=.6):
    """One event -> one outgoing vector -> EV/HLA/VLA. No legacy fields or RNG."""
    geometry.validate()
    if not vector(ball_velocity) or not vector(bat_point_velocity):
        raise ValueError('invalid_contact_velocity')
    if not all(math.isfinite(x) for x in (restitution,effective_mass)) or not 0<=restitution<=1 or effective_mass<=0:
        raise ValueError('invalid_collision_parameters')
    n=geometry.normal_world
    relative=tuple(i-b for i,b in zip(ball_velocity,bat_point_velocity))
    speed=math.sqrt(dot(relative,relative))
    outgoing,normal,delta=collide(ball_velocity,bat_point_velocity,n,restitution,effective_mass)
    approaching=normal<0 and delta>0
    ev,hla,vla=angles(outgoing) if approaching else (None,None,None)
    fraction=max(0,min(1,-normal/speed)) if speed>0 else None
    # For the finite effective-mass impulse, bat velocity changes by -(m/M)*delta*n.
    restitution_residual=abs(normal+delta*(1+BALL_MASS/effective_mass)+restitution*normal) if approaching else None
    tangent_residual=max(abs((o-i)-delta*a) for o,i,a in zip(outgoing,ball_velocity,n))
    return dict(contract_version=CONTRACT_VERSION,
        collision_status='impulse_contact' if approaching else 'non_approaching',
        outgoing_world_ms=outgoing if approaching else None,
        relative_normal_ms=normal,impulse_delta_ms=delta,normal_impulse_kg_ms=BALL_MASS*delta,
        ev_ms=ev,hla_deg=hla,vla_deg=vla,
        relative_speed_ms=speed,approach_speed_ms=max(0,-normal),normal_speed_fraction=fraction,
        axial_fraction=abs(geometry.closest_axial_m)/geometry.half_length_m,
        normal_elevation_deg=math.degrees(math.asin(max(-1,min(1,n[2])))),
        bat_point_speed_ms=math.sqrt(dot(bat_point_velocity,bat_point_velocity)),
        restitution_residual_ms=restitution_residual,tangential_impulse_residual_ms=tangent_residual)


def band(value,edges):
    if value is None:return 'undefined'
    lower=None
    for upper in edges:
        if value<upper:return f'<{upper:g}' if lower is None else f'{lower:g}..<{upper:g}'
        lower=upper
    return f'{edges[-1]:g}+'


def quality_bands(metrics,region):
    axial=('cap' if region=='cap' else
           ('.75..1' if metrics['axial_fraction']>=.75 else band(metrics['axial_fraction'],(.25,.5,.75))))
    return dict(axial_fraction=axial,
        normal_speed_fraction=band(metrics['normal_speed_fraction'],(.5,.8,.95)),
        normal_elevation_deg=band(metrics['normal_elevation_deg'],(-30,-10,10,30)),
        bat_point_speed_ms=band(metrics['bat_point_speed_ms'],(25,30,35,40)))


class QualityAccumulator:
    """Independent one-dimensional strata; bins are descriptors, not ContactTypes."""
    def __init__(self):
        self.groups=defaultdict(Counter)
        self.statuses=Counter()

    def add(self,record,metrics):
        variant=record['variant'];kind=record['adjusted_contact_type'];legacy=record['legacy_group']
        status=metrics['collision_status'] if metrics else 'not_evaluated:'+record['status']
        self.statuses[variant,kind,legacy,status]+=1
        if not metrics or status!='impulse_contact':return
        for dimension,label in quality_bands(metrics,record['contact_region']).items():
            for contact_type,group in ((kind,legacy),('all','all')):
                g=self.groups[variant,contact_type,group,dimension,label]
                g['contacts']+=1
                for field in ('ev_ms','approach_speed_ms','normal_speed_fraction','bat_point_speed_ms','axial_fraction'):
                    if metrics[field] is not None:g[field]+=metrics[field]
                g['normal_fraction_n']+=metrics['normal_speed_fraction'] is not None
                g['ev_ge_40']+=metrics['ev_ms']>=40
                if metrics['hla_deg'] is not None:
                    g['hla_n']+=1;g['foul']+=abs(metrics['hla_deg'])>=45
                if metrics['vla_deg'] is not None:
                    g['vla_n']+=1;g['vla']+=metrics['vla_deg'];g['abs_vla']+=abs(metrics['vla_deg'])
                    g['negative_vla']+=metrics['vla_deg']<0

    def write(self,output):
        fields=['variant','adjusted_contact_type','legacy_group','dimension','band']
        result=[]
        for key,g in sorted(self.groups.items()):
            n=g['contacts']
            result.append(dict(zip(fields,key),contacts=n,avg_ev_ms=g['ev_ms']/n,
                avg_approach_speed_ms=g['approach_speed_ms']/n,
                avg_normal_speed_fraction=g['normal_speed_fraction']/g['normal_fraction_n'] if g['normal_fraction_n'] else None,
                avg_bat_point_speed_ms=g['bat_point_speed_ms']/n,avg_axial_fraction=g['axial_fraction']/n,
                defined_hla_contacts=g['hla_n'],defined_vla_contacts=g['vla_n'],
                avg_vla_deg=g['vla']/g['vla_n'] if g['vla_n'] else None,
                avg_abs_vla_deg=g['abs_vla']/g['vla_n'] if g['vla_n'] else None,
                ev_ge_40_pct=100*g['ev_ge_40']/n,
                foul_direction_pct=100*g['foul']/g['hla_n'] if g['hla_n'] else None,
                negative_vla_pct=100*g['negative_vla']/g['vla_n'] if g['vla_n'] else None))
        write_csv(output/'contact_3d_quality.csv',fields+['contacts','avg_ev_ms','avg_approach_speed_ms',
            'avg_normal_speed_fraction','avg_bat_point_speed_ms','avg_axial_fraction','defined_hla_contacts',
            'defined_vla_contacts','avg_vla_deg','avg_abs_vla_deg','ev_ge_40_pct','foul_direction_pct','negative_vla_pct'],result)
        fields=['variant','adjusted_contact_type','legacy_group','collision_status']
        write_csv(output/'contact_3d_status.csv',fields+['states'],
            [dict(zip(fields,key),states=n) for key,n in sorted(self.statuses.items())])
