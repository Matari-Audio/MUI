#!/usr/bin/env python3
"""One native qualification recipe for all hosted OSes; retain every failure."""
import os
from pathlib import Path
import subprocess
import sys


def main():
    output = Path("ui-results")
    output.mkdir(exist_ok=True)
    (output / "configuration.txt").write_text("\n".join(
        f"{key}={os.environ.get(key, '')}" for key in (
            "RUNNER_OS", "RUNNER_ARCH", "WGPU_BACKEND", "WGPU_DX12_COMPILER",
            "MUI_TEST_ALLOW_WARP", "VK_ICD_FILENAMES")))
    suffix = ".exe" if sys.platform == "win32" else ""
    binary = lambda name: f"target/debug/examples/{name}{suffix}"
    cases = [
        ("gpu-contract", [binary("gpu_contract"), str(output / "gpu-contract")], []),
        ("pixels", [binary("gpu_pixels"), str(output / "pixels")], []),
        ("embedded-gpu", [binary("native_editor"), "--test-input"], []),
        ("multiwindow-gpu", [binary("native_windows")], ["--renderer", "gpu"]),
        ("gallery-gpu", [f"target/debug/mui-preview{suffix}", "--test-ui"],
         ["--gallery", "--renderer", "gpu"]),
        ("effects", ["cargo", "test", "-p", "mui-vello", "--test", "effects_smoke",
                     "--features", "cpu,gpu-effects,test-support", "--locked", "--",
                     "--test-threads=1"], []),
    ]
    failed = False
    for name, command, flags in cases:
        result = subprocess.run([
            sys.executable, "tools/ci/ui.py", "--output", str(output / name),
            "--timeout", "240", *flags, "--", *command], check=False)
        failed |= result.returncode != 0
    return int(failed)


if __name__ == "__main__":
    raise SystemExit(main())
