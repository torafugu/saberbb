#!/usr/bin/env python3
"""Compare slower intended swing plans, rebuilding each pulse's initial pose."""
import argparse
import json
import sqlite3
from pathlib import Path

from diagnostic_db import add_db_argument
from plan_tilt_sweep import run


if __name__=='__main__':
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--input-dir',type=Path,required=True);add_db_argument(p)
    p.add_argument('--output-dir',type=Path,required=True)
    p.add_argument('--variant',default='observed_sigma_0.005_r0.9')
    p.add_argument('--speed-ratios',type=float,nargs='+',default=[.4,.6,.8,1.])
    p.add_argument('--tilts-deg',type=float,nargs='+',default=[0.,15.])
    p.add_argument('--yaws-deg',type=float,nargs='+',default=[0.])
    p.add_argument('--aim-z-mm',type=float,nargs='+',default=[0.])
    p.add_argument('--sample-size',type=int,default=1000);p.add_argument('--sample-seed',default='0')
    p.add_argument('--duration-sec',type=float,default=.18);p.add_argument('--anchor-fraction',type=float,default=.5)
    p.add_argument('--gyro-sign',type=int,choices=(-1,0,1),default=0)
    a=p.parse_args()
    try:print(json.dumps(run(a.input_dir,a.db,a.output_dir,a.variant,a.tilts_deg,a.sample_size,a.sample_seed,
        a.duration_sec,a.anchor_fraction,a.gyro_sign,yaws=a.yaws_deg,
        aim_z_offsets=tuple(x/1000 for x in a.aim_z_mm),speed_ratios=a.speed_ratios),indent=2))
    except (OSError,ValueError,KeyError,TypeError,sqlite3.Error) as exc:p.exit(1,f'Planned speed sweep failed: {exc}\n')
