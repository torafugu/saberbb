#!/usr/bin/env python3
"""Audit and aggregate observation-update contact CSV without rerunning games/searches."""
import argparse
import csv
import hashlib
import json
import math
import sqlite3
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

from contact_3d import ContactGeometry, solve_contact, band
from contact_collision_probe import cross, dot, write_csv
from execution_frame_probe import Paired
from rotating_bat_probe import norm, sub, mul

SCALARS=['lever_m','half_length_m','effective_radius_m','window_start_ms','window_end_ms',
         'forecast_error_x_m','forecast_error_z_m','forecast_time_error_ms','eye_for_sensor','min_evaluated_clearance_m']
CONTACT_SCALARS=['entry_since_release_sec','axial_at_entry_m','closest_axial_at_entry_m',
                'relative_normal_ms','impulse_delta_ms','relative_speed_ms','approach_speed_ms',
                'normal_speed_fraction','axial_fraction','normal_elevation_deg','bat_point_speed_ms']
VECTORS=['axis_at_entry_','normal_','point_velocity_','ball_velocity_','outgoing_velocity_']
METRICS=['forecast_error_x_m','forecast_error_z_m','forecast_time_error_ms','local_abs_b_over_r',
         'signed_axial_fraction','normal_z','normal_speed_fraction','bat_point_speed_ms','ev_ms','hla_deg','vla_deg',
         'incoming_z_ms','impulse_z_ms','outgoing_z_ms']
REQUIRED=['game_id','count_seq','adjusted_contact_type','legacy_group','forecast_condition','variant',
          'status','collision_status','contact_contract_version','contact_region','entry_time_offset_ms']+SCALARS+CONTACT_SCALARS+['ev_ms','hla_deg','vla_deg']+[
              prefix+axis+('_ms' if 'velocity' in prefix else '') for prefix in VECTORS for axis in 'xyz']


def number(row,key):
    x=float(row[key])
    if not math.isfinite(x):raise ValueError('non_finite:'+key)
    return x


def vec(row,prefix):
    return tuple(number(row,prefix+axis+('_ms' if 'velocity' in prefix else '')) for axis in 'xyz')


def local_impact(axis,normal,relative_velocity,region):
    """Frozen infinite-cylinder impact offset at entry; NOT global rotating-path minimum."""
    if region!='side':return None
    transverse=sub(relative_velocity,mul(axis,dot(relative_velocity,axis)))
    speed=norm(transverse)
    if speed<1e-12:return None
    direction=mul(transverse,-1/speed)
    return dot(normal,cross(axis,direction))


def audit(raw,checks):
    r=dict(raw)
    for key in SCALARS:r[key]=number(raw,key)
    for key in ('game_id','count_seq'):
        r[key]=int(raw[key])
        if r[key]<0:raise ValueError('invalid_key')
    if (r['lever_m']<=r['half_length_m'] or r['half_length_m']<=0 or r['effective_radius_m']<=0 or
        r['window_start_ms']>=r['window_end_ms']):raise ValueError('invalid_geometry_parameters')
    if r['variant']!=f"{r['forecast_condition']}_r{r['lever_m']:g}":raise ValueError('variant_condition_mismatch')
    if r['status'] not in ('contact','no_contact','overlap_at_start','unresolved'):raise ValueError('unknown_search_status')
    if raw['contact_contract_version']!='1':raise ValueError('unsupported_contact_contract')
    for k in METRICS:
        if k not in SCALARS:r[k]=None
    for k in ('ev_ms','hla_deg','vla_deg'):r[k]=None
    r['local_signed_b_over_r']=None
    if r['status']!='contact':
        if r['collision_status']!='not_evaluated:'+r['status']:raise ValueError('unexpected_collision_status')
        if any(raw[k] for k in ('ev_ms','hla_deg','vla_deg')):raise ValueError('noncontact_with_outcome')
        return r
    for k in CONTACT_SCALARS:r[k]=number(raw,k)
    axis=vec(raw,'axis_at_entry_');normal=vec(raw,'normal_')
    vin=vec(raw,'ball_velocity_');vbat=vec(raw,'point_velocity_')
    geometry=ContactGeometry(r['entry_since_release_sec'],axis,normal,r['axial_at_entry_m'],
                             r['closest_axial_at_entry_m'],r['half_length_m'],r['contact_region'])
    calculated=solve_contact(geometry,vin,vbat)
    if calculated['collision_status']!=r['collision_status']:raise ValueError('collision_status_mismatch')
    for key in CONTACT_SCALARS:
        if key in calculated:
            error=abs(r[key]-calculated[key]);checks[key]=max(checks[key],error)
            if error>1e-7:raise ValueError('reconstruction_mismatch:'+key)
    b=local_impact(axis,normal,sub(vin,vbat),r['contact_region'])
    if b is not None:
        # At the first entry, minimum evaluated gap is the entry gap (all prior gaps exceeded tolerance).
        distance=r['effective_radius_m']+r['min_evaluated_clearance_m']
        if distance<=0:raise ValueError('invalid_entry_distance')
        b*=distance/r['effective_radius_m']
    r.update(local_signed_b_over_r=b,local_abs_b_over_r=abs(b) if b is not None else None,
        signed_axial_fraction=r['closest_axial_at_entry_m']/r['half_length_m'],normal_z=normal[2],
        incoming_z_ms=None,impulse_z_ms=None,outgoing_z_ms=None)
    if calculated['outgoing_world_ms'] is not None:
        out=vec(raw,'outgoing_velocity_')
        checks['outgoing_vector_ms']=max(checks['outgoing_vector_ms'],norm(sub(out,calculated['outgoing_world_ms'])))
        if norm(sub(out,calculated['outgoing_world_ms']))>1e-7:raise ValueError('outgoing_vector_mismatch')
        for k in ('ev_ms','hla_deg','vla_deg'):
            r[k]=number(raw,k);error=abs(r[k]-calculated[k])
            checks[k]=max(checks[k],error)
            if error>1e-7:raise ValueError('outgoing_angle_or_speed_mismatch')
        r.update(incoming_z_ms=vin[2],impulse_z_ms=r['impulse_delta_ms']*normal[2],outgoing_z_ms=out[2])
        zerror=abs(out[2]-vin[2]-r['impulse_z_ms'])
        checks['vertical_velocity_closure_ms']=max(checks['vertical_velocity_closure_ms'],zerror)
    elif any(raw[k] for k in ('ev_ms','hla_deg','vla_deg')):raise ValueError('nonapproaching_with_outcome')
    return r


