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
    def test_variable_binding_and_duplicate_tables_apply(self):
        cargo = self.root / 'Cargo.toml'
        cargo.write_text(cargo.read_text() + "[target.'cfg(windows)'.dev-dependencies]\n" + cargo.read_text().splitlines()[-1] + "\n")
        workflow = self.root / '.github/workflows/ci.yml'
        workflow.parent.mkdir(parents=True)
        workflow.write_text('env:\n  MOOSE_REV: "' + OLD + '"\nsteps:\n  - run: cargo install cargo-moose --git https://github.com/Matari-Audio/moose --rev "$MOOSE_REV" --locked\n')
        before_cargo, before_workflow = cargo.read_text(), workflow.read_text()
        subprocess.run(['git', '-C', str(self.root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(self.root), '-c', 'user.name=Test', '-c', 'user.email=test@example.com', 'commit', '-qm', 'workflow'], check=True)
        result = self.run_cli('--apply')
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(cargo.read_text(), before_cargo.replace(OLD, NEW))
        self.assertEqual(workflow.read_text(), before_workflow.replace(OLD, '5dfb23df2c9eebfef229b0d2e286110637197c79'))
        subprocess.run(['git', '-C', str(self.root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(self.root), '-c', 'user.name=Test', '-c', 'user.email=test@example.com', 'commit', '-qm', 'pins'], check=True)
        self.assertIn('0 files proposed', self.run_cli('--apply').stdout)

    def test_ambiguous_variable_refuses_without_writes(self):
        workflow = self.root / '.github/workflows/ci.yml'
        workflow.parent.mkdir(parents=True)
        workflow.write_text('env:\n  MOOSE_REV: "' + OLD + '"\n  MOOSE_REV: ${{ inputs.rev }}\nsteps:\n  - run: cargo install cargo-moose --git https://github.com/Matari-Audio/moose --rev "$MOOSE_REV"\n')
        subprocess.run(['git', '-C', str(self.root), 'add', '.'], check=True)
        subprocess.run(['git', '-C', str(self.root), '-c', 'user.name=Test', '-c', 'user.email=test@example.com', 'commit', '-qm', 'workflow'], check=True)
        before = (self.root / 'Cargo.toml').read_bytes(), workflow.read_bytes()
        result = self.run_cli('--apply')
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(before, ((self.root / 'Cargo.toml').read_bytes(), workflow.read_bytes()))

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
