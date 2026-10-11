#!/usr/bin/env python3
"""Exercise two independently linked AccessKit providers in one Cocoa process."""
import ctypes
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import _ctypes


def main():
    if sys.platform != "darwin":
        raise SystemExit("this regression requires a native macOS runner")
    if len(sys.argv) == 4 and sys.argv[1] == "--unisolated-child":
        # Restore the three original global class names in both images.
        # The second provider must fail rather than hiding the regression.
        first = ctypes.CDLL(sys.argv[2], mode=ctypes.RTLD_LOCAL)
        second = ctypes.CDLL(sys.argv[3], mode=ctypes.RTLD_LOCAL)
        first.mui_probe_open.restype = ctypes.c_void_p
        second.mui_probe_open.restype = ctypes.c_void_p
        assert first.mui_probe_open()
        assert second.mui_probe_open()
        return
    root = Path(__file__).resolve().parents[2]
    manifest = root / "tools/ci/fixtures/accesskit-images/Cargo.toml"
    # A dedicated workspace and target produce two distinct dyld images. No
    # Rust types, trait objects or ownership cross the C ABI between them.
    with tempfile.TemporaryDirectory(prefix="mui-accesskit-images-") as target:
        env = dict(os.environ, CARGO_TARGET_DIR=target)
        subprocess.run(["cargo", "fetch", "--locked", "--manifest-path", str(manifest)], cwd=root, env=env, check=True)
        subprocess.run(["cargo", "build", "--locked", "--offline", "--workspace", "--manifest-path", str(manifest)], cwd=root, env=env, check=True)
        paths = [Path(target) / "debug" / f"libmui_accesskit_image_{name}.dylib" for name in ("a", "b")]
        negative = subprocess.run([sys.executable, str(Path(__file__).resolve()), "--unisolated-child", *map(str, paths)], env=dict(env, MUI_ACCESSKIT_PROBE_UNISOLATED="1"), stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, timeout=60)
        assert negative.returncode != 0, "old global class names unexpectedly accepted two providers"
        assert "could not create new class AccessKitSubclassAssociatedObject" in negative.stderr, negative.stderr
        print("old global class names failed as expected", flush=True)
        previous = None
        for cycle in range(2):
            libraries = [ctypes.CDLL(str(path), mode=ctypes.RTLD_LOCAL) for path in paths]
            handles = []
            names = []
            for lib in libraries:
                lib.mui_probe_open.restype = ctypes.c_void_p
                lib.mui_probe_names.argtypes = [ctypes.c_void_p, ctypes.c_void_p, ctypes.c_size_t]
                lib.mui_probe_names.restype = ctypes.c_size_t
                lib.mui_probe_action.argtypes = [ctypes.c_void_p]
                lib.mui_probe_action.restype = ctypes.c_size_t
                lib.mui_probe_close.argtypes = [ctypes.c_void_p]
                lib.mui_probe_close.restype = None
                handle = lib.mui_probe_open()
                assert handle, "provider fixture did not initialize"
                handles.append(handle)
                buf = ctypes.create_string_buffer(4096)
                size = lib.mui_probe_names(handle, buf, len(buf))
                names.append(buf.raw[:size].decode().splitlines())
            assert names[0][3] != names[1][3], "dyld loaded one image twice"
            for a, b in zip(names[0][:3], names[1][:3]):
                assert a != b, f"foreign Objective-C class reused: {a}"
            for lib, handle in zip(libraries, handles):
                assert lib.mui_probe_action(handle) == 3
            # Destroy A while B remains active, then prove B's callbacks still
            # target its own provider and Rust ivars.
            libraries[0].mui_probe_close(handles[0])
            assert libraries[1].mui_probe_action(handles[1]) == 6
            libraries[1].mui_probe_close(handles[1])
            print(f"cycle {cycle + 1}: two image providers activated, updated, acted and released", flush=True)
            if previous:
                for old, new in zip(previous, names):
                    # dyld may retain images (e.g. TLS); namespace allocator's
                    # explicit occupied-marker test also runs in every open.
                    if old[:3] != new[:3] and old[3] == new[3]:
                        assert all(a != b for a, b in zip(old[:3], new[:3])), "reload reused a Rust-bearing class"
            previous = names
            for lib in libraries:
                _ctypes.dlclose(lib._handle)
        print("AccessKit same-process two-image regression passed", flush=True)


if __name__ == "__main__":
    main()
