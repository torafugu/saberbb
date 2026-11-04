#!/usr/bin/env python3
"""Audit cached tilt HLA/VLA by forecast height and batting side; no DB or search."""
import argparse
import csv
import json
import math
import hashlib
from collections import Counter,defaultdict
from pathlib import Path

from bat_axis_origin_audit import check
from contact_collision_probe import angles,write_csv
from contact_3d import band
from effective_mass_probe import direction
from execution_contact_posture import digest,same
from plan_tilt_sweep import KINEMATICS
from rotating_bat_probe import norm
from statcast_distribution_compare import load_csv

HLA_EDGES=(-180,-90,-45,-30,-15,-10,0,10,15,30,45,90,180)
HEIGHT_EDGES=(0,.3,.6,.9,1.2,1.5)
EV_EDGES=(0,10,20,25,30,35,40,45,50,55,60)
VLA_EDGES=(-90,-60,-30,0,10,20,30,40,50,60,90)
KEYS=['dataset','scenario','stratum','handedness','forecast_height_band_m','model']
METRICS=('ev_ms','vla_deg','abs_outgoing_x_ms','abs_hla_deg')


def number(x):
    if isinstance(x,bool) or not isinstance(x,(int,float)) or not math.isfinite(x):raise ValueError('nonfinite_or_invalid_number')
    return x


def vector(v):
    if not isinstance(v,(list,tuple)) or len(v)!=3:raise ValueError('invalid_vector')
    return tuple(number(x) for x in v)


def strata(side,height):
    label='right' if side==1 else 'left';height_band=band(height,HEIGHT_EDGES)
    yield 'overall','all','all'
    yield 'handedness',label,'all'
    yield 'forecast_height','all',height_band
    yield 'handedness_height',label,height_band


def hla_class(m):
    if m['collision_status']!='impulse_contact':return m['collision_status']
    h=m['hla_deg']
    if h is None:return 'undefined_hla'
    if abs(h)>=45:return 'foul_direction'
    return 'central_abs_lt10' if abs(h)<10 else 'negative_fair' if h<0 else 'positive_fair'


def scenario_names(meta):
    if meta['version']==1:return ['saved_tilt']+[f'tilt_{a:g}deg' for a in meta['angles_deg']]
    if meta['version'] in (2,3,4) and meta.get('scenarios'):
        conditions=set()
        for name,c in meta['scenarios'].items():
            tilt=number(c['tilt_deg']);yaw=number(c['yaw_batter_deg'])
            aim=number(c['aim_z_offset_m']) if meta['version']>=3 else None
            label=f'tilt_{tilt:g}deg_yaw_{yaw:g}deg'+(f'_aim_z_{aim*1000:g}mm' if aim is not None else '')
            speed=number(c['planned_speed_ratio']) if meta['version']==4 else None
            if speed is not None:
                if not 0<speed<=1:raise ValueError('invalid_metadata_speed_ratio')
                label+=f'_speed_r{speed:g}'
            if not 0<=tilt<=60 or abs(yaw)>45 or name!=label:raise ValueError('invalid_axis_metadata_scenario')
            conditions.add((tilt,yaw,aim,speed))
        aims=meta['aim_z_offsets_m'] if meta['version']>=3 else [None]
        speeds=meta['planned_speed_ratios'] if meta['version']==4 else [None]
        if conditions!={(x,y,z,r) for x in meta['angles_deg'] for y in meta['yaws_deg'] for z in aims for r in speeds}:raise ValueError('missing_axis_metadata_scenarios')
        return ['saved_tilt']+list(meta['scenarios'])
    raise ValueError('unsupported_tilt_metadata')