def signed_band(x):
    return 'zero_1e-8' if abs(x)<1e-8 else band(x,(-.1,-.03,0,.03,.1))


def strata(r):
    x,z,t=(r[k] for k in ('forecast_error_x_m','forecast_error_z_m','forecast_time_error_ms'))
    yield 'overall','all','all','all'
    yield 'forecast_x_z',signed_band(x),signed_band(z),'all'
    yield 'forecast_time',band(abs(t),(1,5,15,30)), 'all','all'
    yield 'forecast_z_sign',signed_band(z),'all','all'
    planar=band(math.hypot(x,z),(.03,.07,.16,.3))
    b=band(r['local_abs_b_over_r'],(.25,.5,.75,.95)) if r['local_abs_b_over_r'] is not None else ('cap' if r['contact_region']=='cap' else 'undefined')
    axial=band(abs(r['signed_axial_fraction']),(.25,.5,.75)) if r['signed_axial_fraction'] is not None else 'undefined'
    yield 'error_to_impact',planar,b,axial
    if r['status']=='contact':
        yield 'impact_to_normal',b,axial,band(r['normal_speed_fraction'],(.5,.8,.95))
        yield 'normal_to_outcome',band(r['normal_speed_fraction'],(.5,.8,.95)),band(r['normal_z'],(-.5,-.1,0,.1,.5)),band(r['bat_point_speed_ms'],(25,30,35,40))


