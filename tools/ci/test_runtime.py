import unittest
from unittest.mock import patch

import runtime


class RuntimeTests(unittest.TestCase):
    def test_attempt_window_excludes_time_before_rerun(self):
        run = {
            "run_attempt": 2, "created_at": "2026-10-03T05:00:00Z",
            "run_started_at": "2026-10-03T09:00:00Z", "updated_at": "2026-10-03T09:11:00Z",
        }
        jobs = [{
            "id": 1, "conclusion": "success", "started_at": "2026-10-03T09:01:00Z",
            "completed_at": "2026-10-03T09:10:00Z", "steps": [],
        }]
        report = runtime.summarize(run, jobs)
        self.assertEqual(report["creation_to_update_seconds"], 15060)
        self.assertEqual(report["latest_attempt_to_last_job_seconds"], 600)
        self.assertEqual(report["latest_attempt_to_first_job_seconds"], 60)
        self.assertEqual(report["job_window_seconds"], 540)

    def test_skipped_missing_and_negative_intervals_are_unknown(self):
        jobs = [
            {"id": 1, "conclusion": "skipped", "started_at": "2026-10-03T09:00:00Z", "completed_at": "2026-10-03T09:00:00Z"},
            {"id": 2, "conclusion": "cancelled", "started_at": "2026-10-03T09:00:01Z", "completed_at": "2026-10-03T09:00:00Z"},
            {"id": 3, "conclusion": None, "started_at": None, "completed_at": None},
        ]
        report = runtime.summarize({}, jobs)
        self.assertIsNone(report["job_window_seconds"])
        self.assertTrue(all(job["elapsed_seconds"] is None for job in report["jobs"]))

    def test_job_pages_are_pinned_to_observed_attempt(self):
        run = {"run_attempt": 2}
        job = {"id": 1, "conclusion": "skipped"}
        with patch("runtime.api", side_effect=[run, {"jobs": [job] * 100}, {"jobs": [job]}]) as api:
            report = runtime.collect("owner/repo", 123)
        self.assertEqual(len(report["jobs"]), 101)
        self.assertEqual(api.call_args_list[1].args[0], "repos/owner/repo/actions/runs/123/attempts/2/jobs?per_page=100&page=1")
        self.assertEqual(api.call_args_list[2].args[0], "repos/owner/repo/actions/runs/123/attempts/2/jobs?per_page=100&page=2")

    def test_request_failure_does_not_relay_arbitrary_cli_output(self):
        result = runtime.subprocess.CompletedProcess([], 1, "", "private diagnostic")
        with patch("runtime.subprocess.run", return_value=result):
            with self.assertRaisesRegex(RuntimeError, r"gh api failed.*exit 1") as error:
                runtime.api("repos/owner/repo/actions/runs/123")
        self.assertNotIn("private diagnostic", str(error.exception))


if __name__ == "__main__":
    unittest.main()
