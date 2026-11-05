#!/usr/bin/env python3
"""Paired whole-game bootstrap of audited, frozen intent-collision cached outcomes."""
import argparse
from collections import Counter, defaultdict
import csv
import json
import math
from pathlib import Path
import random

import bat_vibration_profile as profile
import rust_intent_collision_probe as collision
from batting_intent import FROZEN_POLICIES
from contact_3d import band
from contact_collision_probe import write_csv
from execution_contact_posture import digest, same
from plan_speed_mixture_bootstrap import quantile
from rust_observed_intent_audit import decide

PREFIX = 'rust_intent_game_bootstrap'
SIDE_FIELDS = ('plans','searched',*collision.EVENTS,'fair_n_ev_ms','fair_sum_ev_ms',
               'fair_n_vla_deg','fair_sum_vla_deg','fair_n_spin_rpm','fair_sum_spin_rpm')
PAIR_FIELDS = ('common_fair','common_fair_downward_delta','common_fair_high_vla_delta',
               'common_fair_line_window_delta','common_fair_delta_ev_ms','common_fair_delta_vla_deg',
               'common_fair_delta_spin_rpm')
SIZE = 2*len(SIDE_FIELDS)+len(PAIR_FIELDS)
GROUP_FIELDS = ('strategy','preferred_intent','stratum','forecast_height_band_m')
METRICS = ('ev_ms','vla_deg','spin_rpm')
REGIONS = ('downward','high_vla','line_window')


def read_csv(path):
    with path.open(newline='') as f:
        reader = csv.DictReader(f)
        if not reader.fieldnames or len(set(reader.fieldnames)) != len(reader.fieldnames):
            raise ValueError('missing_or_duplicate_csv_header:'+path.name)
        rows = list(reader)
    if any(None in r or None in r.values() for r in rows):
        raise ValueError('ragged_csv:'+path.name)
    return rows


def audit_table(path, expected, keys, checks):
    actual = read_csv(path)
    if len(actual) != len(expected):
        raise ValueError('cached_table_row_count_mismatch:'+path.name)
    indexed = {tuple(r[k] for k in keys):r for r in actual}
    if len(indexed) != len(actual):
        raise ValueError('duplicate_cached_table_key:'+path.name)
    for row in expected:
        key = tuple(str(row[k]) for k in keys)
        if key not in indexed or set(row) != set(indexed[key]):
            raise ValueError('cached_table_schema_or_key_mismatch:'+path.name)
        saved = indexed[key]; converted = {}
        for field,value in row.items():
            raw = saved[field]
            if value is None:
                if raw != '':
                    raise ValueError('cached_undefined_mismatch:'+path.name+':'+field)
                converted[field] = None
            elif isinstance(value,bool):
                if raw not in ('True','False'):
                    raise ValueError('invalid_cached_boolean')
                converted[field] = raw == 'True'
            elif isinstance(value,(int,float)):
                converted[field] = float(raw)
            else:
                converted[field] = raw
        same(row,converted,checks,'cache:'+path.name)


def number(raw, name, optional=False):
    if optional and raw == '':
        return None
    value = float(raw)
    if not math.isfinite(value):
        raise ValueError('nonfinite_cached_number:'+name)
    return value


