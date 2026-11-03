"""World-vector closure and horizontal direction classification on frozen contacts."""
import math
from collections import Counter, defaultdict

from contact_3d import band
from contact_collision_probe import BALL_MASS, angles, dot, write_csv
from effective_mass_probe import direction
from forecast_contact_geometry import vec

KEYS=['scope','variant','outcome_basis','stratum','b_over_r_band','normal_z_band',
      'normal_only_direction','final_direction','horizontal_regime']
VECTORS=['incoming','normal_delta','tangent_delta','normal_only','outgoing','normal','axis','bat_point_velocity']
FIELDS=[prefix+'_'+a for prefix in VECTORS for a in 'xyz']+[
    'ev_ms','vla_deg','outgoing_forward_margin_ms','normal_only_forward_margin_ms','horizontal_speed_ms']


def decompose(r,m,source=False):
    vin=vec(r,'ball_velocity_');n=vec(r,'normal_');axis=vec(r,'axis_at_entry_')
    if source:
        out=vec(r,'outgoing_velocity_');normal=tuple(r['impulse_delta_ms']*x for x in n);tangent=(0.,0.,0.)
    else:
        out=m['outgoing_world_ms']
        jn=m.get('normal_impulse_world_kg_ms')
        jt=m.get('tangent_impulse_world_kg_ms')
        if jn is None:
            # The profile normal-only model has a scalar normal impulse.
            jn=tuple(m['normal_impulse_kg_ms']*x for x in n)
            jt=(0.,0.,0.)
        if jt is None:raise ValueError('missing_tangent_impulse_vector')
        normal=tuple(x/BALL_MASS for x in jn);tangent=tuple(x/BALL_MASS for x in jt)
    normal_only=tuple(x+d for x,d in zip(vin,normal))
    residual=max(abs(o-i-dn-dt) for o,i,dn,dt in zip(out,vin,normal,tangent))
    orthogonality=abs(dot(tangent,n))
    if max(residual,orthogonality)>1e-7:raise ValueError('horizontal_impulse_closure_mismatch')
    if not all(math.isfinite(x) for v in (vin,n,axis,out,normal,tangent) for x in v):
        raise ValueError('nonfinite_horizontal_impulse')
    _,hla,_=angles(normal_only)
    normal_dir='undefined_direction' if hla is None else 'fair_direction' if abs(hla)<45 else 'foul_direction'
    speed=math.hypot(out[0],out[1])
    regime='undefined_horizontal' if speed<=1e-12 else 'nonforward' if out[1]<=0 else 'forward_lateral_dominant' if abs(out[0])>=out[1] else 'forward_fair_cone'
    values={}
    for prefix,v in zip(VECTORS,(vin,normal,tangent,normal_only,out,n,axis,vec(r,'point_velocity_'))):
        values.update({prefix+'_'+a:x for a,x in zip('xyz',v)})
    values.update(ev_ms=m['ev_ms'],vla_deg=m['vla_deg'],outgoing_forward_margin_ms=out[1]-abs(out[0]),
                  normal_only_forward_margin_ms=normal_only[1]-abs(normal_only[0]),horizontal_speed_ms=speed)
    return values,normal_dir,regime,residual,orthogonality


class HorizontalAudit:
    def __init__(self,sample_limit=500):
        self.summary=defaultdict(Counter);self.coverage=Counter();self.examples=[];self.sample_limit=sample_limit
        self.checks=defaultdict(float)

    def add(self,r,results):
        if r['status']!='contact':return
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        candidates=[('source_constant_mass_normal',r,True)]+[(k,m,False) for k,m in results.items()]
        for basis,m,source in candidates:
            for scope in scopes:self.coverage[scope,r['variant'],basis,m['collision_status']]+=1
            if m['collision_status']!='impulse_contact':continue
            values,normal_dir,regime,residual,ortho=decompose(r,m,source)
            self.checks['vector_closure_ms']=max(self.checks['vector_closure_ms'],residual)
            self.checks['tangent_normal_orthogonality_ms']=max(self.checks['tangent_normal_orthogonality_ms'],ortho)
            b=r.get('local_abs_b_over_r');nz=r.get('normal_z')
            for scope in scopes:
                for stage,bb,zz in [('overall','all','all'),('impact_normal_z',band(b,(.25,.5,.75,.95)),band(nz,(-.75,-.25,0,.25,.75)))]:
                    key=(scope,r['variant'],basis,stage,bb,zz,normal_dir,direction(m),regime)
                    g=self.summary[key];g['contacts']+=1
                    for name,value in values.items():
                        if value is not None:g['n_'+name]+=1;g['sum_'+name]+=value
            if b is not None and b>=.75 and len(self.examples)<self.sample_limit:
                self.examples.append(dict(game_id=r['game_id'],count_seq=r['count_seq'],variant=r['variant'],
                    outcome_basis=basis,b_over_r=b,source_z_m=r.get('_source_z_m'),
                    normal_only_direction=normal_dir,final_direction=direction(m),horizontal_regime=regime,**values))

    def write(self,output):
        rows=[]
        for key,g in sorted(self.summary.items()):
            row=dict(zip(KEYS,key),contacts=g['contacts'])
            for name in FIELDS:
                n=g['n_'+name];row['n_'+name]=n;row['avg_'+name]=g['sum_'+name]/n if n else None
            rows.append(row)
        write_csv(output/'finite_barrel_horizontal_summary.csv',KEYS+['contacts']+
            [p+k for k in FIELDS for p in ('n_','avg_')],rows)
        keys=['scope','variant','outcome_basis','collision_status']
        write_csv(output/'finite_barrel_horizontal_coverage.csv',keys+['geometry_contacts'],
            [dict(zip(keys,k),geometry_contacts=n) for k,n in sorted(self.coverage.items())])
        write_csv(output/'finite_barrel_horizontal_checks.csv',['metric','max_error'],
            [dict(metric=k,max_error=v) for k,v in sorted(self.checks.items())])
        write_csv(output/'finite_barrel_horizontal_examples.csv',[
            'game_id','count_seq','variant','outcome_basis','b_over_r','source_z_m',
            'normal_only_direction','final_direction','horizontal_regime']+FIELDS,self.examples)
