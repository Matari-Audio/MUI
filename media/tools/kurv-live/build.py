#!/usr/bin/env python3
"""Add the live bridge only to a disposable, initialized motion-export build."""
import argparse, os, subprocess, shutil
from pathlib import Path
ROOT=Path(__file__).resolve().parents[3]
p=argparse.ArgumentParser();p.add_argument('--build-dir',type=Path,default=Path('/tmp/kurv-motion-build'));a=p.parse_args()
build=a.build_dir.resolve();dst=build/'KURV'
if not (build/'.mui-motion-export').is_file():p.error('Run media/tools/kurv-motion/export.py to initialize an isolated build first.')
from runpy import run_path
run_path(str(ROOT/'media/tools/kurv-motion/export.py'))['install_capture_runtime'](dst)
gallery=dst/'src/editors/mui2/gallery.rs';s=gallery.read_text();s=s[:s.index('// Included only in an isolated Kurv storybook build')]+(ROOT/'media/tools/kurv-motion/capture.rs').read_text()+'\n'+(ROOT/'media/tools/kurv-live/bridge.rs').read_text();gallery.write_text(s)
binary=dst/'src/bin/kurv-motion-live.rs';binary.write_text('fn main(){if let Err(e)=pure_va_dispersion_core::gallery::motion_live(){eprintln!("{e}");std::process::exit(1);}}\n')
cargo=dst/'Cargo.toml';s=cargo.read_text()
if 'name = "kurv-motion-live"' not in s:s+='\n[[bin]]\nname = "kurv-motion-live"\npath = "src/bin/kurv-motion-live.rs"\nrequired-features = ["gallery"]\n'
cargo.write_text(s)
subprocess.run(['cargo','build','--no-default-features','--features','gallery','--bin','kurv-motion-live'],cwd=dst,check=True)