def parse_row(raw):
    row = dict(raw)
    for field in ('game_id','count_seq','search_iterations'):
        row[field] = int(raw[field])
    if row['count_seq'] < 1 or row['search_iterations'] < 0:
        raise ValueError('invalid_cached_pitch_key_or_iterations')
    for field in ('selected',*collision.EVENTS):
        if raw[field] not in ('True','False'):
            raise ValueError('invalid_cached_boolean:'+field)
        row[field] = raw[field] == 'True'
    for field in ('aim_z_offset_m','forecast_height_m','forecast_time_sec'):
        row[field] = number(raw[field],field)
    for field in ('time_offset_sec','source_z_m','applied_e','ev_ms','hla_deg','vla_deg','spin_rpm'):
        row[field] = number(raw[field],field,True)
    if row['intent'] not in ('contact_first','line_drive'):
        raise ValueError('unsupported_cached_intent')
    decision = decide(row['strategy'],row['preferred_intent'])
    if row['selected'] != (row['intent']==decision['selected_intent']) or row['decision_reason'] != decision['reason']:
        raise ValueError('cached_decision_mismatch')
    if row['forecast_height_band_m'] != band(row['forecast_height_m'],(.6,.9,1.2)):
        raise ValueError('cached_forecast_height_band_mismatch')
    aim = -.015 if row['intent']=='line_drive' and row['forecast_height_m']>=.6 else 0.
    if abs(row['aim_z_offset_m']-aim)>1e-10:
        raise ValueError('cached_frozen_aim_mismatch')
    if row['forecast_time_sec']<=0:
        raise ValueError('invalid_cached_forecast_clock')
    status = row['status']; response = row['collision_status']
    allowed = collision.SEARCHED | {'execution_starts_before_plan','execution_starts_before_release',
        'invalid_plan_available_clock','unsupported_residual_execution_delay','invalid_contact_search_input'}
    if status not in allowed:
        raise ValueError('unsupported_cached_search_status')
    if (row['search_iterations']>0) != (status in collision.SEARCHED):
        raise ValueError('cached_search_iteration_status_mismatch')
    if status != 'contact' and response != 'not_evaluated:'+status:
        raise ValueError('cached_search_response_mismatch')
    if status == 'contact':
        if row['time_offset_sec'] is None or row['contact_region'] not in ('side','cap'):
            raise ValueError('missing_cached_contact_identity')
        if not -.06<=row['time_offset_sec']<=.06:
            raise ValueError('cached_contact_time_outside_search')
    elif row['time_offset_sec'] is not None or row['contact_region'] != '':
        raise ValueError('noncontact_has_contact_identity')
    if response == 'impulse_contact':
        if row['contact_region']!='side' or row['source_z_m'] is None or row['applied_e'] is None:
            raise ValueError('invalid_cached_response_eligibility')
        e = profile.profile_e(row['source_z_m'])
        if e is None or abs(e-row['applied_e'])>1e-10:
            raise ValueError('cached_restitution_mismatch')
        if any(row[k] is None for k in ('ev_ms','vla_deg','spin_rpm')) or row['ev_ms']<0 or row['spin_rpm']<0:
            raise ValueError('invalid_cached_response_metrics')
        if not -90<=row['vla_deg']<=90 or (row['hla_deg'] is not None and not -180<=row['hla_deg']<=180):
            raise ValueError('invalid_cached_response_angles')
    else:
        if status=='contact' and response not in {'non_approaching','not_evaluated:non_approaching',
            'not_evaluated:missing_normal','not_evaluated:unsupported_cap','not_evaluated:out_of_curve',
            'not_evaluated:invalid_pitch_spin','not_evaluated:tensile_normal_target','not_evaluated:energy_increasing_target'}:
            raise ValueError('unsupported_cached_collision_status')
        if any(row[k] is not None for k in ('ev_ms','hla_deg','vla_deg','spin_rpm')):
            raise ValueError('unavailable_response_has_metrics')
    expected = collision.flags(row,dict(collision_status=response,**{k:row[k] for k in ('hla_deg','vla_deg')}))
    if any(row[k]!=v for k,v in expected.items()):
        raise ValueError('cached_outcome_flag_mismatch')
    return row


