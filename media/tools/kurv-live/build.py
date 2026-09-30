#!/usr/bin/env python3
"""Build KURV's mui-cut live adapter, `kurv-cut-live`, against this MUI.

KURV's checkout is only read: its committed tree (`git archive`) goes to a
disposable build directory, `bridge.rs` is appended to its gallery module,
the keyboard also lights the notes the host holds, and a `[patch]` points
every MUI git crate at this worktree, so the adapter, KURV's editor and
mui-motion-bridge are all this MUI.

    media/tools/kurv-live/build.py --kurv ../KURV          # -> bin/kurv-cut-live
    media/tools/kurv-live/build.py --print-patch            # the [patch] alone

Set CARGO_TARGET_DIR to share a target directory.
"""
import argparse
import io
import os
import shutil
import subprocess
import sys
import tarfile
import tomllib
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
MUI_GIT = 'https://github.com/Matari-Audio/MUI'
BIN = 'kurv-cut-live'
# Where the keyboard decides a key is lit, and what the overlay adds.
KEYS_HOOK = 'let on = self.held == Some(base + semitone);'
KEYS_LIT = 'let on = self.held == Some(base + semitone) || super::gallery::cut_held(base + semitone);'


def mui_crates():
    """Every package under crates/: (name, directory)."""
    out = []
    for toml in sorted((ROOT / 'crates').glob('*/Cargo.toml')):
        name = tomllib.loads(toml.read_text())['package']['name']
        out.append((name, toml.parent))
    return out


def patch():
    """The `[patch]` table sending KURV's MUI git crates to this worktree."""
    lines = [f'[patch."{MUI_GIT}"]']
    lines += [f'{name} = {{ path = "{path}" }}' for name, path in mui_crates()]
    return '\n'.join(lines) + '\n'


def overlay(dst):
    """Add the adapter, its binary and the patch to a KURV tree at `dst`."""
    gallery = dst / 'src/editors/mui2/gallery.rs'
    gallery.write_text(gallery.read_text() + (HERE / 'bridge.rs').read_text())
    keys = dst / 'src/editors/mui2/keys.rs'
    text = keys.read_text()
    if text.count(KEYS_HOOK) != 1:
        raise SystemExit('KURV keys.rs changed: update KEYS_HOOK in build.py')
    keys.write_text(text.replace(KEYS_HOOK, KEYS_LIT))
    (dst / f'src/bin/{BIN}.rs').write_text(
        'fn main() {\n'
        '    if let Err(e) = pure_va_dispersion_core::gallery::cut_live() {\n'
        '        eprintln!("{e}");\n'
        '        std::process::exit(1);\n'
        '    }\n'
        '}\n')
    cargo = dst / 'Cargo.toml'
    text = cargo.read_text()
    bridge = ROOT / 'media/mui-motion-bridge'
    text = text.replace('[dependencies]\n', f'[dependencies]\nmui-motion-bridge = {{ path = "{bridge}" }}\n', 1)
    text += f'\n[[bin]]\nname = "{BIN}"\npath = "src/bin/{BIN}.rs"\nrequired-features = ["gallery"]\n\n{patch()}'
    cargo.write_text(text)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('--kurv', type=Path, help='a KURV checkout (only read)')
    p.add_argument('--rev', default='HEAD', help='the KURV revision to build')
    p.add_argument('--build-dir', type=Path, default=HERE / '.build')
    p.add_argument('--out', type=Path, default=HERE / 'bin' / BIN)
    p.add_argument('--check', action='store_true', help='cargo check only')
    p.add_argument('--print-patch', action='store_true')
    a = p.parse_args()
    if a.print_patch:
        sys.stdout.write(patch())
        return
    if not a.kurv:
        p.error('--kurv is required')
    src, build = a.kurv.resolve(), a.build_dir.resolve()
    if build == src or src in build.parents or build in src.parents:
        p.error('the build directory must be outside the KURV checkout')
    rev = subprocess.check_output(['git', '-C', str(src), 'rev-parse', a.rev], text=True).strip()
    dst = build / 'KURV'
    # A fresh tree every time: the overlay is not idempotent, and a copy
    # of 22 MB of source is cheap next to the build.
    if dst.exists():
        shutil.rmtree(dst)
    dst.mkdir(parents=True)
    data = subprocess.check_output(['git', '-C', str(src), 'archive', rev])
    with tarfile.open(fileobj=io.BytesIO(data)) as archive:
        archive.extractall(dst, filter='data')
    # KURV's path dependency (../Matari-Access) and untracked vendor inputs.
    access = build / 'Matari-Access'
    if not access.exists():
        access.symlink_to((src.parent / 'Matari-Access').resolve(), target_is_directory=True)
    for path in (src / 'vendor').iterdir():
        target = dst / 'vendor' / path.name
        if not target.exists():
            target.symlink_to(path.resolve(), target_is_directory=path.is_dir())
    overlay(dst)
    # KURV's `process-lab` feature: a local lab build, not a product one.
    cmd = ['cargo', 'check' if a.check else 'build', '--no-default-features', '--features', 'gallery,process-lab', '--bin', BIN]
    if not a.check:
        cmd += ['--message-format=json-render-diagnostics']
    env = dict(os.environ)
    env.pop('RUSTUP_TOOLCHAIN', None)  # KURV's own rust-toolchain.toml
    out = subprocess.run(cmd, cwd=dst, env=env, check=True, stdout=subprocess.PIPE, text=True).stdout
    if a.check:
        return
    import json
    exe = next(m['executable'] for m in map(json.loads, out.splitlines())
               if m.get('reason') == 'compiler-artifact' and m['target']['name'] == BIN and m.get('executable'))
    a.out.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(exe, a.out)
    print(a.out)


if __name__ == '__main__':
    main()
