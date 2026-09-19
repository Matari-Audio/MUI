#!/usr/bin/env python3
"""Run the native conformance binary, then an isolated Hybrid/classic matrix.

Creates a NEW output directory; never overwrites evidence. No background jobs,
no software-adapter rankings, and no inferred FPS. Real audio/presentation tests
must be run separately in a plugin host.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import platform
from pathlib import Path
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parents[2]

def run(args: list[str], log: Path | None = None) -> None:
    print('+', ' '.join(args), flush=True)
    if log:
        with log.open('w', encoding='utf8') as output:
            subprocess.run(args, cwd=ROOT, stdout=output, stderr=subprocess.STDOUT, check=True)
    else:
        subprocess.run(args, cwd=ROOT, check=True)

def output(args: list[str]) -> str:
    return subprocess.check_output(args, cwd=ROOT, text=True).strip()

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('out', type=Path)
    parser.add_argument('--frames', type=int, default=600)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--scales', type=float, nargs='+', default=[1., 1.5, 2.])
    parser.add_argument('--profile', choices=['perf', 'release'], default='perf')
    parser.add_argument('--allow-software', action='store_true', help='Correctness diagnostics only')
    args = parser.parse_args()
    if not 1 <= args.frames <= 100000 or not 1 <= args.repetitions <= 100:
        parser.error('frame/repetition count outside limits')
    if any(not .125 <= s <= 8 for s in args.scales):
        parser.error('scale outside .125..=8')
    out = args.out.absolute()
    out.mkdir(parents=True, exist_ok=False)
    diff = subprocess.check_output(['git', 'diff', 'HEAD', '--binary'], cwd=ROOT)
    meta = {'scope': 'headless analytic welding composition; NOT full-editor, DAW, power or presented-FPS benchmark',
            'platform': platform.platform(), 'python': sys.version, 'profile': args.profile,
            'rustc': output(['rustc', '--version', '--verbose']),
            'commit': output(['git', 'rev-parse', 'HEAD']),
            'status': output(['git', 'status', '--short']),
            'tracked_diff_sha256': hashlib.sha256(diff).hexdigest(),
            'frames': args.frames, 'repetitions': args.repetitions, 'scales': args.scales,
            'software_diagnostics_allowed': args.allow_software}
    (out / 'metadata.json').write_text(json.dumps(meta, indent=2) + '\n')
    (out / 'tracked-working-tree.patch').write_bytes(diff)
    common = ['cargo', 'run', '--locked', '--profile', args.profile, '-p', 'mui-vello',
              '--features', 'gpu-effects,bench-classic']
    run(common + ['--example', 'gpu_contract', '--', str(out / 'contract')], out / 'contract.log')
    # Fail before timing starts when the image-parity dependency is absent.
    __import__('PIL.Image')
    paths = []
    for repetition in range(args.repetitions):
        # Alternate first backend rather than systematically warming one first.
        order = ['hybrid', 'classic'] if repetition % 2 == 0 else ['classic', 'hybrid']
        directory = out / f'repeat-{repetition + 1}'
        directory.mkdir()
        for scale in args.scales:
            for case in ['static', 'morph', 'geometry', 'resize']:
                for backend in order:
                    tag = f'{backend}-{case}-{scale:g}'
                    command = common + ['--example', 'gpu_matrix', '--', '--backend', backend,
                        '--case', case, '--scale', f'{scale:g}', '--frames', str(args.frames),
                        '--out', str(directory)]
                    if args.allow_software:
                        command.append('--allow-software')
                    run(command, directory / f'{tag}.log')
                    paths.append(directory / f'{tag}.csv')
                    time.sleep(.25)  # explicit inter-run pause; not inside a measured sample
                run([sys.executable, str(Path(__file__).with_name('compare.py')),
                     str(directory / f'hybrid-{case}-{scale:g}.png'),
                     str(directory / f'classic-{case}-{scale:g}.png'),
                     '--out', str(directory / f'parity-{case}-{scale:g}.json')],
                    directory / f'parity-{case}-{scale:g}.log')
    run([sys.executable, str(Path(__file__).with_name('report.py')), *map(str, paths),
         '--out', str(out / 'summary.json')])
    print('Raw evidence written. Review paired images before interpreting performance differences.')

if __name__ == '__main__':
    main()