def load_cache(folder):
    folder = Path(folder); prefix = collision.PREFIX
    if (folder/(prefix+'_metadata.json')).exists() and (folder/'rust_collision_response_metadata.json').exists():
        raise ValueError('ambiguous_collision_cache_kind')
    native=not (folder/(prefix+'_metadata.json')).exists() and (folder/'rust_collision_response_metadata.json').exists()
    if native:prefix='rust_collision_response'
    paths = [folder/(prefix+'_'+suffix) for suffix in ('metadata.json','details.csv','summary.csv','pairs.csv','status.csv','coverage.csv','checks.csv')]
    meta = json.loads(paths[0].read_text()); checks = defaultdict(float)
    if native and meta.get('cache_kind')!='rust_response_parity_v1':raise ValueError('unsupported_rust_response_cache')
    if meta['version']!=1 or meta['model']!=collision.MODEL:
        raise ValueError('unsupported_cached_collision_model')
    same(dict(axial_inertia_kg_m2=collision.AXIAL_INERTIA,mu=collision.MU,et=collision.ET,bat=profile.metadata()),
         meta['model_parameters'],checks,'frozen_model')
    # Paths in prior metadata belong to the source machine; identify inputs by basename.
    for name in ('pre_swing_aim_seed2_frozen.json','finite_barrel_collision.py','bat_vibration_profile.py',
                 'rust_intent_collision_probe.py','contact_spin.py'):
        hashes = [v for k,v in meta['inputs'].items() if Path(k).name==name]
        if hashes != [digest(Path(__file__).with_name(name))]:
            raise ValueError('frozen_source_hash_mismatch:'+name)
    source_checks = read_csv(paths[-1])
    if not source_checks:
        raise ValueError('missing_source_checks')
    for r in source_checks:
        error = number(r['max_error'],r['metric'])
        if error<0 or error>1e-7:
            raise ValueError('failed_source_check:'+r['metric'])
        checks['cached_source:'+r['metric']]=error
    overall = collision.Aggregate(); games = defaultdict(collision.Aggregate); statuses = Counter()
    coverage = Counter(); previous = None; pending = []
    def add_pair(rows):
        if len(rows)!=2 or {r['intent'] for r in rows}!={'contact_first','line_drive'}:
            raise ValueError('missing_or_duplicate_cached_intent_pair')
        a,b = rows
        for field in ('game_id','count_seq','strategy','preferred_intent','forecast_height_band_m','forecast_height_m','forecast_time_sec'):
            if a[field]!=b[field]:
                raise ValueError('cached_pair_input_mismatch:'+field)
        if a['forecast_height_m']<.6:
            fields=('status','search_iterations','time_offset_sec','contact_region','collision_status',
                    'source_z_m','applied_e','ev_ms','hla_deg','vla_deg','spin_rpm',*collision.EVENTS)
            same({k:a[k] for k in fields},{k:b[k] for k in fields},checks,'unchanged_low_height_branch')
        if not meta['first_game_id']<=a['game_id']<=meta['last_game_id']:
            raise ValueError('cached_pitch_outside_metadata_range')
        for r in rows:
            coverage['matched_plans'] += 1
            coverage['search_status:'+r['status']] += 1
            coverage['collision_status:'+r['collision_status']] += 1
            coverage['replayed_searches'] += r['status'] in collision.SEARCHED
            coverage['response_events'] += r['collision_status'] in {'impulse_contact','non_approaching',
                'not_evaluated:tensile_normal_target','not_evaluated:energy_increasing_target'}
            statuses[r['intent'],r['selected'],r['collision_status']] += 1
        coverage['spin_lookup_pitches'] += any(r['source_z_m'] is not None for r in rows)
        coverage['invalid_spin_pitches'] += any(r['collision_status']=='not_evaluated:invalid_pitch_spin' for r in rows)
        coverage['matched_states'] += 1
        overall.add(rows); games[a['game_id']].add(rows)
    with paths[1].open(newline='') as f:
        reader = csv.DictReader(f)
        if not reader.fieldnames or len(reader.fieldnames)!=len(set(reader.fieldnames)):
            raise ValueError('invalid_cached_detail_header')
        for raw in reader:
            if None in raw or None in raw.values():
                raise ValueError('ragged_cached_details')
            row = parse_row(raw); key = row['game_id'],row['count_seq']
            if previous is not None and key<previous:
                raise ValueError('cached_pitch_order_mismatch')
            if previous is not None and key!=previous:
                add_pair(pending); pending=[]
            pending.append(row); previous=key
        if pending:
            add_pair(pending)
    if not games:
        raise ValueError('empty_cached_pairs')
    coverage['solver_calls']=2*coverage['response_events']
    for k,v in coverage.items():
        source_key={'solver_calls':'rust_solver_calls','spin_lookup_pitches':'response_input_pitches',
                    'invalid_spin_pitches':'invalid_response_spin_pitches'}.get(k,k) if native else k
        if native and k.startswith('collision_status:'):source_key='status:'+k.removeprefix('collision_status:')
        if native and k.startswith('search_status:'):
            # Native metadata separately counts response statuses. Recheck geometric
            # search counts through the already audited summaries rather than aliasing them.
            continue
        if meta['coverage'].get(source_key,0)!=v:
            raise ValueError('cached_coverage_mismatch:'+k)
    if coverage['matched_plans']!=meta['coverage']['matched_plans'] or coverage['matched_states']!=meta['coverage']['matched_states']:
        raise ValueError('incomplete_cached_details')
    audit_table(paths[2],overall.summaries(),('scope','intent','strategy','preferred_intent','stratum','forecast_height_band_m'),checks)
    audit_table(paths[3],overall.paired_rows(),('strategy','preferred_intent','stratum','forecast_height_band_m'),checks)
    status_rows = [dict(intent=k[0],selected=k[1],collision_status=k[2],plans=n) for k,n in sorted(statuses.items())]
    audit_table(paths[4],status_rows,('intent','selected','collision_status'),checks)
    audit_table(paths[5],[dict(metric=k,count=v) for k,v in sorted(meta['coverage'].items())],('metric',),checks)
    if native and meta.get('holdout_manifest') is not None:
        manifest=meta['holdout_manifest'];records=manifest['games'];ids={r['game_id'] for r in records}
        if (manifest['status']!='complete' or len(ids)!=len(records) or set(meta['selected_game_ids'])!=ids
            or meta['provenance_games_verified']!=len(ids) or not set(games)<=ids):
            raise ValueError('invalid_bootstrap_holdout_game_population')
        # Explicitly recorded zero-contribution games are real clusters, unlike
        # missing IDs guessed from a numeric range. Preserve them in the draws.
        for game in ids:games.setdefault(game,collision.Aggregate())
    return meta,overall,games,checks,paths


