"""The rev-pin guard: `python3 -m unittest discover media/tools/mui-sync`."""
import importlib.util
import tempfile
import unittest
from pathlib import Path

spec = importlib.util.spec_from_file_location('mui_sync', Path(__file__).with_name('mui-sync.py'))
sync = importlib.util.module_from_spec(spec)
spec.loader.exec_module(sync)

VOLT = '''[dependencies]
moose = { git = "https://github.com/Matari-Audio/moose", rev = "48879b19" }
mui = { git = "https://github.com/Matari-Audio/MUI", rev = "48dde277444db4c4b9fa1099ecd52f997a18402a", features = ["cpu"] }
mui-text = { rev = "1", git = "https://github.com/Matari-Audio/MUI" }
mui-baseview = { git = "https://github.com/Matari-Audio/MUI" }
# mui-old = { git = "https://github.com/Matari-Audio/MUI", rev = "1" }
'''


class Guard(unittest.TestCase):
    def test_flags_mui_rev_pins_only(self):
        self.assertEqual(sync.pins(VOLT), ['mui', 'mui-text'])

    def test_unpin_drops_mui_revs_and_keeps_the_rest(self):
        out = sync.unpin(VOLT)
        self.assertEqual(sync.pins(out), [])
        self.assertIn('mui = { git = "https://github.com/Matari-Audio/MUI", features = ["cpu"] }', out)
        self.assertIn('mui-text = { git = "https://github.com/Matari-Audio/MUI" }', out)
        self.assertIn('rev = "48879b19"', out, 'moose keeps its pin')

    def test_check_fails_on_a_pinned_repo_and_skips_vendor(self):
        with tempfile.TemporaryDirectory(dir=Path(__file__).parent) as d:
            d = Path(d)
            (d / 'vendor/x').mkdir(parents=True)
            (d / 'vendor/x/Cargo.toml').write_text(VOLT)
            (d / 'Cargo.toml').write_text(sync.unpin(VOLT))
            self.assertEqual(sync.check([d]), 0)
            (d / 'Cargo.toml').write_text(VOLT)
            self.assertEqual(sync.check([d]), 1)


if __name__ == '__main__':
    unittest.main()


class Sync(unittest.TestCase):
    def test_reports_short_format_errors_with_their_location(self):
        out = 'warning: x\nsrc/gui/host.rs:3:5: error[E0412]: cannot find type `WindowHandle`\nerror: could not compile `p`\n'
        self.assertEqual(sync.errors(out), [
            'src/gui/host.rs:3:5: error[E0412]: cannot find type `WindowHandle`',
            'error: could not compile `p`',
        ])

    def test_links_path_dependencies_beside_the_repo_into_the_worktree(self):
        with tempfile.TemporaryDirectory() as d:
            d = Path(d)
            (d / 'repos/Access').mkdir(parents=True)
            (d / 'repos/KURV').mkdir()
            wt = d / 'work/sync-KURV'
            wt.mkdir(parents=True)
            (wt / 'Cargo.toml').write_text('[dependencies]\na = { path = "../Access" }\nb = { path = "crates/b" }\n')
            made = sync.siblings(d / 'repos/KURV', wt)
            self.assertEqual(made, [d / 'work/Access'])
            self.assertEqual((d / 'work/Access').resolve(), (d / 'repos/Access').resolve())

    def test_names_a_mui_package_by_version_when_crates_io_has_one_too(self):
        lock = '''[[package]]
name = "mui"
version = "0.1.0"
source = "git+https://github.com/Matari-Audio/MUI#abc"

[[package]]
name = "vello_encoding"
version = "0.6.0"
source = "git+https://github.com/Matari-Audio/MUI#abc"

[[package]]
name = "vello_encoding"
version = "0.5.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
'''
        self.assertEqual(sync.mui_packages(lock), ['mui', 'vello_encoding@0.6.0'])
