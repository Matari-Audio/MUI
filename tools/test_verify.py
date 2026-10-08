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
    (temporary / "uname").write_text('#!/bin/sh\nprintf "%s\\n" "$VERIFY_TEST_OS"\n')
    (temporary / "uname").chmod(0o755)
    env = {**os.environ, "PATH": f"{temporary}:{os.environ['PATH']}", "VERIFY_COMMAND_LOG": str(log)}

    def commands(section, platform="Linux"):
        log.write_text("")
        subprocess.run([str(ROOT / "tools/verify.sh"), section], cwd=ROOT, env={**env, "VERIFY_TEST_OS": platform}, check=True)
        return [json.loads(line) for line in log.read_text().splitlines()]

    workflow = (ROOT / ".github/workflows/verify.yml").read_text()
    ui_workflow = (ROOT / ".github/workflows/ui.yml").read_text()
    assert "uses: ./.github/workflows/ui.yml" in workflow
    assert "needs: [gate, platforms, ui]" in workflow
    assert 'test "$UI_RESULT" = success' in workflow
    assert "cargo test -p mui-baseview --lib --locked -- --ignored --test-threads=1" in ui_workflow
    assert "--gallery --record" in ui_workflow
    assert "backend: [vulkan, gl]" in ui_workflow
    embedded = ui_workflow.split("\n  embedded:\n", 1)[1].split("\n  native-probe:\n", 1)[0]
    assert "continue-on-error" not in embedded, "embedded CPU lifecycle must be required"
    assert "windows-2025" in embedded and "macos-15" in embedded
    assert "MUI_RENDERER: cpu" in embedded and "WGPU_BACKEND: ${{ matrix.unavailable }}" in embedded
    assert "WARP compute shader compilation can crash" in embedded
    assert "target/debug/examples/native_editor" in embedded and "--timeout 60" in embedded
    assert "weston --backend=headless-backend.so" in ui_workflow and "WINIT_UNIX_BACKEND=wayland" in ui_workflow
    browser = (ROOT / ".github/workflows/playground.yml").read_text()
    assert "browser: [chromium, firefox, webkit]" in browser and "needs: [build, browser]" in browser
    assert "python3 tools/ci/playground.py" in browser
    sections = re.search(r"section: \[([^]]+)\]", workflow).group(1).replace(" ", "").split(",")
    recorded = {section: commands(section) for section in sections}
    vendor_manifests = {"vendor/moose-baseview/Cargo.toml", "vendor/xim-rs/Cargo.toml"}
    vendor_tests = [argv for argv in recorded["root-test"] if "--manifest-path" in argv]
    assert {argv[argv.index("--manifest-path") + 1] for argv in vendor_tests} == vendor_manifests, vendor_tests
    assert all("--lib" in argv for argv in vendor_tests), vendor_tests
    xim = next(argv for argv in vendor_tests if "vendor/xim-rs/Cargo.toml" in argv)
    assert xim[xim.index("--features") + 1] == "x11rb-client,x11rb-xcb", xim
    for platform in ("Darwin", "Windows_NT"):
        native = [argv for argv in commands("root-test", platform) if "--manifest-path" in argv]
        assert len(native) == 1 and "vendor/moose-baseview/Cargo.toml" in native[0], native
    for manifest in vendor_manifests:
        assert (ROOT / manifest).with_name("Cargo.lock").is_file(), manifest
        assert f"cargo fetch --manifest-path {manifest} --locked" in workflow, manifest
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
aggregate = workflow.split("\n  linux:\n", 1)[1].split("\n  ui:\n", 1)[0]
assert "needs: [gate, platforms, ui]" in aggregate and "if: always()" in aggregate
platforms = workflow.split("\n  platforms:\n", 1)[1]
assert "cargo fetch --manifest-path vendor/moose-baseview/Cargo.toml --locked" in platforms
assert "cargo test --manifest-path vendor/moose-baseview/Cargo.toml --lib --locked --offline" in platforms
assert "if: runner.os == 'macOS'\n        name: Accessibility providers in two native libraries\n        run: python3 tools/ci/check-macos-accesskit-images.py" in platforms
assert (ROOT / "tools/ci/fixtures/accesskit-images/Cargo.lock").is_file()
assert "vendor/xim-rs" not in platforms
assert "    if:" not in platforms.split("    strategy:", 1)[0], "platform jobs must run on every update"
body = textwrap.dedent(aggregate.split("        run: |\n", 1)[1])
results = ("success", "skipped", "failure", "cancelled")
for gate, platform, ui in itertools.product(results, results, results):
    env = {**os.environ, "GATE_RESULT": gate, "UI_RESULT": ui, "PLATFORM_RESULT": platform}
    passed = subprocess.run(["bash", "-e", "-c", body], env=env).returncode == 0
    assert passed == (gate == "success" and ui == "success" and platform == "success"), (gate, platform, ui)
print("verify: complete disjoint media coverage, locked/offline commands, aggregate truth table OK")
