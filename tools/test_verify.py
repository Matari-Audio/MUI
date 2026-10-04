#!/usr/bin/env python3
"""Check CI package coverage and required-check failures without Cargo builds."""
import itertools
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tempfile
import textwrap
import tomllib

ROOT = Path(__file__).resolve().parents[1]

with tempfile.TemporaryDirectory() as directory:
    temporary = Path(directory)
    log = temporary / "commands.jsonl"
    # Both tools record argv so even the local all gate runs without compiling
    # or recursively invoking this check through root-lint.
    stub = f"#!{sys.executable}\n" + textwrap.dedent("""\
        import json, os, sys
        with open(os.environ["VERIFY_COMMAND_LOG"], "a") as log:
            log.write(json.dumps([os.path.basename(sys.argv[0]), *sys.argv[1:]]) + "\\n")
        """)
    for tool in ("cargo", "python3"):
        executable = temporary / tool
        executable.write_text(stub)
        executable.chmod(0o755)
    env = {**os.environ, "PATH": f"{temporary}:{os.environ['PATH']}", "VERIFY_COMMAND_LOG": str(log)}

    def commands(section):
        log.write_text("")
        subprocess.run([str(ROOT / "tools/verify.sh"), section], cwd=ROOT, env=env, check=True)
        return [json.loads(line) for line in log.read_text().splitlines()]

    workflow = (ROOT / ".github/workflows/verify.yml").read_text()
    sections = re.search(r"section: \[([^]]+)\]", workflow).group(1).replace(" ", "").split(",")
    recorded = {section: commands(section) for section in sections}
    groups = []
    for section in ("media-cut-test", "media-native-test"):
        tests = [argv for argv in recorded[section] if argv[:2] == ["cargo", "test"]]
        assert len(tests) == 1, (section, tests)
        groups.append({tests[0][index + 1] for index, arg in enumerate(tests[0]) if arg == "-p"})
    workspace = tomllib.loads((ROOT / "media/Cargo.toml").read_text())["workspace"]
    packages = {
        tomllib.loads((ROOT / "media" / member / "Cargo.toml").read_text())["package"]["name"]
        for member in workspace["members"]
    }
    assert not groups[0] & groups[1], groups
    assert groups[0] | groups[1] == packages, (groups, packages)
    for section in ("all", "media-test"):
        media_tests = [argv for argv in commands(section) if argv[:4] == ["cargo", "test", "--manifest-path", "media/Cargo.toml"]]
        assert len(media_tests) == 1 and "--workspace" in media_tests[0], media_tests
    for argv in itertools.chain.from_iterable(recorded.values()):
        if argv[0] == "cargo" and argv[1] != "fmt":
            assert {"--locked", "--offline"} <= set(argv), argv
    assert subprocess.run([str(ROOT / "tools/verify.sh"), "unknown"], capture_output=True).returncode == 2

# Execute the actual aggregate shell body against every success/skip/failure/
# cancellation combination. A skipped matrix must never satisfy required work.
aggregate = workflow.split("\n  linux:\n", 1)[1].split("\n  platforms:\n", 1)[0]
assert "needs: [gate, platforms]" in aggregate and "if: always()" in aggregate
# Scheduling and the aggregate's expectation must stay identical.
required = re.search(r"PLATFORMS_REQUIRED: \$\{\{ (.+) \}\}", aggregate).group(1)
platforms = workflow.split("\n  platforms:\n", 1)[1]
scheduled = platforms.split("    if: >-\n", 1)[1].split("    strategy:", 1)[0]
assert " ".join(scheduled.split()) == required
body = textwrap.dedent(aggregate.split("        run: |\n", 1)[1])
results = ("success", "skipped", "failure", "cancelled")
for gate, platform, required in itertools.product(results, results, (True, False)):
    env = {**os.environ, "GATE_RESULT": gate, "PLATFORM_RESULT": platform, "PLATFORMS_REQUIRED": str(required).lower()}
    passed = subprocess.run(["bash", "-e", "-c", body], env=env).returncode == 0
    assert passed == (gate == "success" and platform == ("success" if required else "skipped")), (gate, platform, required)
print("verify: complete disjoint media coverage, locked/offline commands, aggregate truth table OK")
