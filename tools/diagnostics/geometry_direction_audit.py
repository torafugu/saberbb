"""Geometry/direction associations and source-condition transitions, without imputation."""
import json
import math
import sqlite3
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

from contact_3d import band
from contact_collision_probe import write_csv
from effective_mass_probe import direction
from contact_selection_audit import region_flags

KEYS=['scope','variant','outcome_basis','stratum','band_1','band_2','direction']


def strata(r):
    yield 'overall','all','all'
    b=band(r.get('local_abs_b_over_r'),(.25,.5,.75,.95))
    nz=band(r.get('normal_z'),(-.75,-.25,0,.25,.75))
    z=band(r.get('_source_z_m'),(.60,.64,.68,.72,.76,.80,.84))
    yield 'impact_normal_z',b,nz
    nx,ny=r.get('normal_x'),r.get('normal_y')
    az=None if r['status']!='contact' or nx is None or ny is None or math.hypot(float(nx),float(ny))<1e-12 else math.degrees(math.atan2(float(nx),float(ny)))
    yield 'axial_normal_azimuth',z,band(az,(-135,-90,-45,0,45,90,135))


class DirectionAudit:
    def __init__(self):
        self.summary=defaultdict(Counter);self.joint=Counter()
        self.temp=tempfile.TemporaryDirectory(prefix='saberbb_direction_')
        self.db=sqlite3.connect(str(Path(self.temp.name)/'pairs.db'))
        self.db.execute('CREATE TABLE trials(game INTEGER,seq INTEGER,lever REAL,condition TEXT,payload TEXT,PRIMARY KEY(game,seq,lever,condition))')

    def close(self):
        self.db.close();self.temp.cleanup()

    def add(self,r,results):
        fields=['status','collision_status','adjusted_contact_type','ev_ms','vla_deg','hla_deg',
                'local_abs_b_over_r','normal_z','normal_x','normal_y','_source_z_m']
        payload={k:r.get(k) for k in fields}
        self.db.execute('INSERT INTO trials VALUES(?,?,?,?,?)',(r['game_id'],r['count_seq'],r['lever_m'],r['forecast_condition'],json.dumps(payload)))
        if r['status']!='contact':return
        scopes=['all_swings']+(['recorded_batting_only'] if r['adjusted_contact_type']!='UnrecordedBatting' else [])
        for basis,m in [('source_constant_mass_normal',r)]+[(name,x) for name,x in results.items() if x['collision_status']=='impulse_contact']:
            for scope in scopes:
                for stage,a,b in strata(r):
                    key=(scope,r['variant'],basis,stage,a,b,direction(m))
                    g=self.summary[key];g['contacts']+=1
                    for metric in ('ev_ms','vla_deg'):
                        value=m.get(metric)
                        if value is not None:
                            if not math.isfinite(value):raise ValueError('nonfinite_direction_metric')
                            g['n_'+metric]+=1;g['sum_'+metric]+=value
                    h=m.get('hla_deg')
                    if h is not None:
                        if not math.isfinite(h):raise ValueError('nonfinite_direction_metric')
                        g['n_hla']+=1;g['sin_hla']+=math.sin(math.radians(h));g['cos_hla']+=math.cos(math.radians(h))
                    for label,flag in region_flags(m).items():
                        if flag is not None:g['n_'+label]+=1;g[label]+=flag
                    if stage=='overall' and all(m.get(k) is not None for k in ('ev_ms','vla_deg','hla_deg')):
                        # Include direction separately so the asymmetric -45 boundary is explicit.
                        jointkey=key+(band(m['ev_ms'],(25,35,40,45,50)),
                            band(m['vla_deg'],(-50,-20,0,10,30,50)),band(h,(-135,-90,-45,0,45,90,135)))
                        self.joint[jointkey]+=1

    def transition_rows(self):
        counts=Counter()
        self.db.commit()
        # Source outcomes only. Before contact geometry is retained even if after has no contact.
        query='''SELECT a.lever,a.condition,b.condition,a.payload,b.payload FROM trials a JOIN trials b
                 ON a.game=b.game AND a.seq=b.seq AND a.lever=b.lever
                 WHERE a.condition IN ('prior_only','exact_observation_control')
                   AND b.condition NOT IN ('prior_only','exact_observation_control')'''
        for lever,before_condition,after_condition,before,after in self.db.execute(query):
            left,right=json.loads(before),json.loads(after)
            if left['adjusted_contact_type']!=right['adjusted_contact_type']:
                raise ValueError('condition_contact_type_mismatch')
            scopes=['all_swings']+(['recorded_batting_only'] if left['adjusted_contact_type']!='UnrecordedBatting' else [])
            before_state=left['status']+':'+left['collision_status']
            after_state=right['status']+':'+right['collision_status']
            for scope in scopes:
                for stage,a,b in strata(left):
                    region='undefined' if left.get('ev_ms') is None or left.get('vla_deg') is None else 'low_ev_and_high_vla' if left['ev_ms']<25 and left['vla_deg']>=50 else 'low_ev' if left['ev_ms']<25 else 'high_vla' if left['vla_deg']>=50 else 'other'
                    counts[(scope,lever,before_condition,after_condition,stage,a,b,region,band(left.get('ev_ms'),(25,35,40,45,50)),band(left.get('vla_deg'),(-50,-20,0,10,30,50)),
                            before_state,after_state,direction(left),direction(right))]+=1
        keys=['scope','lever_m','before_condition','after_condition','stratum','band_1','band_2',
              'before_region','before_ev_band','before_vla_band','before_state','after_state','before_direction','after_direction']
        return keys,[dict(zip(keys,k),paired_trials=n) for k,n in sorted(counts.items())]

    def write(self,output):
        rows=[]
        for key,g in sorted(self.summary.items()):
            n=g['n_hla'];length=math.hypot(g['sin_hla'],g['cos_hla'])
            row=dict(zip(KEYS,key),contacts=g['contacts'],n_hla=n,
                circular_mean_hla_deg=math.degrees(math.atan2(g['sin_hla'],g['cos_hla'])) if n and length>1e-12*n else None,
                hla_resultant_length=length/n if n else None)
            for metric in ('ev_ms','vla_deg'):
                nm=g['n_'+metric];row['n_'+metric]=nm;row['avg_'+metric]=g['sum_'+metric]/nm if nm else None
            for label in region_flags({}):
                nm=g['n_'+label];row['n_'+label]=nm;row[label+'_contacts']=g[label];row[label+'_fraction']=g[label]/nm if nm else None
            rows.append(row)
        columns=KEYS+['contacts','n_hla','circular_mean_hla_deg','hla_resultant_length']+[
            p+k for k in ('ev_ms','vla_deg') for p in ('n_','avg_')]+[
            v for k in region_flags({}) for v in ('n_'+k,k+'_contacts',k+'_fraction')]
        write_csv(output/'finite_barrel_direction_summary.csv',columns,rows)
        keys=KEYS+['ev_band','vla_band','hla_band']
        write_csv(output/'finite_barrel_direction_joint.csv',keys+['contacts'],[dict(zip(keys,k),contacts=n) for k,n in sorted(self.joint.items())])
        keys,rows=self.transition_rows()
        write_csv(output/'finite_barrel_observation_transitions.csv',keys+['paired_trials'],rows)
