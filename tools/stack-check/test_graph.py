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

class WindowsAbi(unittest.TestCase):
    def test_dx12_compares_only_allocator_and_hal_windows(self):
        import check
        from unittest.mock import patch
        from types import SimpleNamespace
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.lock').write_text('version = 4\n')
            (root / 'Cargo.toml').write_text('')
            ids = ['app', 'hal', 'alloc', 'win-new', 'win-old', 'unrelated']
            names = ['app', 'wgpu-hal', 'gpu-allocator', 'windows', 'windows', 'midir']
            def dep(pkg): return {'pkg': pkg, 'dep_kinds': [{'kind': None}]}
            packages = [{'id': i, 'name': n, 'source': None, 'manifest_path': str(root / 'Cargo.toml')} for i, n in zip(ids, names)]
            nodes = [{'id': 'app', 'deps': [dep('hal'), dep('unrelated')]},
                     {'id': 'hal', 'features': ['dx12'], 'deps': [dep('alloc'), dep('win-new')]},
                     {'id': 'alloc', 'deps': [dep('win-old')]},
                     {'id': 'unrelated', 'deps': [dep('win-old')]},
                     {'id': 'win-new', 'deps': []}, {'id': 'win-old', 'deps': []}]
            metadata = {'workspace_root': str(root), 'workspace_members': ['app'], 'packages': packages, 'resolve': {'nodes': nodes}}
            config = {'target': 'x86_64-pc-windows-msvc', 'feature_mode': 'default', 'features': [], 'root_packages': ['app'], 'lockfile': 'Cargo.lock'}
            def run(*args, **kwargs):
                return SimpleNamespace(returncode=0, stdout=('\n'.join([config['target'], 'x86_64-unknown-linux-gnu']) if args[0][0] == 'rustc' else json.dumps(metadata)), stderr='')
            with patch.object(check.subprocess, 'run', side_effect=run):
                report, _ = check.shipping_graph(root, {'family': {}}, config)
                self.assertEqual(report['issues'][0]['kind'], 'windows-abi-mismatch')
                nodes[2]['deps'] = [dep('win-new')]
                self.assertEqual(check.shipping_graph(root, {'family': {}}, config)[0]['issues'], [])
                nodes[2]['deps'] = [dep('win-old')]
                config['target'] = 'x86_64-unknown-linux-gnu'
                self.assertEqual(check.shipping_graph(root, {'family': {}}, config)[0]['issues'], [])

class UpstreamRenderer(unittest.TestCase):
    def test_egui_renderer_exception_requires_exact_version_and_registry_parent(self):
        import check
        from unittest.mock import patch
        from types import SimpleNamespace
        registry = 'registry+https://github.com/rust-lang/crates.io-index'
        manifest = {'family': {'mui': {'repository': 'https://github.com/Matari-Audio/MUI', 'revision': 'b' * 40, 'packages': ['vello_cpu']}},
                    'upstream_coexistence': [{'package': 'vello_cpu', 'version': '0.0.9', 'parent': 'epaint'}]}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.lock').write_text('version = 4\n')
            (root / 'Cargo.toml').write_text('')
            packages = [{'id': name, 'name': name, 'version': version, 'source': source, 'manifest_path': str(root / 'Cargo.toml')}
                        for name, version, source in [('app', '0.1.0', None), ('epaint', '0.35.0', registry), ('vello_cpu', '0.0.9', registry)]]
            def dep(name): return {'pkg': name, 'dep_kinds': [{'kind': None}]}
            nodes = [{'id': 'app', 'deps': [dep('epaint')]}, {'id': 'epaint', 'deps': [dep('vello_cpu')]}, {'id': 'vello_cpu', 'deps': []}]
            metadata = {'workspace_root': str(root), 'workspace_members': ['app'], 'packages': packages, 'resolve': {'nodes': nodes}}
            config = {'target': 'x86_64-unknown-linux-gnu', 'feature_mode': 'default', 'features': [], 'root_packages': ['app'], 'lockfile': 'Cargo.lock'}
            def run(*args, **kwargs):
                return SimpleNamespace(returncode=0, stdout=config['target'] if args[0][0] == 'rustc' else json.dumps(metadata), stderr='')
            with patch.object(check.subprocess, 'run', side_effect=run):
                self.assertEqual(check.shipping_graph(root, manifest, config)[0]['issues'], [])
                packages[1]['name'] = 'different-parent'
                self.assertEqual(check.shipping_graph(root, manifest, config)[0]['issues'][0]['kind'], 'unregistered-source')
                packages[1]['name'] = 'epaint'
                packages[1]['source'] = 'git+https://example.com/epaint#' + 'c' * 40
                self.assertEqual(check.shipping_graph(root, manifest, config)[0]['issues'][0]['kind'], 'unregistered-source')
                packages[1]['source'] = registry
                packages[2]['version'] = '0.0.10'
                self.assertEqual(check.shipping_graph(root, manifest, config)[0]['issues'][0]['kind'], 'unregistered-source')

class CompatibilityLane(unittest.TestCase):
    def test_legacy_and_fixed_baseview_coexist_but_unfixed_03_is_rejected(self):
        import check
        from unittest.mock import patch
        from types import SimpleNamespace
        registry = 'registry+https://github.com/rust-lang/crates.io-index'
        revision = 'b' * 40
        fixed = 'git+https://github.com/DerpcatMusic/baseview?rev=' + revision + '#' + revision
        manifest = {'family': {'compat': {'repository': 'https://github.com/DerpcatMusic/baseview', 'revision': revision, 'packages': ['baseview'], 'api_lines': {'baseview': '0.3'}}}}
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / 'Cargo.lock').write_text('version = 4\n')
            (root / 'Cargo.toml').write_text('')
            packages = [{'id': identifier, 'name': name, 'version': version, 'source': source, 'manifest_path': str(root / 'Cargo.toml')}
                        for identifier, name, version, source in [('app', 'app', '0.1.0', None), ('old', 'baseview', '0.1.1', registry), ('fixed', 'baseview', '0.3.2', fixed)]]
            def dep(name): return {'pkg': name, 'dep_kinds': [{'kind': None}]}
            metadata = {'workspace_root': str(root), 'workspace_members': ['app'], 'packages': packages,
                        'resolve': {'nodes': [{'id': 'app', 'deps': [dep('old'), dep('fixed')]}, {'id': 'old', 'deps': []}, {'id': 'fixed', 'deps': []}]}}
            config = {'target': 'x86_64-unknown-linux-gnu', 'feature_mode': 'default', 'features': [], 'root_packages': ['app'], 'lockfile': 'Cargo.lock'}
            def run(*args, **kwargs):
                return SimpleNamespace(returncode=0, stdout=config['target'] if args[0][0] == 'rustc' else json.dumps(metadata), stderr='')
            with patch.object(check.subprocess, 'run', side_effect=run):
                report, _ = check.shipping_graph(root, manifest, config)
                self.assertEqual(report['issues'], [])
                self.assertEqual([p['id'] for p in report['resolved']], ['fixed'])
                packages[2]['source'] = registry
                for version in ('0.3.2', '0.3.7'):
                    packages[2]['version'] = version
                    self.assertEqual(check.shipping_graph(root, manifest, config)[0]['issues'][0]['kind'], 'unregistered-source')
