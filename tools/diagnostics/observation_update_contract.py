"""Python reference for Rust sensor-noise and three-position forecast parity."""
import argparse
import json
from pathlib import Path
from observation_update_probe import standard_noise, observe, estimate


def cases():
    history = [dict(time_sec=t, position_world_m=[.1 + .2*t + .2*t*t, 16-40*t, 1.8+t-4.5*t*t]) for t in (.08, .16, .24)]
    result = []
    for game, seq, eye, sigma in [(0, 0, 0., .005), (25804, 22, -2., .005), (25804, 22, 2., .005),
                                   (26163, 65535, 0., .005), (4294967295, 1, 6., .005), (10, 2, -6., .005), (1, 1, 0., 0.)]:
        noise = standard_noise(game, seq)
        observations = observe(history, sigma, eye, noise)
        f, v, a = estimate(observations)
        result.append(dict(game_id=game, count_seq=seq, eye=eye, sigma=sigma, history=history, noise=noise,
                           expected_estimate=dict(observations=observations, velocity=v, acceleration=a, forecast=f)))
    return dict(version=1, cases=result)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    records = ',\n'.join(json.dumps(c, separators=(',', ':'), allow_nan=False) for c in cases()['cases'])
    args.output.write_text('{"version":1,"cases":[\n' + records + '\n]}\n')
