#!/usr/bin/env python3
"""Run a bounded native UI child and retain evidence, including after crashes."""
import argparse
import json
import os
from pathlib import Path
import platform
import signal
import subprocess
import time


def read_events(path):
    events = []
    if path.exists():
        for line in path.read_text(errors="replace").splitlines():
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                break  # Preserve the last complete record after abrupt termination.
    return events


def gallery_summary(events, renderer=None):
    if not events or events[0].get("event") != "run_begin":
        raise ValueError("tester did not record startup")
    header = events[0]
    expected = len(header["scenes"]) * header["variants_per_scene"]
    cases = {e["case"]: e for e in events if e.get("event") == "case_begin"}
    ended = [e for e in events if e.get("event") == "case_end"]
    if sorted(e["case"] for e in ended) != list(range(expected)):
        raise ValueError("gallery cases are missing or duplicated")
    frames = [e for e in events if e.get("event") == "frame"]
    modes = sorted({f.get("rendering_mode", "unknown") for f in frames})
    if renderer is not None and modes != [renderer]:
        raise ValueError(f"expected {renderer} presentation, observed {modes}")
    for case in range(expected):
        found = [f for f in frames if f["case"] == case]
        if sorted(f["step"] for f in found) != list(range(header["steps_per_case"])):
            raise ValueError(f"case {case} did not complete every input step")
        if not any(f["presented"] for f in found):
            raise ValueError(f"case {case} never submitted a frame")
    if events[-1] != {"event": "run_end", "completed": expected, "expected": expected}:
        raise ValueError("tester did not finish after native teardown")
    if set(cases) != set(range(expected)):
        raise ValueError("case metadata is incomplete")

    def distribution(values):
        values = sorted(values)
        return {"p50": values[(len(values) - 1) // 2],
                "p95": values[int((len(values) - 1) * .95)], "max": values[-1]}

    costs = {}
    for scene in header["scenes"]:
        selected = [f for f in frames if cases[f["case"]]["scene"] == scene]
        costs[scene] = {key: distribution([f[key] for f in selected])
                        for key in ("resolve_ms", "submit_ms")}
        costs[scene]["peak_texture_bytes"] = max(f.get("peak_texture_bytes") or 0 for f in selected)
    return {"cases": expected, "frames": len(frames), "rendering_modes": modes,
            "presented": sum(f["presented"] for f in frames), "scene_costs": costs,
            "timing_scope": "CPU resolve and submission calls; excludes journal I/O, not GPU execution or scanout"}


def verify_pixels(path, stage):
    from PIL import Image
    with Image.open(path) as image:
        x, y, width, height = stage
        region = image.convert("RGB").crop((int(x), int(y), int(x + width), int(y + height)))
        if width <= 0 or height <= 0 or x < 0 or y < 0 or x + width > image.width + 1 or y + height > image.height + 1:
            raise ValueError("specimen stage is outside the captured native window")
        if max(high - low for low, high in region.getextrema()) < 4:
            raise ValueError("native specimen stage is blank or uniform")
        return {"width": image.width, "height": image.height, "stage": stage}


def capture(output, request):
    ids = subprocess.check_output(["xdotool", "search", "--onlyvisible", "--name", "^MUI tester$"], timeout=10, text=True).splitlines()
    if len(ids) != 1 or not ids[0].isdigit():
        raise ValueError("expected exactly one visible MUI tester window")
    target = output / "screenshots" / f'{request["case"]:04d}.png'
    target.parent.mkdir(exist_ok=True)
    # Allow asynchronous software-GPU submission to reach X11 before sampling.
    for attempt in range(8):
        time.sleep(.15)
        subprocess.run(["import", "-window", ids[0], str(target)], check=True, timeout=15)
        try:
            metadata = verify_pixels(target, request["stage"])
            (target.with_suffix(".json")).write_text(json.dumps({**request, **metadata}, indent=2))
            return
        except ValueError:
            if attempt == 7:
                raise


def terminate(process):
    if process.poll() is not None:
        return
    if os.name == "nt":
        subprocess.run(["taskkill", "/PID", str(process.pid), "/T", "/F"], capture_output=True, timeout=15)
    else:
        try:
            os.killpg(process.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass  # The child can exit between poll and kill.
    process.wait(timeout=15)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--timeout", type=int, default=1200)
    parser.add_argument("--gallery", action="store_true")
    parser.add_argument("--renderer", choices=("gpu", "cpu"), help="required gallery presentation mode")
    parser.add_argument("--record", action="store_true")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ["--"] else args.command
    if not command:
        parser.error("a child command is required")
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    captures = args.gallery and platform.system() == "Linux"
    env.update(MUI_TEST_RESULTS=str(output), MUI_TEST_CAPTURE="1" if captures else "0",
               MUI_DIAGNOSTICS_DIR=str(output / "mui"), MUI_REPORTING_DISABLED="1", RUST_BACKTRACE="full")
    metadata = {"platform": platform.platform(), "machine": platform.machine(),
                "revision": env.get("GITHUB_SHA"), "command": command,
                "environment": {key: env.get(key) for key in ("WGPU_BACKEND", "MUI_RENDERER", "DISPLAY", "WINIT_X11_SCALE_FACTOR", "LIBGL_ALWAYS_SOFTWARE", "VK_ICD_FILENAMES")}}
    (output / "environment.json").write_text(json.dumps(metadata, indent=2))
    probes = {"Linux": [["glxinfo", "-B"], ["vulkaninfo", "--summary"]],
              "Windows": [["powershell", "-NoProfile", "-Command", "Get-CimInstance Win32_VideoController | Select-Object Name,DriverVersion,PNPDeviceID,AdapterRAM | ConvertTo-Json"]],
              "Darwin": [["system_profiler", "SPDisplaysDataType", "-json"]]}
    for index, probe in enumerate(probes.get(platform.system(), [])):
        with (output / f"graphics-{index}.log").open("w") as log:
            try:
                result = subprocess.run(probe, stdout=log, stderr=subprocess.STDOUT, env=env, timeout=30)
                log.write(f"\nprobe exit: {result.returncode}\n")
            except (OSError, subprocess.TimeoutExpired) as error:
                log.write(f"probe unavailable: {error}\n")
    started = time.monotonic()
    process = recorder = None
    result = {"status": "failed"}
    recorded = set()
    with (output / "stdout.log").open("w") as stdout, (output / "stderr.log").open("w") as stderr, (output / "recording.log").open("w") as recording:
        try:
            if args.record:
                recorder = subprocess.Popen(["ffmpeg", "-nostdin", "-y", "-f", "x11grab", "-video_size", "1600x1200", "-framerate", "2", "-i", env["DISPLAY"], "-c:v", "libx264", "-threads", "1", "-preset", "ultrafast", "-pix_fmt", "yuv420p", str(output / "screen.mp4")], stdout=recording, stderr=subprocess.STDOUT)
            process = subprocess.Popen(command, stdout=stdout, stderr=stderr, env=env, start_new_session=os.name != "nt")
            while process.poll() is None:
                if time.monotonic() - started > args.timeout:
                    raise TimeoutError(f"native child exceeded {args.timeout} seconds")
                if recorder is not None and recorder.poll() is not None:
                    raise RuntimeError("display recorder stopped before the UI child")
                if captures:
                    for request_file in sorted(output.glob("capture-*.json")):
                        if request_file.name in recorded:
                            continue
                        request = json.loads(request_file.read_text())
                        capture(output, request)
                        recorded.add(request_file.name)
                        request_file.with_suffix(".ack").touch()
                time.sleep(.05)
            if process.returncode:
                raise RuntimeError(f"native child returned {process.returncode}")
            if args.gallery:
                result["gallery"] = gallery_summary(read_events(output / "events.jsonl"), args.renderer)
                if captures and len(recorded) != result["gallery"]["cases"]:
                    raise ValueError("not every gallery case has a verified native screenshot")
            result["status"] = "passed"
        except Exception as error:
            result["error"] = str(error)
        finally:
            if process is not None:
                try:
                    terminate(process)
                except (OSError, subprocess.TimeoutExpired) as error:
                    result.update(status="failed", cleanup_error=str(error))
                result["exit_code"] = process.returncode
                if os.name == "nt" and process.returncode is not None:
                    result["windows_status"] = f"0x{process.returncode & 0xffffffff:08x}"
                elif process.returncode is not None and process.returncode < 0:
                    result["signal"] = signal.Signals(-process.returncode).name
            if recorder is not None:
                if recorder.poll() is None:
                    try:
                        recorder.send_signal(signal.SIGINT)
                    except ProcessLookupError:
                        pass
                try:
                    recorder.wait(timeout=15)
                except subprocess.TimeoutExpired:
                    recorder.kill()
                    recorder.wait()
            events = read_events(output / "events.jsonl")
            result.update(elapsed_seconds=time.monotonic() - started, last_event=events[-1] if events else None, screenshots=len(recorded))
            (output / "result.json").write_text(json.dumps(result, indent=2))
    print(json.dumps(result, indent=2))
    return 0 if result["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
