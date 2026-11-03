"""Unsigned bat-axis elevation and contact-normal decomposition, no pose edits."""
import math
from collections import Counter, defaultdict

from contact_3d import band
from contact_collision_probe import dot, write_csv
from contact_selection_audit import region_flags
from effective_mass_probe import direction
from forecast_contact_geometry import vec
from rotating_bat_probe import cross

KEYS=['scope','variant','contact_region','stratum','tilt_band','b_over_r_band','normal_z_band']
GEOMETRY=['tilt_deg','abs_axis_y','normal_x','abs_normal_x','normal_y','normal_z',
          'forward_component','circumferential_component_abs','forward_normal_x','circumferential_normal_x',
          'abs_forward_normal_x','abs_circumferential_normal_x','source_z_m','bat_point_speed_ms']
OUTCOMES=['ev_ms','vla_deg','abs_outgoing_x','outgoing_y','forward_margin_ms']


def geometry(r):
    a,n=vec(r,'axis_at_entry_'),vec(r,'normal_')
    norm_error=max(abs(dot(a,a)-1),abs(dot(n,n)-1))
    if norm_error>1e-8:raise ValueError('tilt_nonunit_geometry')
    tilt=math.degrees(math.atan2(abs(a[2]),math.hypot(a[0],a[1])))
    values=dict(tilt_deg=tilt,abs_axis_y=abs(a[1]),normal_x=n[0],abs_normal_x=abs(n[0]),
        normal_y=n[1],normal_z=n[2],source_z_m=r.get('_source_z_m'),bat_point_speed_ms=r.get('bat_point_speed_ms'))
    checks={'unit_vector_error':norm_error};status='not_side'
    if r['contact_region']=='side':
        orthogonality=abs(dot(a,n));checks['side_axis_normal_dot']=orthogonality
        if orthogonality>1e-8:raise ValueError('tilt_nonperpendicular_side_normal')
        # Project world +Y into the plane perpendicular to the bat axis.
        forward=tuple((1. if i==1 else 0.)-a[1]*a[i] for i in range(3))
        size=math.sqrt(dot(forward,forward))
        if size<=1e-8:status='forward_projection_undefined'
        else:
            ef=tuple(x/size for x in forward);ec=cross(a,ef)
            cf,cc=dot(n,ef),dot(n,ec)
            residual=max(abs(n[i]-cf*ef[i]-cc*ec[i]) for i in range(3))
            checks['normal_basis_closure']=residual
            if residual>1e-8:raise ValueError('tilt_normal_basis_closure')
            values.update(forward_component=cf,circumferential_component_abs=abs(cc),
                forward_normal_x=cf*ef[0],circumferential_normal_x=cc*ec[0],
                abs_forward_normal_x=abs(cf*ef[0]),abs_circumferential_normal_x=abs(cc*ec[0]))
            status='defined'
    if any(v is not None and not math.isfinite(v) for v in values.values()):
        raise ValueError('nonfinite_tilt_geometry')
    return values,status,checks


def strata(tilt,b,nz):
    yield 'overall','all','all','all'
    t=band(tilt,(10,20,30,40,50,60,75,90))
    yield 'tilt',t,'all','all'
    yield 'tilt_impact',t,band(b,(.25,.5,.75,.95)),'all'
    yield 'tilt_impact_normal_z',t,band(b,(.25,.5,.75,.95)),band(nz,(-.75,-.25,0,.25,.75))


def add_values(g,values):
    for k,v in values.items():
        if v is not None:
            if not math.isfinite(v):raise ValueError('nonfinite_tilt_metric')
            g['n_'+k]+=1;g['sum_'+k]+=v


class TiltAudit:
    def __init__(self):
        self.geom=defaultdict(Counter);self.outcomes=defaultdict(Counter);self.coverage=Counter();self.checks=defaultdict(float)

    def add(self,r,results):
        if r['status']!='contact':return
        values,status,checks=geometry(r)
        for k,v in checks.items():self.checks[k]=max(self.checks[k],v)
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        for scope in scopes:
            self.coverage[scope,r['variant'],r['contact_region'],status]+=1
            for stage,t,b,z in strata(values['tilt_deg'],r.get('local_abs_b_over_r'),values['normal_z']):
                key=(scope,r['variant'],r['contact_region'],stage,t,b,z)
                g=self.geom[key+(status,)];g['contacts']+=1;add_values(g,values)
                for basis,m,source in [('source_constant_mass_normal',r,True)]+[(k,m,False) for k,m in results.items()]:
                    out=self.outcomes[key+(basis,m['collision_status'],direction(m))];out['contacts']+=1
                    if m['collision_status']!='impulse_contact':continue
                    velocity=vec(r,'outgoing_velocity_') if source else m['outgoing_world_ms']
                    add_values(out,dict(ev_ms=m['ev_ms'],vla_deg=m['vla_deg'],abs_outgoing_x=abs(velocity[0]),
                        outgoing_y=velocity[1],forward_margin_ms=velocity[1]-abs(velocity[0])))
                    for label,flag in region_flags(m).items():
                        if flag is not None:out['n_'+label]+=1;out[label]+=flag

    def write(self,output):
        def records(groups,keys,metrics,regions=False):
            rows=[]
            for key,g in sorted(groups.items()):
                row=dict(zip(keys,key),contacts=g['contacts'])
                for metric in metrics:
                    n=g['n_'+metric];row['n_'+metric]=n;row['avg_'+metric]=g['sum_'+metric]/n if n else None
                if regions:
                    for label in region_flags({}):
                        n=g['n_'+label];row['n_'+label]=n;row[label+'_contacts']=g[label];row[label+'_fraction']=g[label]/n if n else None
                rows.append(row)
            columns=keys+['contacts']+[p+k for k in metrics for p in ('n_','avg_')]
            if regions:columns += [v for k in region_flags({}) for v in ('n_'+k,k+'_contacts',k+'_fraction')]
            return columns,rows
        cols,rows=records(self.geom,KEYS+['normal_basis_status'],GEOMETRY)
        write_csv(output/'finite_barrel_tilt_geometry.csv',cols,rows)
        cols,rows=records(self.outcomes,KEYS+['outcome_basis','collision_status','direction'],OUTCOMES,True)
        write_csv(output/'finite_barrel_tilt_outcomes.csv',cols,rows)
        keys=['scope','variant','contact_region','normal_basis_status']
        write_csv(output/'finite_barrel_tilt_coverage.csv',keys+['geometry_contacts'],
            [dict(zip(keys,k),geometry_contacts=n) for k,n in sorted(self.coverage.items())])
        write_csv(output/'finite_barrel_tilt_checks.csv',['metric','max_error'],
            [dict(metric=k,max_error=v) for k,v in sorted(self.checks.items())])
