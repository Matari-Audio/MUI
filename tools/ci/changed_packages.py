#!/usr/bin/env python3
"""Cargo scope for a PR: the root-workspace packages it touched and every
package that depends on them, as `-p` arguments; `--workspace` when a shared
file changed; nothing when no root package did.

    python3 tools/ci/changed_packages.py origin/main
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def whole(path):
    """A change here can break any package."""
    return path in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml", "tools/verify.sh") or path.startswith(
        (".cargo/", ".github/", "vendor/")
    )


def select(files, packages):
    """`packages` maps name -> (repo-relative dir, set of workspace deps).
    Returns sorted names, or None for the whole workspace."""
    if any(whole(f) for f in files):
        return None
    hit = set()
    for f in files:
        owners = [n for n, (d, _) in packages.items() if f.startswith(d + "/")]
        if owners:
            hit.add(max(owners, key=lambda n: len(packages[n][0])))
    while True:
        more = {n for n, (_, deps) in packages.items() if n not in hit and deps & hit}
        if not more:
            return sorted(hit)
        hit |= more


def main(base):
    diff = subprocess.run(["git", "diff", "--name-only", f"{base}...HEAD"], cwd=ROOT, capture_output=True, text=True, check=True)
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout
    )
    members = set(meta["workspace_members"])
    packages = {
        p["name"]: (
            str(Path(p["manifest_path"]).parent.relative_to(ROOT)),
            {d["name"] for d in p["dependencies"] if d.get("path")},
        )
        for p in meta["packages"]
        if p["id"] in members
    }
    names = select(diff.stdout.split(), packages)
    print("--workspace" if names is None else " ".join(f"-p {n}" for n in names))


if __name__ == "__main__":
    main(sys.argv[1] if len(sys.argv) > 1 else "origin/main")