def game_vectors(overall,games):
    keys = sorted(overall.pairs)
    vectors = []
    for game in sorted(games):
        aggregate = games[game]; blocks=[]
        for key in keys:
            strategy,preference,stratum,height = key
            sides = [aggregate.groups.get(('all_plans',intent,strategy,preference,stratum,height),Counter())
                     for intent in ('contact_first','line_drive')]
            pair = aggregate.pairs.get(key,Counter())
            blocks.append(tuple([g[k] for g in sides for k in SIDE_FIELDS]+[pair[k] for k in PAIR_FIELDS]))
        vectors.append(blocks)
    return keys,vectors


def metrics(v):
    n = len(SIDE_FIELDS)
    a,b = (dict(zip(SIDE_FIELDS,side)) for side in (v[:n],v[n:2*n]))
    pair = dict(zip(PAIR_FIELDS,v[2*n:])); values = {}
    def difference(x,y,scale=1.):
        return scale*(y-x) if x is not None and y is not None else None
    for event in collision.EVENTS:
        values[event+'_pct_per_searched_delta_pp'] = difference(collision.ratio(a[event],a['searched']),collision.ratio(b[event],b['searched']),100.)
    for region in REGIONS:
        values[region+'_pct_among_fair_delta_pp'] = difference(collision.ratio(a[region],a['fair']),collision.ratio(b[region],b['fair']),100.)
        values['common_fair_'+region+'_delta_pp'] = collision.ratio(100*pair['common_fair_'+region+'_delta'],pair['common_fair'])
    for metric in METRICS:
        values['mean_fair_'+metric+'_delta'] = difference(collision.ratio(a['fair_sum_'+metric],a['fair_n_'+metric]),
            collision.ratio(b['fair_sum_'+metric],b['fair_n_'+metric]))
        values['common_fair_mean_delta_'+metric] = collision.ratio(pair['common_fair_delta_'+metric],pair['common_fair'])
    return values


