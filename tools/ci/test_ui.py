import json
import contextlib
import io
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from ui import gallery_summary, main, read_events, verify_pixels


def completed():
    return [{"event": "run_begin", "scenes": ["fixture"], "variants_per_scene": 1, "steps_per_case": 2},
            {"event": "case_begin", "case": 0, "scene": "fixture"},
            {"event": "frame", "case": 0, "step": 0, "presented": True, "resolve_ms": 2, "submit_ms": 3, "peak_texture_bytes": 64},
            {"event": "frame", "case": 0, "step": 1, "presented": False, "resolve_ms": 1, "submit_ms": 0},
            {"event": "case_end", "case": 0},
            {"event": "run_end", "completed": 1, "expected": 1}]


class UiEvidenceTests(unittest.TestCase):
    def test_failed_child_and_timeout_preserve_result(self):
        for code, deadline, exit_code in [("raise SystemExit(17)", "10", 17),
                                          ("import time; time.sleep(10)", "1", None)]:
            with tempfile.TemporaryDirectory() as name:
                with patch("sys.argv", ["ui.py", "--output", name, "--timeout", deadline,
                                        "--", sys.executable, "-c", code]), \
                     patch("platform.system", return_value="fixture"), \
                     contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(main(), 1)
                result = json.loads((Path(name) / "result.json").read_text())
                self.assertEqual(result["status"], "failed")
                self.assertIsNotNone(result["exit_code"])
                if exit_code is not None:
                    self.assertEqual(result["exit_code"], exit_code)
                else:
                    self.assertIn("exceeded", result["error"])

    def test_incomplete_or_unpresented_runs_never_pass(self):
        events = completed()
        self.assertEqual(gallery_summary(events)["presented"], 1)
        for bad in [events[:-1], events[:3] + events[4:], events + [events[4]]]:
            with self.assertRaises(ValueError):
                gallery_summary(bad)
        events[2]["presented"] = False
        with self.assertRaises(ValueError):
            gallery_summary(events)

    def test_partial_crash_record_keeps_last_completed_stage(self):
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / "events.jsonl"
            path.write_text(json.dumps({"event": "frame_begin", "case": 7}) + '\n{"event":')
            self.assertEqual(read_events(path), [{"event": "frame_begin", "case": 7}])

    @unittest.skipUnless(importlib.util.find_spec("PIL"), "pixel checks run in the display job with Pillow")
    def test_visible_specimen_required_in_captured_window(self):
        from PIL import Image
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / "window.png"
            image = Image.new("RGB", (100, 80), "black")
            image.save(path)
            with self.assertRaises(ValueError):
                verify_pixels(path, [20, 0, 80, 80])
            image.putpixel((40, 40), (0, 255, 0))
            image.save(path)
            self.assertEqual(verify_pixels(path, [20, 0, 80, 80])["width"], 100)
            with self.assertRaises(ValueError):
                verify_pixels(path, [90, 0, 80, 80])


if __name__ == "__main__":
    unittest.main()
