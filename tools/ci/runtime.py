#!/usr/bin/env python3
"""Read GitHub Actions timings; requires authenticated `gh`, never reads logs.

Examples:
  python3 tools/ci/runtime.py --repo Matari-Audio/MUI --run 37105777320
  python3 tools/ci/runtime.py --repo zed-industries/zed --workflow run_tests.yml --limit 10

Output is metadata only. Job/step durations are elapsed wall time, not CPU time
or billed minutes. Attempt windows include scheduling and dependency waits.
Creation-to-update spans can include earlier attempts and time before approval.
"""

import argparse
from datetime import datetime, timezone
import json
import re
import subprocess
import sys
from urllib.parse import quote, urlencode


def timestamp(value):
    return datetime.fromisoformat(value.replace("Z", "+00:00")) if value else None


def seconds(start, end):
    if not start or not end:
        return None
    value = (timestamp(end) - timestamp(start)).total_seconds()
    # GitHub sometimes reports negative intervals for jobs canceled before start.
    return value if value >= 0 else None


def timed(item):
    return {
        key: item.get(key)
        for key in ("name", "status", "conclusion", "started_at", "completed_at")
    } | {
        "elapsed_seconds": None if item.get("conclusion") == "skipped" else seconds(
            item.get("started_at"), item.get("completed_at")
        ),
    }


def summarize(run, jobs):
    valid = [job for job in jobs if timed(job)["elapsed_seconds"] is not None]
    first = min((job["started_at"] for job in valid), default=None)
    last = max((job["completed_at"] for job in valid), default=None)
    return {
        key: run.get(key)
        for key in (
            "id", "html_url", "name", "event", "head_branch", "head_sha",
            "status", "conclusion", "run_attempt", "created_at", "run_started_at", "updated_at",
        )
    } | {
        "creation_to_update_seconds": seconds(run.get("created_at"), run.get("updated_at")),
        "latest_attempt_to_last_job_seconds": seconds(run.get("run_started_at"), last),
        "latest_attempt_to_first_job_seconds": seconds(run.get("run_started_at"), first),
        "job_window_seconds": seconds(first, last),
        "jobs": [timed(job) | {
            "id": job["id"], "html_url": job.get("html_url"),
            "runner_name": job.get("runner_name"), "labels": job.get("labels", []),
            "steps": [timed(step) | {"number": step.get("number")} for step in job.get("steps", [])],
        } for job in jobs],
    }


def api(endpoint):
    try:
        result = subprocess.run(
            ["gh", "api", endpoint], capture_output=True, text=True, timeout=45, check=False,
        )
    except (OSError, subprocess.TimeoutExpired) as error:
        raise RuntimeError(f"GitHub request failed for {endpoint}: {type(error).__name__}") from error
    if result.returncode:
        # Do not relay arbitrary CLI output or authentication diagnostics.
        raise RuntimeError(f"gh api failed for {endpoint} (exit {result.returncode})")
    try:
        return json.loads(result.stdout)
    except json.JSONDecodeError as error:
        raise RuntimeError(f"Invalid GitHub JSON for {endpoint}") from error


def collect(repo, run_id):
    base = f"repos/{repo}/actions/runs/{run_id}"
    run = api(base)
    jobs = []
    page = 1
    # Pin jobs to the observed attempt, even if another attempt starts mid-request.
    while True:
        batch = api(f"{base}/attempts/{run['run_attempt']}/jobs?per_page=100&page={page}")["jobs"]
        jobs.extend(batch)
        if len(batch) < 100:
            break
        page += 1
    return summarize(run, jobs)


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--repo", required=True, help="owner/repository")
    selection = parser.add_mutually_exclusive_group(required=True)
    selection.add_argument("--run", type=int, nargs="+", help="explicit run IDs")
    selection.add_argument("--workflow", help="workflow filename or ID")
    parser.add_argument("--limit", type=int, default=10, help="latest completed runs, including failures/skips (1–100)")
    parser.add_argument("--event", help="optional workflow event, e.g. push, pull_request, merge_group")
    args = parser.parse_args()
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", args.repo):
        parser.error("--repo must be owner/repository")
    if not 1 <= args.limit <= 100:
        parser.error("--limit must be between 1 and 100")
    if args.run and any(run_id <= 0 for run_id in args.run):
        parser.error("run IDs must be positive")
    if args.run and args.event:
        parser.error("--event requires --workflow")
    try:
        ids = args.run
        if ids is None:
            query = {"per_page": args.limit, "status": "completed"}
            if args.event:
                query["event"] = args.event
            runs = api(f"repos/{args.repo}/actions/workflows/{quote(args.workflow, safe='')}/runs?{urlencode(query)}")
            ids = [run["id"] for run in runs["workflow_runs"]]
        report = {
            "repo": args.repo, "captured_at": datetime.now(timezone.utc).isoformat(),
            "selection": {"workflow": args.workflow, "event": args.event, "run_ids": ids},
            "runs": [collect(args.repo, run_id) for run_id in ids],
        }
    except (RuntimeError, KeyError, TypeError, ValueError) as error:
        print(f"runtime: {error}", file=sys.stderr)
        return 1
    print(json.dumps(report, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
