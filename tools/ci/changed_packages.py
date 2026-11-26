#!/usr/bin/env python3
"""Cargo scope for a PR: the root-workspace packages it touched and every
package that depends on them, as `-p` arguments; `--workspace` when a shared
file changed; nothing when no root package did.

In the Matari monorepo MUI's crates are members of the monorepo's workspace,
so "the whole workspace" is `-p` for each of them instead of `--workspace`.
`--all` prints that whole-MUI scope; `--exclude NAME` drops a package from it.

    python3 tools/ci/changed_packages.py origin/main
    python3 tools/ci/changed_packages.py --all [--exclude NAME]...
"""
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]


def whole(path):
    """A change here can break any package. Prose never does."""
    if path.endswith(".md"):
        return False
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


def whole_scope(own_workspace, names, exclude=()):
    """Cargo arguments for every package of this tree, minus `exclude`."""
    if own_workspace:
        return " ".join(["--workspace", *(f"--exclude {n}" for n in exclude)])
    return " ".join(f"-p {n}" for n in sorted(set(names) - set(exclude)))


def main(argv):
    meta = json.loads(
        subprocess.run(
            ["cargo", "metadata", "--no-deps", "--format-version", "1", "--offline"], cwd=ROOT, capture_output=True, text=True, check=True
        ).stdout
    )
    own_workspace = Path(meta["workspace_root"]) == ROOT
    members = set(meta["workspace_members"])
    packages = {
        p["name"]: (
            str(Path(p["manifest_path"]).parent.relative_to(ROOT)),
            {d["name"] for d in p["dependencies"] if d.get("path")},
        )
        for p in meta["packages"]
        if p["id"] in members and Path(p["manifest_path"]).is_relative_to(ROOT)
    }
    if argv[:1] == ["--all"]:
        exclude = [argv[i + 1] for i, a in enumerate(argv) if a == "--exclude"]
        print(whole_scope(own_workspace, packages, exclude))
        return
    base = argv[0] if argv else "origin/main"
    diff = subprocess.run(["git", "diff", "--name-only", "--relative", f"{base}...HEAD"], cwd=ROOT, capture_output=True, text=True, check=True)
    names = select(diff.stdout.split(), packages)
    print(whole_scope(own_workspace, packages) if names is None else " ".join(f"-p {n}" for n in names))


if __name__ == "__main__":
    main(sys.argv[1:])
