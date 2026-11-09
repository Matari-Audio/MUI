import json
import os
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent


class ShippingGraph(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=HERE)
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        (self.root / 'Cargo.toml').write_text('[package]\nname="app"\nversion="0.1.0"\n')
        (self.root / 'Cargo.lock').write_text('version = 4\n')
        self.config = {'target': 'x86_64-unknown-linux-gnu', 'feature_mode': 'no-default', 'features': ['app/special'], 'root_packages': ['app'], 'lockfile': 'Cargo.lock'}
        (self.root / 'config.json').write_text(json.dumps(self.config))
        self.bin = self.root / 'bin'
        self.bin.mkdir()
        cargo = self.bin / 'cargo'
        cargo.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
p = pathlib.Path.cwd()
(p / 'arguments.json').write_text(json.dumps(sys.argv[1:]))
if os.environ.get('MUTATE'): (p / 'Cargo.lock').write_text('changed')
root = 'path+file:///app#app@0.1.0'
print(json.dumps({'workspace_root': str(p), 'workspace_members': [root], 'packages': [{'id': root, 'name': 'app', 'source': None, 'manifest_path': str(p / 'Cargo.toml')}], 'resolve': {'nodes': [{'id': root, 'dependencies': [], 'deps': []}]}}))
''')
        cargo.chmod(0o755)

    def run_cli(self, mutate=False):
        env = dict(os.environ, PATH=str(self.bin) + ':' + os.environ['PATH'])
        if mutate: env['MUTATE'] = '1'
        return subprocess.run(['python3', str(HERE / 'check.py'), str(self.root), '--configuration', str(self.root / 'config.json')], text=True, capture_output=True, env=env)

    def test_passes_exact_flags_and_rejects_lock_mutation(self):
        result = self.run_cli()
        self.assertEqual(result.returncode, 0, result.stderr)
        args = json.loads((self.root / 'arguments.json').read_text())
        self.assertEqual(args, ['metadata', '--format-version', '1', '--locked', '--filter-platform', self.config['target'], '--no-default-features', '--features', 'app/special'])
        result = self.run_cli(mutate=True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('modified the lockfile', result.stderr)

    def test_shipping_configuration_ignores_a_separate_nested_lockfile(self):
        nested = self.root / 'unused-tool'
        nested.mkdir()
        (nested / 'Cargo.lock').write_text('version = 4\n[[package]]\nname="mui"\nversion="0.1.0"\n')
        (nested / 'Cargo.toml').write_text('[package]\nname="historical"\nversion="0.1.0"\n[dependencies]\nmui = { git = "https://github.com/Matari-Audio/MUI", rev = "' + 'a' * 40 + '" }\n')
        result = self.run_cli()
        self.assertEqual(result.returncode, 0, result.stderr)
        broad = subprocess.run(['python3', str(HERE / 'check.py'), str(self.root)], text=True, capture_output=True)
        self.assertEqual(broad.returncode, 1)
        self.assertIn('declaration-drift', broad.stdout)
        report = json.loads(result.stdout)['repositories'][0]
        self.assertEqual([lock['path'] for lock in report['lockfiles']], ['Cargo.lock'])