class Audit:
    def __init__(self):
        self.selection=defaultdict(Counter);self.summary=defaultdict(Counter);self.hist=Counter()
        self.pairs=defaultdict(Counter);self.transitions=Counter();self.replay=defaultdict(Counter)
        self.ev_hist=Counter();self.kin=defaultdict(Counter);self.checks=defaultdict(float)

    def add(self,dataset,records,models,kinematic_fields=KINEMATICS):
        baseline=records['saved_tilt'];reference=baseline['forecast_plan'];target=vector(reference['forecast_position_world_m'])
        axis=vector(reference['ideal_axis_world']);check(self.checks,'ideal_axis_unit',abs(norm(axis)-1))
        if abs(axis[0])<1e-8:raise ValueError('ambiguous_saved_batting_side')
        side=1 if axis[0]>0 else -1
        for name,r in records.items():
            plan=r['forecast_plan']
            for field in ('forecast_position_world_m','nominal_time_sec','execution_angle_error_deg'):
                same(reference[field],plan[field],self.checks,'common_'+field)
            ratio=number(plan.get('planned_speed_ratio',1.))
            if not 0<ratio<=1:raise ValueError('invalid_cached_speed_ratio')
            same(reference['ideal_velocity_world_ms'],tuple(v/ratio for v in vector(plan['ideal_velocity_world_ms'])),
                 self.checks,'common_unscaled_ideal_velocity_world_ms')
            a=vector(plan['ideal_axis_world']);check(self.checks,'candidate_axis_unit',abs(norm(a)-1))
            if a[0]*side<=0:raise ValueError('inconsistent_saved_batting_side')
            invalid=r.get('invalid_reason')
            if invalid not in (None,'execution_starts_before_plan','execution_starts_before_release'):raise ValueError('unknown_clock_status')
            if invalid!=baseline.get('invalid_reason'):raise ValueError('inconsistent_clock_population')
            kg=self.kin[dataset,name];kg['trials']+=1;kg['clock_invalid_trials']+=invalid is not None
            if invalid:
                status=invalid;contact=None;results={m:dict(collision_status='not_evaluated:'+invalid,ev_ms=None) for m in models}
            else:
                status=r['search_status'];contact=r['contact'];results=r['responses']
                if status not in ('contact','no_contact','unresolved','overlap_at_start') or status!=contact['status']:raise ValueError('invalid_search_status')
                if set(results)!=set(models):raise ValueError('response_model_mismatch')
                if status=='contact' and r['entry_time_sec'] is None:raise ValueError('contact_without_entry')
                if status!='contact' and r['entry_time_sec'] is not None:raise ValueError('noncontact_with_entry')
                kin=r['kinematics'];kg['evaluable_trials']+=1
                for metric in kinematic_fields:kg[metric]+=number(kin[metric])
                kg['pulse_pivot_below_ground']+=kin['pulse_min_pivot_z_m']<0
            for model,m in results.items():
                success=m['collision_status']=='impulse_contact';fair=direction(m)=='fair_direction'
                if success:
                    if status!='contact':raise ValueError('successful_noncontact')
                    out=vector(m['outgoing_world_ms']);ev,hla,vla=angles(out)
                    for label,value in (('ev_ms',ev),('hla_deg',hla),('vla_deg',vla)):
                        if value is None:
                            if m[label] is not None:raise ValueError('undefined_angle_filled')
                        else:check(self.checks,'saved_vector_'+label,abs(number(m[label])-value),1e-7)
                    if ev is None or vla is None:raise ValueError('undefined_success_ev_vla')
                elif m.get('ev_ms') is not None:raise ValueError('failed_collision_with_ev')
                g=self.replay[dataset,name,model];g['trials']+=1;g['execution_clock_invalid_trials']+=invalid is not None
                g['evaluable_trials']+=invalid is None;g['geometry_contacts']+=status=='contact'
                g['evaluable_geometry_contacts']+=status=='contact';g['successful_contacts']+=success;g['fair_contacts']+=fair
                if success:
                    g['ev_ms']+=m['ev_ms'];g['vla_deg']+=m['vla_deg'];g['abs_outgoing_x_ms']+=abs(m['outgoing_world_ms'][0])
                    for field,predicate in [('low_ev',m['ev_ms']<25),('high_ev',m['ev_ms']>=45),('downward',m['vla_deg']<0),('high_vla',m['vla_deg']>=50)]:
                        g[field]+=predicate;g['fair_'+field]+=predicate and fair
                    for cohort in ('all_successful','fair_direction'):
                        if cohort=='fair_direction' and not fair:continue
                        self.ev_hist[dataset,name,model,cohort,band(m['ev_ms'],EV_EDGES),band(m['vla_deg'],VLA_EDGES)]+=1
                for stage,hand,height_band in strata(side,target[2]):
                    key=(dataset,name,stage,hand,height_band,model);s=self.selection[key]
                    s['trials']+=1;s['clock_invalid_trials']+=invalid is not None;s['evaluable_trials']+=invalid is None
                    s['geometry_contacts']+=status=='contact';s['successful_contacts']+=success;s['fair_contacts']+=fair
                    s[status+'_trials']+=1
                    s['unsupported_cap_contacts']+=m['collision_status']=='not_evaluated:unsupported_cap'
                    s['out_of_curve_contacts']+=m['collision_status']=='not_evaluated:out_of_curve'
                    s['non_approaching_contacts']+=m['collision_status'] in ('non_approaching','not_evaluated:non_approaching')
                    s['foul_contacts']+=direction(m)=='foul_direction';s['undefined_hla_contacts']+=success and m['hla_deg'] is None
                    if invalid is None:
                        s['pulse_pivot_below_ground']+=r['kinematics']['pulse_min_pivot_z_m']<0
                        s['pulse_min_pivot_z_m']+=r['kinematics']['pulse_min_pivot_z_m']
                    if not success:continue
                    for cohort in ('all_successful','fair_direction'):
                        if cohort=='fair_direction' and not fair:continue
                        h=m['hla_deg'];g=self.summary[key+(cohort,)];g['contacts']+=1
                        for metric,value in [('ev_ms',m['ev_ms']),('vla_deg',m['vla_deg']),('abs_outgoing_x_ms',abs(out[0]))]:g[metric]+=value
                        g['low_ev']+=m['ev_ms']<25;g['high_ev']+=m['ev_ms']>=45;g['downward']+=m['vla_deg']<0;g['high_vla']+=m['vla_deg']>=50
                        if h is not None:
                            g['defined_hla_contacts']+=1;g['abs_hla_deg']+=abs(h)
                            g['central_abs_lt5']+=abs(h)<5;g['central_abs_lt10']+=abs(h)<10;g['central_abs_lt20']+=abs(h)<20
                            rad=math.radians(h);g['sin_hla']+=math.sin(rad);g['cos_hla']+=math.cos(rad);g['sin_batter_hla']+=side*math.sin(rad)
                        for frame,value in [('world',h),('batter_relative',side*h if h is not None else None)]:
                            self.hist[key+(cohort,frame,band(value,HLA_EDGES),band(m['vla_deg'],VLA_EDGES))]+=1
        for name,r in records.items():
            if name=='saved_tilt':continue
            for model in models:
                if baseline.get('invalid_reason'):
                    before=after=dict(collision_status='not_evaluated:'+baseline['invalid_reason'])
                else:before=baseline['responses'][model];after=r['responses'][model]
                common=all(m['collision_status']=='impulse_contact' for m in (before,after))
                for stage,hand,height_band in strata(side,target[2]):
                    key=(dataset,name,stage,hand,height_band,model)
                    self.transitions[key+(hla_class(before),hla_class(after))]+=1
                    if not common:continue
                    for cohort in ('common_successful','common_fair'):
                        if cohort=='common_fair' and not all(direction(m)=='fair_direction' for m in (before,after)):continue
                        g=self.pairs[key+(cohort,)];g['contacts']+=1
                        for label,m in [('before',before),('after',after)]:
                            h=m['hla_deg'];g[label+'_fair']+=direction(m)=='fair_direction'
                            if h is not None:g[label+'_defined_hla']+=1;g[label+'_abs_hla']+=abs(h);g[label+'_central10']+=abs(h)<10
                        if all(m['hla_deg'] is not None for m in (before,after)):
                            g['defined_pairs']+=1;g['abs_delta_hla']+=abs((after['hla_deg']-before['hla_deg']+180)%360-180)

    def validate(self,dataset,folder,meta):
        expected_keys={(dataset,s,m) for s in scenario_names(meta) for m in meta['models']}
        found=set();origins=set()
        fields=['trials','execution_clock_invalid_trials','evaluable_trials','geometry_contacts','evaluable_geometry_contacts',
                'successful_contacts','fair_contacts','low_ev','high_ev','downward','high_vla','fair_low_ev','fair_high_ev','fair_downward','fair_high_vla']
        for r in load_csv(folder/'plan_tilt_summary.csv',['scenario','scope','model','stratum','orientation']+fields):
            if (r['scope'],r['stratum'])!=('all_swings','overall'):continue
            origin=(r['scenario'],r['orientation'],r['model'])
            if origin in origins:raise ValueError('duplicate_source_summary_row')
            origins.add(origin)
            if r['scenario'] not in scenario_names(meta)[1:] or r['orientation'] not in ('saved_tilt','planned_tilt'):raise ValueError('invalid_summary_condition')
            name='saved_tilt' if r['orientation']=='saved_tilt' else r['scenario']
            key=(dataset,name,r['model'])
            if key not in expected_keys or r['variant']!=meta['variant']:raise ValueError('unknown_summary_key')
            found.add(key);g=self.replay[key]
            for f in fields:
                if int(r[f])!=g[f]:raise ValueError('summary_count_mismatch:'+f)
            for f in ('ev_ms','vla_deg','abs_outgoing_x_ms'):
                n=g['successful_contacts']
                if int(r['n_'+f])!=n:raise ValueError('summary_metric_count_mismatch')
                if n:check(self.checks,'summary_avg_'+f,abs(float(r['avg_'+f])-g[f]/n),1e-7)
        expected_origins={(name,o,m) for name in scenario_names(meta)[1:] for o in ('saved_tilt','planned_tilt') for m in meta['models']}
        if found!=expected_keys or origins!=expected_origins:raise ValueError('missing_summary_states')
        expected=Counter();seen=set();group_counts=Counter()
        for r in load_csv(folder/'plan_tilt_ev_vla.csv',['scenario','scope','model','orientation','cohort','ev_band_ms','vla_band_deg','contacts','cohort_contacts','fraction']):
            if r['scope']!='all_swings':continue
            if (r['scenario'],r['orientation'],r['model']) not in expected_origins or r['variant']!=meta['variant']:raise ValueError('invalid_histogram_condition')
            name='saved_tilt' if r['orientation']=='saved_tilt' else r['scenario']
            key=(dataset,name,r['model'],r['cohort'],r['ev_band_ms'],r['vla_band_deg'])
            original=(r['scenario'],r['orientation'])+key
            if original in seen:raise ValueError('duplicate_source_histogram_cell')
            seen.add(original);n=int(r['contacts']);g=self.replay[key[:3]]
            if r['cohort'] not in ('all_successful','fair_direction'):raise ValueError('unknown_histogram_cohort')
            total=g['fair_contacts' if r['cohort']=='fair_direction' else 'successful_contacts']
            if n<=0 or total!=int(r['cohort_contacts']) or not math.isclose(float(r['fraction']),n/total,abs_tol=1e-12):raise ValueError('histogram_denominator_mismatch')
            if key in expected and expected[key]!=n:raise ValueError('inconsistent_repeated_control_histogram')
            expected[key]=n;group_counts[r['scenario'],r['orientation'],r['model'],r['cohort']]+=n
        for scenario,orientation,model in expected_origins:
            name='saved_tilt' if orientation=='saved_tilt' else scenario
            for cohort in ('all_successful','fair_direction'):
                count_key='fair_contacts' if cohort=='fair_direction' else 'successful_contacts'
                if group_counts[scenario,orientation,model,cohort]!=self.replay[dataset,name,model][count_key]:raise ValueError('missing_source_histogram_group')
        actual=Counter({k:v for k,v in self.ev_hist.items() if k[0]==dataset})
        if actual!=expected:raise ValueError('cached_ev_vla_histogram_mismatch')
        found=set()
        for r in load_csv(folder/'plan_tilt_kinematics.csv',['scenario','scope','trials','evaluable_trials','clock_invalid_trials','pulse_pivot_below_ground']):
            if r['scope']!='all_swings':continue
            key=dataset,r['scenario']
            if key in found or key not in self.kin:raise ValueError('invalid_kinematics_key')
            found.add(key);g=self.kin[key]
            for f in ('trials','evaluable_trials','clock_invalid_trials','pulse_pivot_below_ground'):
                if int(r[f])!=g[f]:raise ValueError('kinematics_count_mismatch')
            for f in meta.get('kinematics_fields',KINEMATICS):
                if g['evaluable_trials']:check(self.checks,'kinematics_avg_'+f,abs(float(r['avg_'+f])-g[f]/g['evaluable_trials']),1e-7)
        if found!={k for k in self.kin if k[0]==dataset}:raise ValueError('missing_kinematics_states')

    def write(self,out):
        selection_fields=['trials','clock_invalid_trials','evaluable_trials','geometry_contacts','successful_contacts','fair_contacts','foul_contacts','undefined_hla_contacts','pulse_pivot_below_ground','no_contact_trials','overlap_at_start_trials','unresolved_trials','unsupported_cap_contacts','out_of_curve_contacts','non_approaching_contacts']
        rows=[]
        for key,g in sorted(self.selection.items()):
            rows.append(dict(zip(KEYS,key),**{f:g[f] for f in selection_fields},avg_pulse_min_pivot_z_m=g['pulse_min_pivot_z_m']/g['evaluable_trials'] if g['evaluable_trials'] else None))
        write_csv(out/'plan_tilt_direction_selection.csv',KEYS+selection_fields+['avg_pulse_min_pivot_z_m'],rows)
        rows=[]
        for key,g in sorted(self.summary.items()):
            r=dict(zip(KEYS+['cohort'],key));n=g['contacts'];nh=g['defined_hla_contacts']
            for f in ('contacts','defined_hla_contacts','low_ev','high_ev','downward','high_vla'):r[f]=g[f]
            for f in METRICS:r['avg_'+f]=g[f]/(nh if f=='abs_hla_deg' else n) if (nh if f=='abs_hla_deg' else n) else None
            for cut in (5,10,20):r[f'central_abs_lt{cut}_fraction']=g[f'central_abs_lt{cut}']/nh if nh else None
            length=math.hypot(g['sin_hla'],g['cos_hla'])
            r['world_circular_mean_hla_deg']=math.degrees(math.atan2(g['sin_hla'],g['cos_hla'])) if nh and length/nh>1e-12 else None
            batter_length=math.hypot(g['sin_batter_hla'],g['cos_hla'])
            r['batter_relative_circular_mean_hla_deg']=math.degrees(math.atan2(g['sin_batter_hla'],g['cos_hla'])) if nh and batter_length/nh>1e-12 else None
            r['world_hla_resultant_length']=length/nh if nh else None
            rows.append(r)
        fields=KEYS+['cohort','contacts','defined_hla_contacts','low_ev','high_ev','downward','high_vla']+['avg_'+f for f in METRICS]+[f'central_abs_lt{cut}_fraction' for cut in (5,10,20)]+['world_circular_mean_hla_deg','batter_relative_circular_mean_hla_deg','world_hla_resultant_length']
        write_csv(out/'plan_tilt_direction_summary.csv',fields,rows)
        keys=KEYS+['cohort','angle_frame','hla_band_deg','vla_band_deg']
        write_csv(out/'plan_tilt_direction_histogram.csv',keys+['contacts','cohort_contacts','fraction'],[
            dict(zip(keys,k),contacts=n,cohort_contacts=self.summary[k[:7]]['contacts'],fraction=n/self.summary[k[:7]]['contacts']) for k,n in sorted(self.hist.items())])
        rows=[]
        for key,g in sorted(self.pairs.items()):
            r=dict(zip(KEYS+['cohort'],key),contacts=g['contacts'],defined_hla_pairs=g['defined_pairs'],avg_abs_delta_hla_deg=g['abs_delta_hla']/g['defined_pairs'] if g['defined_pairs'] else None)
            for side in ('before','after'):
                n=g[side+'_defined_hla'];r[side+'_defined_hla']=n
                r[side+'_avg_abs_hla_deg']=g[side+'_abs_hla']/n if n else None
                r[side+'_central_abs_lt10_fraction']=g[side+'_central10']/n if n else None
                r[side+'_fair_fraction']=g[side+'_fair']/g['contacts']
            rows.append(r)
        fields=KEYS+['cohort','contacts','defined_hla_pairs','avg_abs_delta_hla_deg']+[s+f for s in ('before','after') for f in ('_defined_hla','_avg_abs_hla_deg','_central_abs_lt10_fraction','_fair_fraction')]
        write_csv(out/'plan_tilt_direction_pairs.csv',fields,rows)
        keys=KEYS+['before_hla_class','after_hla_class']
        write_csv(out/'plan_tilt_direction_transitions.csv',keys+['trials'],[dict(zip(keys,k),trials=n) for k,n in sorted(self.transitions.items())])