class Aggregates:
    def __init__(self):self.groups=defaultdict(Counter)
    def add(self,r):
        for scope in ('all_swings','recorded_batting_only'):
            if scope=='recorded_batting_only' and r['adjusted_contact_type']=='UnrecordedBatting':continue
            for stage,a,b,c in strata(r):
                g=self.groups[scope,r['variant'],stage,a,b,c];g['states']+=1
                g[r['status']+'_states']+=1;g['impulse_contacts']+=r['collision_status']=='impulse_contact'
                g['cap_contacts']+=r['status']=='contact' and r['contact_region']=='cap'
                for k in METRICS:
                    if r[k] is not None:g[k]+=r[k];g[k+'_n']+=1
                if r['ev_ms'] is not None:g['ev40']+=r['ev_ms']>=40
                if r['hla_deg'] is not None:g['foul']+=abs(r['hla_deg'])>=45
                if r['vla_deg'] is not None:g['negative_vla']+=r['vla_deg']<0
    def write(self,output):
        keys=['scope','variant','stage','band_1','band_2','band_3'];rows=[]
        for key,g in sorted(self.groups.items()):
            r=dict(zip(keys,key),states=g['states'])
            for k in ('contact_states','no_contact_states','overlap_at_start_states','unresolved_states','impulse_contacts','cap_contacts'):r[k]=g[k]
            for k in METRICS:r['avg_'+k]=g[k]/g[k+'_n'] if g[k+'_n'] else None
            r.update(geometry_contacts=g['normal_z_n'],side_local_b_contacts=g['local_abs_b_over_r_n'],
                defined_hla_contacts=g['hla_deg_n'],defined_vla_contacts=g['vla_deg_n'],
                contact_pct=100*g['contact_states']/g['states'],
                ev_ge_40_pct=100*g['ev40']/g['ev_ms_n'] if g['ev_ms_n'] else None,
                foul_direction_pct=100*g['foul']/g['hla_deg_n'] if g['hla_deg_n'] else None,
                negative_vla_pct=100*g['negative_vla']/g['vla_deg_n'] if g['vla_deg_n'] else None)
            rows.append(r)
        fields=keys+['states','contact_states','no_contact_states','overlap_at_start_states','unresolved_states',
            'impulse_contacts','cap_contacts','geometry_contacts','side_local_b_contacts','defined_hla_contacts','defined_vla_contacts',
            'contact_pct','ev_ge_40_pct','foul_direction_pct','negative_vla_pct']+['avg_'+k for k in METRICS]
        write_csv(output/'forecast_geometry_chain.csv',fields,rows)


