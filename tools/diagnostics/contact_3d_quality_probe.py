#!/usr/bin/env python3
"""Audit and stratify existing rotating-contact CSVs without a database or game replay."""
import argparse
import csv
import json
import math
from collections import Counter, defaultdict
from pathlib import Path

from contact_3d import CONTRACT_VERSION, ContactGeometry, QualityAccumulator, solve_contact
from contact_state_probe import SOURCE
from contact_collision_probe import write_csv


def number(row,key):
    value=float(row[key])
    if not math.isfinite(value):raise ValueError('non_finite:'+key)
    return value


def read_vector(row,prefix):
    return tuple(number(row,prefix+axis+'_ms') for axis in 'xyz')


def audit_contact(row,metadata):
    n=tuple(number(row,'normal_'+axis) for axis in 'xyz')
    axis=tuple(number(row,'axis_at_entry_'+axis) for axis in 'xyz')
    bat=read_vector(row,'point_velocity_')
    axial=number(row,'axial_at_entry_m');half=number(row,'half_length_m')
    closest=(number(row,'closest_axial_at_entry_m') if row.get('closest_axial_at_entry_m') else
             max(-half,min(half,axial)))
    geometry=ContactGeometry(number(row,'entry_since_release_sec'),axis,n,axial,closest,half,row['contact_region'])
    if all(row.get('ball_velocity_'+a+'_ms') not in (None,'') for a in 'xyz'):
        incoming=read_vector(row,'ball_velocity_');source='saved_incoming_vector'
    else:
        if metadata.get('probe_version')==2:
            raise ValueError('v2_missing_incoming_vector')
        # v1 impulse-contact CSVs store only outgoing angles/speed and delta*n.
        # Reconstruct the incoming vector; this is a consistency audit, not independent velocity evidence.
        ev=number(row,'ev_ms');hla=math.radians(number(row,'hla_deg'));vla=math.radians(number(row,'vla_deg'))
        outgoing=(ev*math.cos(vla)*math.sin(hla),ev*math.cos(vla)*math.cos(hla),ev*math.sin(vla))
        delta=number(row,'impulse_delta_ms')
        if delta<=0:raise ValueError('v1_missing_incoming_for_non_approaching')
        incoming=tuple(v-delta*a for v,a in zip(outgoing,n));source='reconstructed_v1_incoming'
    metrics=solve_contact(geometry,incoming,bat,metadata['restitution'],metadata['effective_mass_kg'])
    checks={}
    for field in ('relative_normal_ms','impulse_delta_ms','ev_ms','hla_deg','vla_deg'):
        if row.get(field) not in (None,'') and metrics[field] is not None:
            diff=metrics[field]-number(row,field)
            if field=='hla_deg':diff=(diff+180)%360-180
            checks[field+'_reconstruction_error']=abs(diff)
    if all(row.get('outgoing_velocity_'+a+'_ms') not in (None,'') for a in 'xyz') and metrics['outgoing_world_ms'] is not None:
        checks['outgoing_vector_reconstruction_error_ms']=max(abs(a-b) for a,b in zip(
            metrics['outgoing_world_ms'],read_vector(row,'outgoing_velocity_')))
    for key in ('restitution_residual_ms','tangential_impulse_residual_ms'):
        if metrics[key] is not None:checks[key]=metrics[key]
    return metrics,source,checks


def run(contacts,metadata_path,output):
    metadata=json.loads(Path(metadata_path).read_text())
    if metadata.get('probe_version') not in (1,2) or metadata.get('state_source')!=SOURCE:
        raise ValueError('Unsupported rotating-contact metadata')
    if metadata['probe_version']==2 and metadata.get('contact_contract_version')!=CONTRACT_VERSION:
        raise ValueError('Unsupported contact contract version')
    if (not all(isinstance(metadata.get(k),(int,float)) and math.isfinite(metadata[k])
                for k in ('restitution','effective_mass_kg','half_length_m','effective_radius_m')) or
        not 0<=metadata['restitution']<=1 or metadata['effective_mass_kg']<=0 or metadata['half_length_m']<=0 or metadata['effective_radius_m']<=0):
        raise ValueError('Invalid collision metadata')
    output=Path(output);output.mkdir(parents=True,exist_ok=False)
    quality=QualityAccumulator();coverage=Counter();checks=defaultdict(float);exclusions=[];seen=set()
    with Path(contacts).open(newline='',encoding='utf-8') as stream:
        for row in csv.DictReader(stream):
            key=(row['game_id'],row['count_seq'],row['variant'])
            if key in seen:raise ValueError('Duplicate game/count/variant key')
            seen.add(key);coverage['rows']+=1
            if row['status']!='contact':
                quality.add(row,None);coverage['not_contact_rows']+=1;continue
            try:
                if metadata['probe_version']==2 and number(row,'contact_contract_version')!=CONTRACT_VERSION:
                    raise ValueError('unsupported_row_contact_contract')
                if (abs(number(row,'half_length_m')-metadata['half_length_m'])>1e-12 or
                    abs(number(row,'effective_radius_m')-metadata['effective_radius_m'])>1e-12):
                    raise ValueError('shape_metadata_mismatch')
                metrics,source,errors=audit_contact(row,metadata)
                for field,value in errors.items():checks[field]=max(checks[field],value)
                if max(errors.values(),default=0)>1e-7:
                    raise ValueError('reconstruction_mismatch')
            except (ValueError,TypeError,KeyError,OverflowError) as exc:
                coverage['invalid_contact_rows']+=1
                reason=str(exc)
                quality.add(row,{'collision_status':'invalid_contact'})
                exclusions.append(dict(game_id=key[0],count_seq=key[1],variant=key[2],reason=reason))
                continue
            coverage[source]+=1;coverage[metrics['collision_status']]+=1
            quality.add(row,metrics)
    quality.write(output)
    fields=['rows','not_contact_rows','invalid_contact_rows','saved_incoming_vector',
            'reconstructed_v1_incoming','impulse_contact','non_approaching']
    result={k:coverage[k] for k in fields}
    write_csv(output/'contact_3d_coverage.csv',fields,[result])
    write_csv(output/'contact_3d_checks.csv',['metric','max_error'],
        [dict(metric=k,max_error=v) for k,v in sorted(checks.items())])
    write_csv(output/'contact_3d_exclusions.csv',['game_id','count_seq','variant','reason'],exclusions)
    (output/'contact_3d_metadata.json').write_text(json.dumps(dict(contact_contract_version=CONTRACT_VERSION,
        source_probe_version=metadata['probe_version'],audit_tolerance=1e-7,restitution=metadata['restitution'],
        effective_mass_kg=metadata['effective_mass_kg'],
        note='v1 incoming velocity is reconstructed from trial outgoing velocity and impulse, not measured'),indent=2)+'\n')
    return result


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--contacts-csv',type=Path,required=True)
    p.add_argument('--metadata-json',type=Path,required=True)
    p.add_argument('--output-dir',type=Path,required=True)
    args=p.parse_args()
    try:print(run(args.contacts_csv,args.metadata_json,args.output_dir))
    except (OSError,ValueError,KeyError,csv.Error) as exc:p.exit(1,f'Diagnostic failed: {exc}\n')
