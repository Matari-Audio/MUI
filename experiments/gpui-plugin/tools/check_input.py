#!/usr/bin/env python3
"""Run existing input assertions under headless Weston/Xwayland; retain failures and timeouts."""
import argparse
from contextlib import contextmanager
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


@contextmanager
def headless(output):
    # A rootful window on the user's compositor still receives physical input/focus.
    with tempfile.TemporaryDirectory(prefix='mui-wayland-') as runtime, (output / 'weston.log').open('w') as log:
        env = {**os.environ, 'XDG_RUNTIME_DIR': runtime, 'WAYLAND_DISPLAY': 'mui-input'}
        compositor = subprocess.Popen(['weston', '--backend=headless', '--renderer=gl',
            '--fake-seat', '--width=3200', '--height=2400', '--socket=mui-input',
            '--idle-time=0', '--no-config'], env=env, stdout=log, stderr=subprocess.STDOUT)
        try:
            deadline = time.monotonic() + 15
            while not (Path(runtime) / 'mui-input').exists():
                if compositor.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError(f'Headless Weston startup failed; see {output}')
                time.sleep(0.05)
            yield env
        finally:
            compositor.terminate()
            try:
                compositor.wait(timeout=5)
            except subprocess.TimeoutExpired:
                compositor.kill()
                compositor.wait()


@contextmanager
def xwayland(output, env):
    # Explicit display: automatic -displayfd allocation can unlink a rootless
    # compositor's live socket when that compositor has no conventional X lock.
    display = next(number for number in range(100, 1000)
                   if not os.path.lexists(f'/tmp/.X11-unix/X{number}')
                   and not os.path.lexists(f'/tmp/.X{number}-lock'))
    with (output / 'xwayland.log').open('w') as log:
        server = subprocess.Popen(['Xwayland', f':{display}', '-ac', '-noreset',
            '-nolisten', 'tcp', '-geometry', '3200x2400'], env=env, stdout=log, stderr=log)
        try:
            deadline = time.monotonic() + 15
            while not Path(f'/tmp/.X11-unix/X{display}').exists():
                if server.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError(f'Private Xwayland startup failed; see {output}')
                time.sleep(0.05)
            result = {**env, 'DISPLAY': f':{display}'}
            result.pop('WAYLAND_DISPLAY', None)
            result.pop('WAYLAND_SOCKET', None)
            yield result
        finally:
            server.terminate()
            try:
                server.wait(timeout=5)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repeat', type=int, default=2)
    parser.add_argument('--output', type=Path)
    args = parser.parse_args()
    if args.repeat < 1:
        parser.error('--repeat must be positive')
    root = Path(__file__).resolve().parents[3]
    manifest = root / 'experiments/gpui-plugin/Cargo.toml'
    subprocess.run(['cargo', 'build', '--locked', '--manifest-path', str(manifest),
                    '--bins', '--example', 'oscillator'], cwd=root, check=True)
    metadata = json.loads(subprocess.check_output([
        'cargo', 'metadata', '--no-deps', '--format-version=1',
        '--manifest-path', str(manifest)], cwd=root))
    binaries = Path(metadata['target_directory']) / 'debug'
    output = args.output or Path(tempfile.mkdtemp(prefix='mui-input-'))
    output.mkdir(parents=True, exist_ok=True)
    results = []
    with headless(output) as wayland, xwayland(output, wayland) as env:
        cases = [('composition', scale, width) for scale in ('1', '1.5', '2')
                 for width in ('1280', '1480')]
        # The native host probe currently uses physical XTest coordinates at 1x.
        cases.append(('embedded', '1', '800'))
        for repeat in range(args.repeat):
            for kind, scale, width in cases:
                name = f'{kind}-{width}-{scale}x-{repeat}'
                command = ([str(binaries / 'examples/oscillator'), '--kurv', '--check-interactions']
                           if kind == 'composition' else [str(binaries / 'mui-gpui-plugin-probe')])
                with (output / (name + '.log')).open('w') as log:
                    try:
                        result = subprocess.run(command, cwd=root, env={**env,
                            'GPUI_X11_SCALE_FACTOR': scale, 'MUI_CHECK_WIDTH': width},
                            stdout=log, stderr=subprocess.STDOUT, timeout=60)
                        status = result.returncode
                    except subprocess.TimeoutExpired:
                        status = 'timeout'
                if status == 0 and 'PASS:' not in (output / (name + '.log')).read_text():
                    status = 'incomplete'
                results.append(dict(case=name, status=status))
                (output / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
                print(f'{name}: {status}', flush=True)
    print(f'Evidence: {output}')
    return int(any(result['status'] != 0 for result in results))


if __name__ == '__main__':
    raise SystemExit(main())
