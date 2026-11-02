"""Read-only, audited observation-update contact rows for collision counterfactuals."""
import csv
import json
import sqlite3
import tempfile
from pathlib import Path

from contact_point_speed_probe import digest
from forecast_contact_geometry import REQUIRED, audit


class ContactSource:
    def __init__(self, input_dir):
        self.paths={n:Path(input_dir)/n for n in (
            'observation_update_contacts.csv','observation_update_metadata.json','observation_update_coverage.csv')}
        self.hashes={n:digest(p) for n,p in self.paths.items()}
        self.meta=json.loads(self.paths['observation_update_metadata.json'].read_text())
        meta=self.meta
        if meta.get('version')!=1 or meta.get('restitution')!=.5 or meta.get('effective_mass_kg')!=.6:
            raise ValueError('unsupported_source_metadata')
        with self.paths['observation_update_coverage.csv'].open(newline='') as f:coverage=list(csv.DictReader(f))
        if len(coverage)!=1:raise ValueError('invalid_source_coverage')
        self.histories=int(coverage[0]['valid_histories']);self.expected=int(coverage[0]['trial_rows'])
        self.conditions={'prior_only','exact_observation_control'}|{f'observed_sigma_{s:g}' for s in meta['sensor_sigma_m']}
        if self.expected!=self.histories*len(meta['levers_m'])*len(self.conditions):raise ValueError('invalid_source_count')

    def rows(self, checks):
        """Consume to exhaustion: final coverage/uniqueness checks run after the last row."""
        count=0;meta=self.meta
        with tempfile.TemporaryDirectory(prefix='saberbb_contact_keys_') as temp:
            db=sqlite3.connect(str(Path(temp)/'keys.db'))
            try:
                db.execute('CREATE TABLE keys(game INTEGER, seq INTEGER, lever REAL, condition TEXT, PRIMARY KEY(game,seq,lever,condition))')
                with self.paths['observation_update_contacts.csv'].open(newline='') as f:
                    reader=csv.DictReader(f);required=REQUIRED+['omega_'+a+'_rad_s' for a in 'xyz']
                    if not set(required)<=set(reader.fieldnames or []) or len(reader.fieldnames)!=len(set(reader.fieldnames)):
                        raise ValueError('missing_or_duplicate_csv_columns')
                    for line,raw in enumerate(reader,2):
                        try:
                            r=audit(raw,checks)
                            if (r['forecast_condition'] not in self.conditions or r['lever_m'] not in meta['levers_m']
                                or r['half_length_m']!=meta['half_length_m'] or r['effective_radius_m']!=meta['radius_m']):
                                raise ValueError('source_geometry_mismatch')
                            db.execute('INSERT INTO keys VALUES(?,?,?,?)',(r['game_id'],r['count_seq'],r['lever_m'],r['forecast_condition']))
                            yield r
                        except (ValueError,TypeError,KeyError,sqlite3.IntegrityError) as exc:
                            raise ValueError(f'CSV line {line}: {exc}') from exc
                        count+=1
                if count!=self.expected:raise ValueError('source_coverage_mismatch')
                groups=db.execute('SELECT COUNT(*) FROM keys GROUP BY game,seq,lever').fetchall()
                if any(n!=len(self.conditions) for (n,) in groups) or len(groups)!=self.histories*len(meta['levers_m']):
                    raise ValueError('incomplete_source_keys')
            finally:db.close()