def run(input_dir,output):
    source=Path(input_dir)/'observation_update_contacts.csv'
    metadata=Path(input_dir)/'observation_update_metadata.json'
    assumptions=json.loads(metadata.read_text())
    with (Path(input_dir)/'observation_update_coverage.csv').open(newline='') as f:
        coverage_rows=list(csv.DictReader(f))
    if len(coverage_rows)!=1:raise ValueError('invalid_source_coverage')
    expected_rows=int(coverage_rows[0]['trial_rows'])
    if (assumptions.get('version')!=1 or assumptions.get('restitution')!=.5 or assumptions.get('effective_mass_kg')!=.6):
        raise ValueError('unsupported_observation_metadata')
    # Read-only sources; SQLite is a disposable join index, never the user's game DB.
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    aggregate=Aggregates();paired=Paired();checks=defaultdict(float);counts=Counter();status=Counter();geometry_pairs=defaultdict(Counter)
    sha=hashlib.sha256()
    with source.open('rb') as f:
        for chunk in iter(lambda:f.read(1024*1024),b''):sha.update(chunk)
    with tempfile.TemporaryDirectory(prefix='saberbb_geometry_') as temp:
        db=sqlite3.connect(str(Path(temp)/'join.db'))
        try:
            db.execute('CREATE TABLE trials (game INTEGER, seq INTEGER, lever REAL, condition TEXT, payload TEXT, PRIMARY KEY(game,seq,lever,condition))')
            with source.open(newline='') as f:
                reader=csv.DictReader(f)
                if not set(REQUIRED)<=set(reader.fieldnames or []):raise ValueError('missing_contact_csv_columns')
                if len(reader.fieldnames)!=len(set(reader.fieldnames)):raise ValueError('duplicate_csv_columns')
                for line,raw in enumerate(reader,2):
                    try:r=audit(raw,checks)
                    except (ValueError,TypeError,KeyError) as exc:raise ValueError(f'CSV line {line}: {exc}') from exc
                    try:db.execute('INSERT INTO trials VALUES(?,?,?,?,?)',(r['game_id'],r['count_seq'],r['lever_m'],r['forecast_condition'],json.dumps({k:r[k] for k in set(METRICS)|{
                        'status','collision_status','adjusted_contact_type','legacy_group','lever_m','forecast_condition',
                        'half_length_m','effective_radius_m','window_start_ms','window_end_ms','eye_for_sensor',
                        'entry_since_release_sec','entry_time_offset_ms','ball_velocity_x_ms','ball_velocity_y_ms','ball_velocity_z_ms'}})))
                    except sqlite3.IntegrityError as exc:raise ValueError(f'duplicate_trial: CSV line {line}') from exc
                    if (r['lever_m'] not in assumptions['levers_m'] or r['half_length_m']!=assumptions['half_length_m'] or
                        r['effective_radius_m']!=assumptions['radius_m'] or r['window_end_ms']!=1000*assumptions['half_window_sec']):
                        raise ValueError('row_metadata_geometry_mismatch')
                    aggregate.add(r);counts['trial_rows']+=1
                    status[r['variant'],r['status'],r['collision_status']]+=1
            db.commit()
            if counts['trial_rows']!=expected_rows:raise ValueError('source_coverage_row_mismatch')
            conditions=[r[0] for r in db.execute('SELECT DISTINCT condition FROM trials ORDER BY condition')]
            if conditions and 'prior_only' not in conditions:raise ValueError('missing_prior_condition')
            expected={'prior_only','exact_observation_control'}|{f'observed_sigma_{s:g}' for s in assumptions['sensor_sigma_m']}
            if conditions and set(conditions)!=expected:raise ValueError('metadata_conditions_mismatch')
            counts['ball_lever_keys']=db.execute('SELECT COUNT(*) FROM (SELECT DISTINCT game,seq,lever FROM trials)').fetchone()[0]
            if counts['trial_rows']!=counts['ball_lever_keys']*len(conditions):raise ValueError('incomplete_condition_pairs')
            if counts['ball_lever_keys']!=int(coverage_rows[0]['valid_histories'])*len(assumptions['levers_m']):
                raise ValueError('source_coverage_key_mismatch')
            query='''SELECT a.payload,b.payload FROM trials a JOIN trials b
                ON a.game=b.game AND a.seq=b.seq AND a.lever=b.lever
                WHERE a.condition='prior_only' AND b.condition!='prior_only' '''
            for before,after in db.execute(query):
                before=json.loads(before);after=json.loads(after)
                for k in ('adjusted_contact_type','legacy_group','half_length_m','effective_radius_m','window_start_ms','window_end_ms','eye_for_sensor'):
                    if before[k]!=after[k]:raise ValueError('inconsistent_pair:'+k)
                if before['status']==after['status']=='contact':
                    if vec(before,'ball_velocity_')!=vec(after,'ball_velocity_'):raise ValueError('inconsistent_pair:incoming_velocity')
                    clock_before=before['entry_since_release_sec']-number(before,'entry_time_offset_ms')/1000
                    clock_after=after['entry_since_release_sec']-number(after,'entry_time_offset_ms')/1000
                    if abs(clock_before-clock_after)>1e-10:raise ValueError('inconsistent_pair:plate_clock')
                contrast='prior_to_'+after['forecast_condition'];counts['paired_trials']+=1
                for kind,legacy in [(before['adjusted_contact_type'],before['legacy_group']),('all','all')]:
                    paired.add(kind,legacy,before['lever_m'],contrast,before,after)
                if before['collision_status']==after['collision_status']=='impulse_contact':
                    g=geometry_pairs[before['lever_m'],contrast];g['paired_impulse_contacts']+=1
                    for k in METRICS:
                        if before[k] is not None and after[k] is not None:
                            g[k+'_n']+=1;g[k+'_before']+=before[k];g[k+'_after']+=after[k]
                            g[k+'_abs_delta']+=abs(after[k]-before[k]) if k!='hla_deg' else abs((after[k]-before[k]+180)%360-180)
        finally:db.close()
    aggregate.write(output);paired.write(output,prefix='forecast_geometry')
    fields=['lever_m','contrast','paired_impulse_contacts']+[s+k for k in METRICS for s in ('pairs_','before_','after_','abs_delta_')]
    rows=[]
    for (lever,contrast),g in sorted(geometry_pairs.items()):
        r=dict(lever_m=lever,contrast=contrast,paired_impulse_contacts=g['paired_impulse_contacts'])
        for k in METRICS:
            n=g[k+'_n'];r['pairs_'+k]=n
            for prefix,suffix in [('before_','_before'),('after_','_after'),('abs_delta_','_abs_delta')]:r[prefix+k]=g[k+suffix]/n if n else None
        rows.append(r)
    write_csv(output/'forecast_geometry_geometry_pairs.csv',fields,rows)
    write_csv(output/'forecast_geometry_status.csv',['variant','search_status','collision_status','states'],
        [dict(variant=k[0],search_status=k[1],collision_status=k[2],states=n) for k,n in sorted(status.items())])
    write_csv(output/'forecast_geometry_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    coverage={k:counts[k] for k in ('trial_rows','ball_lever_keys','paired_trials')};coverage['conditions']=len(conditions)
    write_csv(output/'forecast_geometry_coverage.csv',list(coverage),[coverage])
    (output/'forecast_geometry_metadata.json').write_text(json.dumps(dict(version=1,input_sha256=sha.hexdigest(),
        input_metadata=assumptions,local_b_definition='side_only_frozen_axis_relative_velocity_at_entry',
        radius='bat_plus_ball_effective_radius',note='Descriptive joint strata, not causal interventions. No physics/search changes.'),indent=2)+'\n')
    return coverage


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__);p.add_argument('--input-dir',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True);a=p.parse_args()
    try:print(run(a.input_dir,a.output_dir))
    except (OSError,ValueError,sqlite3.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
