"""Read-only per-pitch scalar-spin lookup and explicit 3D reconstruction scenarios."""
import csv
import hashlib
import json
import math
import sqlite3
from pathlib import Path

from contact_3d import vector
from contact_collision_probe import cross, dot
from rotating_bat_probe import add, mul, norm, sub


class InvalidPitchSpin(ValueError):
    def __init__(self,reason,details=None):
        self.reason=reason;self.details=details or {}
        super().__init__(reason+(': '+', '.join(f'{k}={v!r}' for k,v in self.details.items()) if self.details else ''))


def reconstruct(rate_rpm,angle_deg,efficiency,velocity,gyro_sign=0):
    values=dict(spin_rate=rate_rpm,spin_angle=angle_deg,spin_efficiency=efficiency)
    for name,value in values.items():
        if not isinstance(value,(int,float)) or not math.isfinite(value):
            raise InvalidPitchSpin('non_finite_'+name,values)
    if rate_rpm<0:raise InvalidPitchSpin('negative_spin_rate',values)
    if not 0<=efficiency<=1:raise InvalidPitchSpin('spin_efficiency_out_of_range',values)
    if not vector(velocity) or norm(velocity)<=0:raise InvalidPitchSpin('invalid_incoming_velocity',dict(velocity=velocity))
    if gyro_sign not in (-1,0,1):raise ValueError('invalid_gyro_sign')
    speed=norm(velocity);vhat=mul(velocity,1/speed)
    angle=math.radians(angle_deg);lift=(math.sin(angle),0.,math.cos(angle))
    projected=sub(lift,mul(vhat,dot(lift,vhat)))
    omega=rate_rpm*math.pi/30
    if efficiency>0 and norm(projected)<1e-12:raise InvalidPitchSpin('degenerate_pitch_spin_frame',dict(velocity=velocity,**values))
    transverse=mul(cross(vhat,projected),omega*efficiency/norm(projected)) if efficiency else (0.,0.,0.)
    gyro=mul(vhat,gyro_sign*omega*math.sqrt(max(0.,1-efficiency**2)))
    return add(transverse,gyro)


class PitchSpinSource:
    def __init__(self,path,gyro_sign=0,invalid_report=None):
        if gyro_sign not in (-1,0,1):raise ValueError('invalid_gyro_sign')
        self.path=Path(path);self.gyro_sign=gyro_sign;self.cache={};self.db=None
        self.valid_keys=set();self.reported=set();self.report=None;self.report_path=None;self.writer=None
        try:
            self.db=sqlite3.connect(self.path.resolve().as_uri()+'?mode=ro',uri=True)
            self.db.execute('PRAGMA query_only=ON');self.db.execute('BEGIN')
            fields={r[1] for r in self.db.execute('PRAGMA table_info(player_game_pitching)')}
            if not {'game_id','count_seq','spin_rate','spin_angle','spin_efficiency'}<=fields:
                raise ValueError('missing_pitch_spin_columns')
            if invalid_report is not None:
                self.report_path=Path(invalid_report);self.report=self.report_path.open('x',newline='')
                self.writer=csv.DictWriter(self.report,fieldnames=['game_id','count_seq','first_variant','reason',
                    'spin_rate','spin_angle','spin_efficiency','ball_velocity_x_ms','ball_velocity_y_ms','ball_velocity_z_ms','gyro_sign'])
                self.writer.writeheader();self.report.flush()
        except BaseException:
            self.close();raise

    def invalid(self,r,raw,reason):
        key=(r['game_id'],r['count_seq'])
        details=dict(game_id=key[0],count_seq=key[1],spin_rate=raw[0],spin_angle=raw[1],spin_efficiency=raw[2])
        report_key=key+(reason,)
        if report_key not in self.reported:
            self.reported.add(report_key)
            if self.writer is not None:
                self.writer.writerow(dict(details,first_variant=r.get('variant',''),reason=reason,
                    **{'ball_velocity_'+a+'_ms':r.get('ball_velocity_'+a+'_ms') for a in 'xyz'},gyro_sign=self.gyro_sign))
                self.report.flush()
        return InvalidPitchSpin(reason,details)

    def get(self,r):
        key=(r['game_id'],r['count_seq'])
        if key not in self.cache:
            rows=self.db.execute('SELECT spin_rate,spin_angle,spin_efficiency FROM player_game_pitching WHERE game_id=? AND count_seq=?',key).fetchall()
            if len(rows)!=1:raise ValueError(f'pitch_spin_key_missing_or_duplicate:{key}')
            self.cache[key]=rows[0]
        raw=self.cache[key]
        try:values=tuple(float(v) for v in raw)
        except (ValueError,TypeError) as exc:raise self.invalid(r,raw,'non_numeric_pitch_spin_scalar') from exc
        velocity=tuple(float(r['ball_velocity_'+a+'_ms']) for a in 'xyz')
        try:result=reconstruct(*values,velocity,self.gyro_sign)
        except InvalidPitchSpin as exc:raise self.invalid(r,raw,exc.reason) from exc
        self.valid_keys.add(key)
        return result

    def metadata(self):
        serialized=json.dumps([dict(game_id=k[0],count_seq=k[1],rate_rpm=v[0],angle_deg=v[1],efficiency=v[2])
            for k,v in sorted(self.cache.items()) if k in self.valid_keys],sort_keys=True,separators=(',',':'),allow_nan=False)
        return dict(source='read_only_player_game_pitching_scalar_spin_reconstruction',unique_pitches=len(self.valid_keys),
            invalid_unique_pitches=len({k[:2] for k in self.reported}),invalid_reason_rows=len(self.reported),
            invalid_report_sha256=hashlib.sha256(self.report_path.read_bytes()).hexdigest() if self.report_path else None,
            used_scalar_rows_sha256=hashlib.sha256(serialized.encode()).hexdigest(),gyro_sign=self.gyro_sign,
            assumptions=[
                'Scalar spin values are saved simulation data, not a measured world angular-velocity vector.',
                'Project the pitching Magnus direction (sin(angle),0,cos(angle)) perpendicular to saved incoming velocity; transverse omega is vhat cross projected_lift.',
                'Transverse magnitude = spin_rate * efficiency; gyro magnitude = spin_rate * sqrt(1-efficiency^2).',
                'gyro_sign=0 omits unknown gyro; +/-1 align/oppose the velocity direction. Signs are sensitivity assumptions, never inferred from efficiency.',
                'No spin decay from release to contact is modeled. Lookup uses an immutable SQLite read snapshot.',
                'The hash covers the scalar rows actually used; it does not claim to hash a live DB/WAL file.'])

    def close(self):
        if self.report is not None:self.report.close();self.report=None
        if self.db is not None:self.db.close();self.db=None
