"""Report evidence, without treating skipped/incomplete rendering as success."""
import html
import json
from pathlib import Path
import sys


def records(path):
    if not path.exists():
        return []
    rows = []
    for line in path.read_text().splitlines():
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            break  # A native fault can leave a partial final record.
    return rows


def case(directory, build_status):
    result_path = directory / "process/result.json"
    if not result_path.exists():
        return {"status": "not-run", "reason": f"build/setup: {build_status}; no child result"}
    try:
        result = json.loads(result_path.read_text())
    except json.JSONDecodeError as error:
        result = {"status": "failed", "error": f"unreadable child result: {error}"}
    stages = records(directory / "stages.jsonl")
    stage = stages[-1] if stages else {"stage": "before-first-marker"}
    samples = records(directory / "samples.jsonl")
    summaries = [row for row in samples if "median_ms" in row]
    expected = {(mode, i) for mode in ("static-redraw", "dynamic-redraw") for i in range(30)}
    observed = [(row.get("mode"), row["sample"]) for row in samples if "sample" in row]
    complete = (len(observed) == 60 and set(observed) == expected
                and len(summaries) == 2
                and {row.get("mode") for row in summaries} == {"static-redraw", "dynamic-redraw"}
                and all(row.get("samples") == 30 and row.get("pixel_checks") == "passed" for row in summaries)
                and stage.get("stage") == "complete")
    passed = result.get("status") == "passed" and result.get("exit_code") == 0 and complete
    stderr = directory / "process/stderr.log"
    return {"status": "passed" if passed else "failed", "last_stage": stage,
            "exit_code": result.get("exit_code"), "signal": result.get("signal"),
            "reason": "validated 60 measured frames" if passed else result.get("error", "incomplete or invalid measurement evidence"),
            "stderr_tail": stderr.read_text(errors="replace")[-3000:] if stderr.exists() else "",
            "timings": summaries}


def cell(value):
    return html.escape(str(value)).replace("|", "&#124;").replace("\n", " ")


def main():
    root, engine, build_status = Path(sys.argv[1]), sys.argv[2], sys.argv[3]
    root.mkdir(parents=True, exist_ok=True)
    report = {f"{workload}-{scale}": case(root / f"{engine}-{workload}-{scale}", build_status)
              for workload in ("controls", "vectors") for scale in (1, 2)}
    (root / "comparison.json").write_text(json.dumps(report, indent=2))
    print(f"### {cell(engine)} — software Vulkan fixture\n")
    print("| Case | Result | Last stage | Exit | Evidence |")
    print("|---|---|---|---|---|")
    details = []
    for name, result in report.items():
        print("| " + " | ".join(cell(value) for value in (name, result["status"],
              result.get("last_stage", {}), result.get("exit_code", "—"), result["reason"])) + " |")
        if result["status"] == "failed" and result.get("stderr_tail"):
            details.append(f"\n<details><summary>{cell(name)}: stderr evidence (last stage is not a proven cause)</summary><pre>{html.escape(result['stderr_tail'])}</pre></details>\n")
    print("".join(details))
    build_log = root / "build.log"
    if build_status != "success" and build_log.exists():
        print(f"\n<details><summary>Build failure evidence</summary><pre>{html.escape(build_log.read_text(errors='replace')[-6000:])}</pre></details>\n")
    print("\nFull logs, adapter inventory, raw timings, pixels and failure evidence are in the job artifact. Native consumer GPU coverage remains untested.")
    return int(any(row["status"] != "passed" for row in report.values()))


if __name__ == "__main__":
    sys.exit(main())
