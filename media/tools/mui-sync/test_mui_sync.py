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
