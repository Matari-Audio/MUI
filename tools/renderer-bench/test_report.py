import json
from pathlib import Path
import tempfile
import unittest
from report import case


class ReportTests(unittest.TestCase):
    def test_not_run_is_never_a_pass(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(case(Path(tmp), "failure")["status"], "not-run")

    def test_crash_retains_last_stage_and_stderr(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "process").mkdir()
            (root / "process/result.json").write_text(json.dumps({"status": "failed", "exit_code": -11, "error": "native fault"}))
            (root / "process/stderr.log").write_text("driver fault evidence")
            (root / "stages.jsonl").write_text('{"stage":"draw_readback"}\n{"partial":')
            result = case(root, "success")
            self.assertEqual(result["status"], "failed")
            self.assertEqual(result["last_stage"]["stage"], "draw_readback")
            self.assertIn("driver fault", result["stderr_tail"])
            (root / "process/result.json").write_text('{"partial":')
            self.assertEqual(case(root, "success")["status"], "failed")

    def test_clean_exit_requires_complete_unique_samples_and_pixel_checks(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "process").mkdir()
            (root / "process/result.json").write_text('{"status":"passed","exit_code":0}')
            self.assertEqual(case(root, "success")["status"], "failed")
            rows = [{"mode": mode, "sample": i} for mode in ("static-redraw", "dynamic-redraw") for i in range(30)]
            rows += [{"mode": mode, "median_ms": 1, "samples": 30, "pixel_checks": "passed"} for mode in ("static-redraw", "dynamic-redraw")]
            def write():
                (root / "samples.jsonl").write_text("\n".join(json.dumps(row) for row in rows))
            write()
            (root / "stages.jsonl").write_text('{"stage":"complete"}\n')
            self.assertEqual(case(root, "success")["status"], "passed")
            rows[0] = rows[1]
            write()
            self.assertEqual(case(root, "success")["status"], "failed")


if __name__ == "__main__":
    unittest.main()
