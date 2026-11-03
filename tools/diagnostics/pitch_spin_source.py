"""Read-only per-pitch scalar-spin lookup and explicit 3D reconstruction scenarios."""
import hashlib
import json
import math
import sqlite3
from pathlib import Path

from contact_3d import vector
from contact_collision_probe import cross, dot
from rotating_bat_probe import add, mul, norm, sub


def reconstruct(rate_rpm,angle_deg,efficiency,velocity,gyro_sign=0):
    if (not all(math.isfinite(x) for x in (rate_rpm,angle_deg,efficiency)) or rate_rpm<0
        or not 0<=efficiency<=1 or not vector(velocity) or norm(velocity)<=0
        or gyro_sign not in (-1,0,1)):raise ValueError('invalid_pitch_spin')
    speed=norm(velocity);vhat=mul(velocity,1/speed)
    angle=math.radians(angle_deg);lift=(math.sin(angle),0.,math.cos(angle))
    projected=sub(lift,mul(vhat,dot(lift,vhat)))
    omega=rate_rpm*math.pi/30
    if efficiency>0 and norm(projected)<1e-12:raise ValueError('degenerate_pitch_spin_frame')
    transverse=mul(cross(vhat,projected),omega*efficiency/norm(projected)) if efficiency else (0.,0.,0.)
    gyro=mul(vhat,gyro_sign*omega*math.sqrt(max(0.,1-efficiency**2)))
    return add(transverse,gyro)


class PitchSpinSource:
    def __init__(self,path,gyro_sign=0):
        if gyro_sign not in (-1,0,1):raise ValueError('invalid_gyro_sign')
        self.path=Path(path);self.gyro_sign=gyro_sign;self.cache={};self.db=None
        try:
            self.db=sqlite3.connect(self.path.resolve().as_uri()+'?mode=ro',uri=True)
            self.db.execute('PRAGMA query_only=ON');self.db.execute('BEGIN')
            fields={r[1] for r in self.db.execute('PRAGMA table_info(player_game_pitching)')}
            if not {'game_id','count_seq','spin_rate','spin_angle','spin_efficiency'}<=fields:
                raise ValueError('missing_pitch_spin_columns')
        except BaseException:
            self.close();raise

    def get(self,r):
        key=(r['game_id'],r['count_seq'])
        if key not in self.cache:
            rows=self.db.execute('SELECT spin_rate,spin_angle,spin_efficiency FROM player_game_pitching WHERE game_id=? AND count_seq=?',key).fetchall()
            if len(rows)!=1:raise ValueError(f'pitch_spin_key_missing_or_duplicate:{key}')
            try:values=tuple(float(v) for v in rows[0])
            except (ValueError,TypeError) as exc:raise ValueError(f'invalid_pitch_spin_row:{key}') from exc
            self.cache[key]=values
        velocity=tuple(float(r['ball_velocity_'+a+'_ms']) for a in 'xyz')
        return reconstruct(*self.cache[key],velocity,self.gyro_sign)

    def metadata(self):
        serialized=json.dumps([dict(game_id=k[0],count_seq=k[1],rate_rpm=v[0],angle_deg=v[1],efficiency=v[2])
            for k,v in sorted(self.cache.items())],sort_keys=True,separators=(',',':'),allow_nan=False)
        return dict(source='read_only_player_game_pitching_scalar_spin_reconstruction',unique_pitches=len(self.cache),
            used_scalar_rows_sha256=hashlib.sha256(serialized.encode()).hexdigest(),gyro_sign=self.gyro_sign,
            assumptions=[
                'Scalar spin values are saved simulation data, not a measured world angular-velocity vector.',
                'Project the pitching Magnus direction (sin(angle),0,cos(angle)) perpendicular to saved incoming velocity; transverse omega is vhat cross projected_lift.',
                'Transverse magnitude = spin_rate * efficiency; gyro magnitude = spin_rate * sqrt(1-efficiency^2).',
                'gyro_sign=0 omits unknown gyro; +/-1 align/oppose the velocity direction. Signs are sensitivity assumptions, never inferred from efficiency.',
                'No spin decay from release to contact is modeled. Lookup uses an immutable SQLite read snapshot.',
                'The hash covers the scalar rows actually used; it does not claim to hash a live DB/WAL file.'])

    def close(self):
        if self.db is not None:self.db.close();self.db=None