def totals(vectors,weights):
    result = [[0.]*SIZE for _ in vectors[0]]
    for index,count in weights.items():
        for total,vector in zip(result,vectors[index]):
            for j,value in enumerate(vector):
                total[j] += count*value
    return result


def bootstrap(vectors,replicates,seed):
    rng = random.Random(seed); n=len(vectors)
    for _ in range(replicates):
        # One whole-game draw is shared by both intents, all strategies and heights.
        weights = Counter(rng.randrange(n) for _ in range(n))
        yield [metrics(v) for v in totals(vectors,weights)]


def audit_point(keys,point,overall,checks):
    summaries={(r['intent'],*(r[k] for k in GROUP_FIELDS)):r for r in overall.summaries() if r['scope']=='all_plans'}
    pairs={tuple(r[k] for k in GROUP_FIELDS):r for r in overall.paired_rows()}
    def delta(a,b):
        return b-a if a is not None and b is not None else None
    for key,values in zip(keys,point):
        a,b=(summaries[(intent,*key)] for intent in ('contact_first','line_drive'))
        pair=pairs[key];expected={}
        for event in collision.EVENTS:
            k=event+'_pct_per_searched'
            expected[k+'_delta_pp']=delta(a[k],b[k])
        for region in REGIONS:
            k=region+'_pct_among_fair'
            expected[k+'_delta_pp']=delta(a[k],b[k])
            k='common_fair_'+region+'_delta_pp';expected[k]=pair[k]
        for metric in METRICS:
            k='mean_fair_'+metric;expected[k+'_delta']=delta(a[k],b[k])
            k='common_fair_mean_delta_'+metric;expected[k]=pair[k]
        same(expected,values,checks,'game_block_point')