def load_inputs(input_dirs,audit):
    """Stream and validate cached pitches into an audit; never replay physics."""
    datasets={};inputs={};contract=None
    for folder in map(Path,input_dirs):
        meta=json.loads((folder/'plan_tilt_metadata.json').read_text())
        if meta['version'] not in (1,2,3,4) or meta['status']!='sampled_sensitivity_not_calibrated':raise ValueError('unsupported_tilt_metadata')
        dataset='seed:'+meta['sample_seed']
        if dataset in datasets:raise ValueError('duplicate_seed_dataset')
        identity={k:meta[k] for k in ('variant','models','duration_sec','anchor_fraction','plan_effort','execution_ratio','start_delay_sec')}
        identity['source_hashes']={Path(k).name:v for k,v in meta['inputs'].items() if Path(k).name!='plan_tilt_sample.csv'}
        if contract is not None and contract!=identity:raise ValueError('incompatible_tilt_datasets')
        contract=identity
        scenarios=scenario_names(meta)
        if len(set(scenarios))!=len(scenarios) or len(set(meta['models']))!=len(meta['models']):raise ValueError('duplicate_metadata_conditions')
        manifest={}
        for r in load_csv(folder/'plan_tilt_sample.csv',['game_id','count_seq','selection_hash']):
            key=int(r['game_id']),int(r['count_seq'])
            expected=hashlib.sha256(f"plan-tilt-v1:{meta['sample_seed']}:{key[0]}:{key[1]}".encode()).hexdigest()
            if min(key)<0 or key in manifest or r['selection_hash']!=expected:raise ValueError('invalid_sample_manifest')
            manifest[key]=expected
        expected_hash=[v for k,v in meta['inputs'].items() if Path(k).name=='plan_tilt_sample.csv']
        if len(expected_hash)!=1 or digest(folder/'plan_tilt_sample.csv')!=expected_hash[0]:raise ValueError('sample_manifest_hash_mismatch')
        if len(manifest)!=meta['sampled_trials'] or not manifest:raise ValueError('sample_size_mismatch')
        if 'sampling_exclusion' in meta:
            ex=meta['sampling_exclusion'];excluded=set()
            for key in ex['excluded_keys']:
                if not isinstance(key,list) or len(key)!=2 or any(type(v) is not int or v<0 for v in key):raise ValueError('invalid_excluded_key')
                if tuple(key) in excluded:raise ValueError('duplicate_excluded_key')
                excluded.add(tuple(key))
            if not excluded or len(excluded)!=ex['excluded_count']:raise ValueError('exclusion_count_mismatch')
            if excluded&set(manifest):raise ValueError('holdout_overlaps_excluded_sample')
            if meta['population_trials_before_exclusion']!=meta['population_trials']+len(excluded):raise ValueError('exclusion_population_mismatch')
        seen=set();group={};current=None
        def flush():
            if set(group)!=set(scenarios):raise ValueError('missing_pitch_scenarios')
            audit.add(dataset,group,meta['models'],meta.get('kinematics_fields',KINEMATICS))
        with (folder/'plan_tilt_states.jsonl').open() as f:
            for line in f:
                r=json.loads(line);key=(r['game_id'],r['count_seq']);name=r['scenario']
                if key not in manifest or name not in scenarios:raise ValueError('unknown_cached_trial')
                if meta['version'] in (2,3,4) and name!='saved_tilt':
                    axis=vector(r['forecast_plan']['ideal_axis_world']);side=1 if axis[0]>0 else -1
                    c=meta['scenarios'][name]
                    check(audit.checks,'declared_planned_tilt_deg',abs(math.degrees(math.asin(max(-1,min(1,axis[2]))))-c['tilt_deg']))
                    check(audit.checks,'declared_planned_yaw_deg',abs(math.degrees(math.atan2(axis[1],side*axis[0]))-c['yaw_batter_deg']))
                    if meta['version']>=3:
                        plan=r['forecast_plan'];offset=(0.,0.,c['aim_z_offset_m'])
                        same(offset,plan['planned_aim_offset_world_m'],audit.checks,'declared_aim_offset')
                        aim=tuple(a+b for a,b in zip(vector(plan['forecast_position_world_m']),offset))
                        same(aim,plan['planned_aim_point_world_m'],audit.checks,'declared_aim_point')
                        same(aim,plan['ideal_point_world_m'],audit.checks,'ideal_aim_point')
                if meta['version']==4:
                    ratio=1. if name=='saved_tilt' else meta['scenarios'][name]['planned_speed_ratio']
                    same(ratio,r['forecast_plan'].get('planned_speed_ratio',1.),audit.checks,'declared_planned_speed_ratio')
                elif 'planned_speed_ratio' in r['forecast_plan']:raise ValueError('undeclared_planned_speed_ratio')
                if key!=current:
                    if current is not None:flush()
                    if key in seen:raise ValueError('noncontiguous_or_duplicate_cached_pitch')
                    seen.add(key);group={};current=key
                if name in group:raise ValueError('duplicate_cached_scenario')
                group[name]=r
            if current is not None:flush()
        if seen!=set(manifest):raise ValueError('missing_cached_pitches')
        audit.validate(dataset,folder,meta)
        datasets[dataset]=dict(input_dir=str(folder),sampled_trials=len(seen),source_metadata=meta)
        for filename in ('plan_tilt_metadata.json','plan_tilt_sample.csv','plan_tilt_states.jsonl','plan_tilt_summary.csv','plan_tilt_ev_vla.csv','plan_tilt_kinematics.csv'):
            inputs[str(folder/filename)]=digest(folder/filename)
    if not datasets:raise ValueError('no_input_datasets')
    return datasets,inputs


