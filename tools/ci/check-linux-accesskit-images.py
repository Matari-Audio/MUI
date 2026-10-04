#!/usr/bin/env python3
"""Private AT-SPI bus: two unloadable provider images, plus upstream negative."""
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile


def run(target, fixture, env, *, old=False):
    subprocess.run(["cargo", "build", "--offline", "--workspace", "--manifest-path", str(fixture / "Cargo.toml")], env=env, check=True)
    images = [target / "debug" / f"libaccesskit_unix_image_{name}.so" for name in ("a", "b")]
    for mode in (["active"] if old else ["active", "idle", "blocked"]):
        with tempfile.TemporaryDirectory(prefix="mui-atspi-bus-") as directory:
            config = Path(directory) / "bus.conf"
            # No desktop activation/services: all bus names are fixture-owned.
            config.write_text(f'<busconfig><type>session</type><listen>unix:tmpdir={directory}</listen><policy context="default"><allow send_destination="*"/><allow eavesdrop="true"/><allow own="*"/></policy></busconfig>')
            bus = subprocess.Popen(["dbus-daemon", "--nofork", "--print-address=1", "--config-file=" + str(config)], stdout=subprocess.PIPE, text=True)
            try:
                address = bus.stdout.readline().strip()
                assert address, "private bus did not start"
                bus_env = dict(env, DBUS_SESSION_BUS_ADDRESS=address)
                bus_env.pop("AT_SPI_BUS_ADDRESS", None)
                command = [str(target / "debug/accesskit-unix-image-probe"), *map(str, images), mode]
                if old:
                    command.append("--old-worker")
                result = subprocess.run(command, env=bus_env, text=True, capture_output=old, timeout=30)
                if old:
                    assert result.returncode != 0, "upstream detached worker unexpectedly passed unload"
                    assert "native accessibility workers survived final close" in result.stderr, result.stderr
                    print("PASS upstream negative: provider workers survive final close", flush=True)
                else:
                    result.check_returncode()
            finally:
                bus.terminate()
                bus.wait(timeout=10)


def main():
    if not sys.platform.startswith("linux"):
        raise SystemExit("this regression requires Linux /proc and dbus-daemon")
    root = Path(__file__).resolve().parents[2]
    fixture = root / "tools/ci/fixtures/accesskit-unix-images"
    with tempfile.TemporaryDirectory(prefix="mui-accesskit-unix-images-") as directory:
        target = Path(os.environ.get("CARGO_TARGET_DIR", str(Path(directory) / "target"))).resolve()
        env = dict(os.environ, CARGO_TARGET_DIR=str(target))
        subprocess.run(["cargo", "fetch", "--locked", "--manifest-path", str(fixture / "Cargo.toml")], env=env, check=True)
        run(target, fixture, env)
        old = Path(directory) / "upstream"
        shutil.copytree(fixture, old)
        manifest = old / "Cargo.toml"
        manifest.write_text(manifest.read_text().split("[patch.crates-io]")[0])
        run(target, old, env, old=True)


if __name__ == "__main__":
    main()
