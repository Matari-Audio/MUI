import json
import subprocess
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
OLD = 'a' * 40
NEW = '7ecbec645affb8a81216854688a2e1f0cf6944e9'

class Update(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory(dir=HERE)
        self.root = Path(self.tmp.name)
        subprocess.run(['git', 'init', '-q', '-b', 'fixture', str(self.root)], check=True)
        (self.root / 'Cargo.toml').write_text('''[package]\nname = "example"\nversion = "0.1.0"\n[target.'cfg(unix)'.dev-dependencies]\nrenamed = { package = "mui", git = "https://github.com/Matari-Audio/MUI", rev = "''' + OLD + '''", optional = true, default-features = false, features = ["x"] }\n''')
        (self.root / 'Cargo.lock').write_text('version = 3\n')
        subprocess.run(['git', '-C', str(self.root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(self.root), '-c', 'user.name=Test', '-c', 'user.email=test@example.com', 'commit', '-qm', 'fixture'], check=True)
        self.addCleanup(self.tmp.cleanup)
    def run_cli(self, *args):
        return subprocess.run(['python3', str(HERE / 'update.py'), str(self.root), *args], capture_output=True, text=True)
    def test_apply_preserves_flags_and_is_idempotent(self):
        before = (self.root / 'Cargo.toml').read_text()
        result = self.run_cli('--apply')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((self.root / 'Cargo.toml').read_text(), before.replace(OLD, NEW))
        subprocess.run(['git', '-C', str(self.root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(self.root), '-c', 'user.name=Test', '-c', 'user.email=test@example.com', 'commit', '-qm', 'update'], check=True)
        self.assertIn('0 files proposed', self.run_cli('--apply').stdout)
    def test_dirty_tree_and_local_patch_leave_files_unchanged(self):
        path = self.root / 'Cargo.toml'
        path.write_text(path.read_text() + '\n[patch.crates-io]\nmui = { path = "vendor/mui" }\n')
        before = path.read_bytes()
        self.assertNotEqual(self.run_cli('--apply').returncode, 0)
        self.assertNotEqual(self.run_cli().returncode, 0)
        self.assertEqual(path.read_bytes(), before)
    def test_registered_duplicate_sources_allow_plan_but_checker_rejects(self):
        lock = self.root / 'Cargo.lock'
        lock.write_text("""version = 4
[[package]]
name = "moose-baseview"
version = "0.1.0"
source = "git+https://github.com/Matari-Audio/MUI?rev=aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa#aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
[[package]]
name = "moose-baseview"
version = "0.1.0"
source = "git+https://github.com/Matari-Audio/moose?rev=bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb#bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
""")
        plan = self.run_cli()
        self.assertEqual(plan.returncode, 0, plan.stderr)
        self.assertIn(NEW, plan.stdout)
        check = subprocess.run(['python3', str(HERE / 'check.py'), str(self.root)], capture_output=True, text=True)
        self.assertEqual(check.returncode, 1)
        self.assertIn('multiple-sources', check.stdout)
