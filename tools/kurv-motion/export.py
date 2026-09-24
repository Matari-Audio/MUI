#!/usr/bin/env python3
"""Export real Kurv layers using its pinned storybook, without editing its checkout."""
import argparse
import io
import json
import os
from pathlib import Path
import shutil
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parents[2]
REVISION = '25f57dcefb2111bfae5fc1dd355b4eed65c56172'

def run(args, **kw):
    return subprocess.run(args, check=True, **kw)

def install_capture_runtime(dst):
    """Reuse the shared framework capture/transport crate in a pinned checkout."""
    shared = dst / '.build-inputs/mui2'
    shutil.copytree(ROOT / 'crates/mui-motion-bridge', shared / 'crates/mui-motion-bridge', dirs_exist_ok=True)
    workspace = shared / 'Cargo.toml'
    text = workspace.read_text()
    if '"crates/mui-motion-bridge"' not in text:
        workspace.write_text(text.replace('members = [', 'members = [\n  "crates/mui-motion-bridge",'))
    cargo = dst / 'Cargo.toml'
    text = cargo.read_text()
    text = '\n'.join(line for line in text.split('\n') if not line.startswith('mui-live = '))
    cargo.write_text(text)
    if '\nmui-motion-bridge = ' not in text:
        cargo.write_text(text.replace('[dependencies]', '[dependencies]\nmui-motion-bridge = { path = ".build-inputs/mui2/crates/mui-motion-bridge" }'))

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--kurv', type=Path, required=True, help='Kurv checkout with restored .build-inputs')
    p.add_argument('--build-dir', type=Path, required=True, help='Disposable isolated build directory')
    p.add_argument('--output', type=Path, required=True)
    p.add_argument('--story', default='showcase')
    p.add_argument('--size', default='1500x1036')
    p.add_argument('--scale', type=float, default=1.5)
    p.add_argument('--resize', nargs=3, metavar=('ID','WIDTH','HEIGHT'))
    p.add_argument('--parts', nargs='*', default=[])
    a = p.parse_args()
    src, build, out = a.kurv.resolve(), a.build_dir.resolve(), a.output.resolve()
    if build == src or src in build.parents or build in src.parents:
        p.error('build directory must be separate from the source checkout')
    build.mkdir(parents=True, exist_ok=True)
    dst = build / 'KURV'
    marker = build / '.mui-motion-export'
    if dst.exists() and (not marker.exists() or marker.read_text().strip() != REVISION):
        p.error('build directory is not a recognized isolated motion export; choose a fresh directory')
    if not dst.exists():
        dst.mkdir()
        data = subprocess.check_output(['git', '-C', str(src), 'archive', REVISION])
        with tarfile.open(fileobj=io.BytesIO(data)) as archive:
            archive.extractall(dst, filter='data')
        (dst / '.build-inputs').mkdir(exist_ok=True)
        (dst / '.build-inputs/mui').symlink_to(src / '.build-inputs/mui', target_is_directory=True)
        shutil.copytree(src / '.build-inputs/mui2', dst / '.build-inputs/mui2', ignore=shutil.ignore_patterns('.git', 'target'))
        (build / 'Matari-Access').symlink_to((src.parent / 'Matari-Access').resolve(), target_is_directory=True)
        for path in (src / 'vendor').iterdir():
            if path.is_symlink():
                target = dst / 'vendor' / path.name
                if target.is_symlink(): target.unlink()
                elif target.exists(): shutil.rmtree(target)
                target.symlink_to(path.resolve(), target_is_directory=True)
    marker.write_text(REVISION + '\n')
    for path in (src / 'vendor').iterdir():
        target = dst / 'vendor' / path.name
        if not target.exists():
            if path.is_dir(): shutil.copytree(path, target)
            else: shutil.copy2(path, target)
    # This is an offline asset tool, not a shipped DSP build. Keep iteration cheap.
    cargo = dst / 'Cargo.toml'
    config = cargo.read_text().replace('[profile.dev.package.pure_va_dispersion_core]\nopt-level = 3\ncodegen-units = 1', '[profile.dev.package.pure_va_dispersion_core]\nopt-level = 1\ncodegen-units = 32')
    if config != cargo.read_text(): cargo.write_text(config)
    # Overlay only the new capture API on the pinned MUI. Never replace Kurv's runtime.
    scene = dst / '.build-inputs/mui2/crates/mui-scene/src'
    capture_source = (ROOT / 'crates/mui-scene/src/capture.rs').read_text()
    capture_target = scene / 'capture.rs'
    if not capture_target.exists() or capture_target.read_text() != capture_source:
        capture_target.write_text(capture_source)
    lib = scene / 'lib.rs'
    text = lib.read_text()
    text = text.replace('pub use capture::CaptureError;', 'pub use capture::{resize_capture, CaptureError};')
    if 'mod capture;' not in text:
        text += '\nmod capture;\npub use capture::{resize_capture, CaptureError};\n'
    if lib.read_text() != text: lib.write_text(text)
    install_capture_runtime(dst)
    gallery = dst / 'src/editors/mui2/gallery.rs'
    # Reset only our isolated hook from the pinned source, making reruns idempotent.
    text = subprocess.check_output(['git', '-C', str(src), 'show', f'{REVISION}:src/editors/mui2/gallery.rs'], text=True)
    marker = 'let scene = ui.scene().expect("the frame just resolved");'
    if text.count(marker) != 1: raise RuntimeError('Kurv gallery hook changed')
    text = text.replace(marker, marker + '\n        motion_export(scene, width, height, scale)?;')
    text = text.replace('let root = racks.tree(&mut ui, &input);', 'let root = racks.tree(&mut ui, &input);\n        let root = motion_resize(&root)?;')
    text += '\n' + (ROOT / 'tools/kurv-motion/capture.rs').read_text()
    if gallery.read_text() != text: gallery.write_text(text)
    out.mkdir(parents=True, exist_ok=True)
    env = dict(os.environ, MUI_MOTION_OUTPUT=str(out), MUI_MOTION_PARTS=json.dumps(a.parts))
    if a.resize:
        env['MUI_MOTION_RESIZE'] = json.dumps([a.resize[0], float(a.resize[1]), float(a.resize[2])])
    else:
        env.pop('MUI_MOTION_RESIZE', None)
    run(['cargo', 'run', '--no-default-features', '--features', 'gallery', '--bin', 'kurv-gallery', '--', a.story, str(out / 'full.png'), '', str(a.scale), a.size], cwd=dst, env=env)
    manifest = json.loads((out / 'scene.json').read_text())
    manifest.update(source_revision=REVISION, story=a.story, parts=a.parts, resize=a.resize)
    (out / 'scene.json').write_text(json.dumps(manifest, indent=2) + '\n')

if __name__ == '__main__': main()
