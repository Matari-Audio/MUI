import json
import contextlib
import io
import importlib.util
from pathlib import Path
import sys
import tempfile
import unittest
from unittest.mock import patch
from ui import capture, gallery_summary, gpui_summary, main, read_events, verify_pixels


def completed():
    return [{"event": "run_begin", "scenes": ["fixture"], "variants_per_scene": 1, "steps_per_case": 2},
            {"event": "case_begin", "case": 0, "scene": "fixture"},
            {"event": "frame", "case": 0, "step": 0, "presented": True, "resolve_ms": 2, "submit_ms": 3, "peak_texture_bytes": 64},
            {"event": "frame", "case": 0, "step": 1, "presented": False, "resolve_ms": 1, "submit_ms": 0},
            {"event": "case_end", "case": 0},
            {"event": "run_end", "completed": 1, "expected": 1}]


class UiEvidenceTests(unittest.TestCase):
    def test_gpui_requires_work_and_an_honest_shutdown_boundary(self):
        events = [{"event": "run_begin", "test_ui": True},
                  *[{"event": "cpu_image_ready"} for _ in range(5)],
                  {"event": "scripted_input_changed_image"},
                  {"event": "test_complete", "completed": True, "resize_from": [100, 100],
                   "resize_to": [75, 75], "native_frame_callbacks": 5, "native_capture_acknowledgements": 5},
                  {"event": "native_shutdown_hook", "completed": True, "error": None,
                   "shutdown_phase": "after_gpui_window_clear_and_entity_flush",
                   "probe_resources_dropped": True, "native_application_returned": False}]
        self.assertFalse(gpui_summary(events, 5)["native_application_returned"])
        for bad in [events[:-1], events[1:], events[:6] + events[7:]]:
            with self.assertRaises(ValueError):
                gpui_summary(bad)
        for change in [{"completed": False}, {"probe_resources_dropped": False},
                       {"error": "failed"}, {"native_application_returned": True}]:
            with self.assertRaises(ValueError):
                gpui_summary(events[:-1] + [{**events[-1], **change}])
        with self.assertRaises(ValueError):
            gpui_summary(events, 4)
        for extent in [None, [75], [0, 75], [True, 75], [100, 100]]:
            with self.assertRaises(ValueError):
                gpui_summary(events[:-2] + [{**events[-2], "resize_to": extent}, events[-1]])
        events[-1] = {"event": "run_end", "completed": True, "error": None,
                      "probe_resources_dropped": True, "native_application_returned": True}
        self.assertTrue(gpui_summary(events, 5)["native_application_returned"])

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

    def test_gpu_probe_rejects_cpu_fallback_and_missing_mode(self):
        events = completed()
        with self.assertRaises(ValueError):
            gallery_summary(events, "gpu")
        for frame in events[2:4]:
            frame["rendering_mode"] = "gpu"
        self.assertEqual(gallery_summary(events, "gpu")["rendering_modes"], ["gpu"])
        events[3]["rendering_mode"] = "cpu"
        with self.assertRaises(ValueError):
            gallery_summary(events, "gpu")
        for frame in events[2:4]:
            frame["rendering_mode"] = "cpu"
        self.assertEqual(gallery_summary(events, "cpu")["presented"], 1)

    def test_partial_crash_record_keeps_last_completed_stage(self):
        with tempfile.TemporaryDirectory() as name:
            path = Path(name) / "events.jsonl"
            path.write_text(json.dumps({"event": "frame_begin", "case": 7}) + '\n{"event":')
            self.assertEqual(read_events(path), [{"event": "frame_begin", "case": 7}])

    def test_native_capture_and_teardown_evidence_are_required(self):
        for code, arguments, message in [
            ("pass", ["--capture"], "no verified capture"),
            ("import sys; print('objc_disposeClassPair: class still has subclasses', file=sys.stderr)", [], "cached Objective-C class"),
        ]:
            with tempfile.TemporaryDirectory() as name:
                with patch("sys.argv", ["ui.py", "--output", name, *arguments, "--", sys.executable, "-c", code]), \
                     patch("platform.system", return_value="Linux"), \
                     patch("ui.subprocess.run", side_effect=FileNotFoundError), \
                     contextlib.redirect_stdout(io.StringIO()):
                    self.assertEqual(main(), 1)
                self.assertIn(message, json.loads((Path(name) / "result.json").read_text())["error"])

    def test_capture_rejects_reference_paths_outside_results(self):
        for reference in ["../private.png", "/tmp/private.png", 7, "reference.txt"]:
            with self.assertRaisesRegex(ValueError, "local PNG filename"):
                capture(Path("/tmp/results"), {"reference": reference})

    @unittest.skipUnless(importlib.util.find_spec("PIL"), "pixel checks run in the display job with Pillow")
    def test_native_pixels_must_match_opaque_cpu_reference(self):
        from PIL import Image
        with tempfile.TemporaryDirectory() as name:
            native, reference = Path(name) / "native.png", Path(name) / "reference.png"
            expected = Image.new("RGBA", (2, 2), (255, 0, 0, 255))
            expected.putpixel((1, 1), (0, 0, 0, 0))
            expected.save(reference)
            observed = expected.convert("RGB")
            observed.putpixel((1, 1), (0, 255, 0))  # Transparent pixels depend on the native background.
            observed.save(native)
            self.assertEqual(verify_pixels(native, [0, 0, 2, 2], reference)["compared_pixels"], 3)
            observed.putpixel((0, 0), (0, 255, 0))
            observed.save(native)
            with self.assertRaisesRegex(ValueError, "differ from CPU reference"):
                verify_pixels(native, [0, 0, 2, 2], reference)
            with self.assertRaisesRegex(ValueError, "dimensions differ"):
                verify_pixels(native, [0, 0, 1, 2], reference)

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
