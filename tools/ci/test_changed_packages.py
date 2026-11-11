import unittest

from changed_packages import select

PACKAGES = {
    "mui-scene": ("crates/mui-scene", set()),
    "mui": ("crates/mui", {"mui-scene"}),
    "mui-headless": ("crates/mui-headless", {"mui"}),
    "mui-vello": ("crates/mui-vello", set()),
}


class Select(unittest.TestCase):
    def test_a_leaf_change_tests_only_that_package(self):
        self.assertEqual(select(["crates/mui-headless/src/cli.rs"], PACKAGES), ["mui-headless"])

    def test_a_change_pulls_in_every_dependent(self):
        self.assertEqual(select(["crates/mui-scene/src/lib.rs"], PACKAGES), ["mui", "mui-headless", "mui-scene"])

    def test_shared_files_mean_the_whole_workspace(self):
        for f in ("Cargo.lock", ".github/workflows/pr.yml", "vendor/vello_cpu/src/lib.rs", "tools/verify.sh"):
            self.assertIsNone(select([f], PACKAGES), f)

    def test_docs_and_media_touch_no_root_package(self):
        self.assertEqual(select(["docs/platform-notes.md", "media/mui-cut/src/lib.rs"], PACKAGES), [])


if __name__ == "__main__":
    unittest.main()