def run(input_dirs,output):
    audit=Audit();datasets,inputs=load_inputs(input_dirs,audit)
    out=Path(output);out.mkdir(parents=True,exist_ok=False);audit.write(out)
    write_csv(out/'plan_tilt_direction_checks.csv',['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(audit.checks.items())])
    metadata=dict(version=1,status='cached_direction_audit_not_calibrated',new_searches=0,db_reads=0,datasets=datasets,inputs=inputs,
        hla_edges_deg=HLA_EDGES,height_edges_m=HEIGHT_EDGES,vertical_edges_deg=VLA_EDGES,
        conventions='World HLA=atan2(vx,vy), world z vertical. right=sign+1 and left=sign-1 inferred from saved ideal bat axis. Batter-relative HLA=sign*world HLA; signs are not labeled pull/opposite.',
        limitations=['Only all_swings is audited; invalid cached rows do not contain recorded-batting membership.',
            'Each seed is separate. Samples may overlap; no pooling or independent-sample significance is asserted.',
            'Forecast height is predicted target world z, not an actual plate height or normalized strike-zone coordinate.',
            'All successful and fair-direction cohorts have different selections. Strata/models/angle frames overlap.',
            'Undefined HLA is retained explicitly. Central fractions use defined HLA; histogram fractions use all cohort contacts.',
            'Circular means avoid the +/-180 discontinuity. Batter-relative angles are mirrored per pitch before aggregation.',
            'Paired statistics condition on common success/common fair. No inference of intended pull/opposite targeting.',
            'Saved vectors are checked against EV/HLA/VLA; source summary, EV/VLA histograms and kinematics are reconciled.',
            'No collision response replay, contact search, DB access, Statcast fit, or production model changes.'])
    (out/'plan_tilt_direction_metadata.json').write_text(json.dumps(metadata,indent=2)+'\n')
    return dict(datasets=len(datasets),new_searches=0,db_reads=0)


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,nargs='+',required=True);p.add_argument('--output-dir',type=Path,required=True)
    a=p.parse_args()
    try:print(json.dumps(run(a.input_dir,a.output_dir),indent=2))
    except (OSError,ValueError,KeyError,TypeError,ZeroDivisionError) as exc:p.exit(1,f'Cached direction audit failed: {exc}\n')