def run(folder,output,replicates=2000,seed='0',confidence=.95):
    if type(replicates) is not int or replicates<1 or not math.isfinite(confidence) or not 0<confidence<1:
        raise ValueError('invalid_bootstrap_parameters')
    output=Path(output)
    if output.exists():
        raise FileExistsError(output)
    meta,overall,games,checks,paths = load_cache(folder)
    if len(games)<2:
        raise ValueError('requires_at_least_two_game_clusters')
    keys,vectors = game_vectors(overall,games)
    point = [metrics(v) for v in totals(vectors,Counter({i:1 for i in range(len(vectors))}))]
    audit_point(keys,point,overall,checks)
    distributions = [defaultdict(list) for _ in keys]
    for sample in bootstrap(vectors,replicates,seed):
        for group,values in zip(distributions,sample):
            for metric,value in values.items():
                if value is not None:
                    group[metric].append(value)
    summary=[];alpha=(1-confidence)/2
    for i,(key,values) in enumerate(zip(keys,point)):
        g = overall.groups[('all_plans','contact_first',*key)]
        h = overall.groups[('all_plans','line_drive',*key)]
        contributing_games = sum(any(x!=0 for x in vector[i]) for vector in vectors)
        for metric,value in values.items():
            samples = sorted(distributions[i][metric]); valid=len(samples)
            # A partially undefined distribution is reported but receives no interval.
            # This avoids presenting a conditional-on-defined interval as a full CI.
            complete = valid==replicates
            low = quantile(samples,alpha) if complete else None
            high = quantile(samples,1-alpha) if complete else None
            status = 'point_undefined' if value is None else 'undefined_replicates' if not complete else 'interval_available'
            summary.append(dict(zip(GROUP_FIELDS,key),metric=metric,estimate=value,
                ci_low=low,ci_high=high,confidence=confidence,valid_replicates=valid,undefined_replicates=replicates-valid,
                interval_status=status,interval_relation=('positive' if low>0 else 'negative' if high<0 else 'includes_zero') if complete else 'unavailable',
                sampled_games=len(games),contributing_games=contributing_games,pitches=g['plans'],
                reference_searched=g['searched'],candidate_searched=h['searched'],reference_fair=g['fair'],candidate_fair=h['fair'],
                common_fair=overall.pairs[key]['common_fair']))
    coverage=dict(observed_games=len(games),comparable_games=sum(bool(g.pairs) for g in games.values()),matched_pitches=meta['coverage']['matched_states'],
        matched_plans=meta['coverage']['matched_plans'],groups=len(keys),replicates=replicates,
        interval_available=sum(r['interval_status']=='interval_available' for r in summary),
        intervals_unavailable=sum(r['interval_status']!='interval_available' for r in summary))
    metadata=dict(version=1,status='paired_whole_game_percentile_bootstrap',reference_intent='contact_first',candidate_intent='line_drive',
        source_metadata=meta,coverage=coverage,seed=str(seed),confidence=confidence,replicates=replicates,
        resampling='N observed game IDs drawn uniformly with replacement N times; shared multiplicities across both intents and all strata; ratio of summed counts, not mean game rates',
        interval='two-sided percentile with linear interpolation; any undefined replicate suppresses the interval',
        inputs={str(p):digest(p) for p in paths},code_sha256=digest(Path(__file__)),db_reads=0,new_searches=0,collision_replays=0,
        limitations=[
            'Cached details are reaggregated and checked against summaries/pairs/status/coverage plus frozen model hashes. No contact or collision physics is reexecuted.',
            'A verified explicit holdout manifest retains all generated games, including zero-contribution clusters. Other caches use observed comparable games; numeric ID gaps are never invented.',
            'All diagnostic alternatives are paired; selected_plans is audited but is not a separate intervention or bootstrap population.',
            'Resampling games accounts for within-game dependence, not repeated-player or shared-team dependence across games.',
            'Height/strategy cohorts retain shared game draws. Sparse cohorts may have undefined denominators; they are not zero-filled.',
            'Intervals are marginal, not simultaneous or multiplicity-adjusted. Direction labels are not p-values or automatic adoption decisions.',
            'No new holdout, model calibration, policy optimization, Statcast fitting, or real-world validation is performed.'
        ])
    output.mkdir(parents=True,exist_ok=False)
    write_csv(output/(PREFIX+'_summary.csv'),list(summary[0]),summary)
    write_csv(output/(PREFIX+'_coverage.csv'),['metric','count'],[dict(metric=k,count=v) for k,v in sorted(coverage.items())])
    write_csv(output/(PREFIX+'_checks.csv'),['metric','max_error'],[dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    (output/(PREFIX+'_metadata.json')).write_text(json.dumps(metadata,indent=2,allow_nan=False)+'\n')
    return dict(status=metadata['status'],**coverage)


if __name__=='__main__':
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--input-dir',type=Path,required=True)
    parser.add_argument('--output-dir',type=Path,required=True)
    parser.add_argument('--replicates',type=int,default=2000)
    parser.add_argument('--bootstrap-seed',default='0')
    parser.add_argument('--confidence',type=float,default=.95)
    args=parser.parse_args()
    try:
        print(json.dumps(run(args.input_dir,args.output_dir,args.replicates,args.bootstrap_seed,args.confidence),indent=2))
    except (OSError,ValueError,KeyError,TypeError) as exc:
        parser.exit(1,f'Intent game bootstrap failed: {exc}\n')
