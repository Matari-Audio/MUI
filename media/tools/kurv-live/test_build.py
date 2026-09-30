"""build.py's patch and overlay; with KURV_CHECKOUT set, a real `cargo check`
of KURV plus the adapter against this MUI.

    python3 media/tools/kurv-live/test_build.py
    KURV_CHECKOUT=../KURV python3 media/tools/kurv-live/test_build.py
"""
import os
import subprocess
import sys
import tempfile
import tomllib
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import build  # noqa: E402


class Patch(unittest.TestCase):
    def test_every_mui_crate_points_at_this_worktree(self):
        table = tomllib.loads(build.patch())['patch'][build.MUI_GIT]
        crates = {tomllib.loads(t.read_text())['package']['name']: t.parent
                  for t in (build.ROOT / 'crates').glob('*/Cargo.toml')}
        self.assertIn('mui', crates)
        self.assertEqual(set(table), set(crates))
        for name, entry in table.items():
            self.assertEqual(Path(entry['path']), crates[name])
            self.assertEqual(set(entry), {'path'}, 'a path, no version or rev pin')


class Overlay(unittest.TestCase):
    def test_adds_the_adapter_its_binary_the_bridge_and_the_patch(self):
        with tempfile.TemporaryDirectory(dir=build.HERE) as d:
            dst = Path(d)
            (dst / 'src/editors/mui2').mkdir(parents=True)
            (dst / 'src/bin').mkdir()
            (dst / 'src/editors/mui2/gallery.rs').write_text('// gallery\n')
            (dst / 'src/editors/mui2/keys.rs').write_text(f'fn lit() {{ {build.KEYS_HOOK} }}\n')
            (dst / 'Cargo.toml').write_text('[package]\nname = "k"\n\n[dependencies]\nserde = "1"\n')
            build.overlay(dst)
            self.assertIn('pub fn cut_live()', (dst / 'src/editors/mui2/gallery.rs').read_text())
            self.assertIn('cut_held(base + semitone)', (dst / 'src/editors/mui2/keys.rs').read_text())
            self.assertIn('cut_live()', (dst / f'src/bin/{build.BIN}.rs').read_text())
            cargo = tomllib.loads((dst / 'Cargo.toml').read_text())
            self.assertEqual(Path(cargo['dependencies']['mui-motion-bridge']['path']),
                             build.ROOT / 'media/mui-motion-bridge')
            self.assertEqual(cargo['bin'][0]['name'], build.BIN)
            self.assertIn('mui', cargo['patch'][build.MUI_GIT])

    def test_a_moved_keyboard_hook_is_an_error_not_a_silent_skip(self):
        with tempfile.TemporaryDirectory(dir=build.HERE) as d:
            dst = Path(d)
            (dst / 'src/editors/mui2').mkdir(parents=True)
            (dst / 'src/bin').mkdir()
            (dst / 'src/editors/mui2/gallery.rs').write_text('')
            (dst / 'src/editors/mui2/keys.rs').write_text('fn lit() {}\n')
            (dst / 'Cargo.toml').write_text('[dependencies]\n')
            with self.assertRaises(SystemExit):
                build.overlay(dst)


@unittest.skipUnless(os.environ.get('KURV_CHECKOUT'), 'set KURV_CHECKOUT to a KURV checkout')
class AgainstKurv(unittest.TestCase):
    def test_kurv_and_the_adapter_compile_against_this_mui(self):
        with tempfile.TemporaryDirectory(dir=Path.home()) as d:
            subprocess.run([sys.executable, str(build.HERE / 'build.py'), '--kurv', os.environ['KURV_CHECKOUT'],
                            '--build-dir', d, '--check'], check=True)


if __name__ == '__main__':
    unittest.main()
