#!/usr/bin/env python3
"""Report framework source drift without modifying repositories."""
import argparse
import hashlib
import json
import re
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent
SHA = re.compile(r"^[0-9a-f]{40}$")
SKIP = {".git", "target", "node_modules", ".graft"}


def identity(url):
    return url.removeprefix("git+").split("?")[0].rstrip("/").removesuffix(".git").lower()


def files(root, name):
    import os
    for directory, dirs, names in os.walk(root):
        dirs[:] = sorted(d for d in dirs if d not in SKIP and not d.startswith("."))
        if name in names:
            yield Path(directory) / name


def load_manifest(path):
    data = tomllib.loads(path.read_text())
    families = data["family"]
    repos = set()
    for family in families.values():
        if not SHA.fullmatch(family["revision"]):
            raise ValueError("family revisions must be full lowercase commit SHAs")
        repo = identity(family["repository"])
        if repo in repos or not family["packages"]:
            raise ValueError("each family needs a unique repository and package list")
        repos.add(repo)
    return data


def scan(root, manifest, lockfiles=None, manifests=None):
    issues, resolved, declarations, tooling = [], [], [], []
    families = manifest["family"]
    aliases = manifest.get("aliases", {})
    owners = {}
    for name, family in families.items():
        for package in family["packages"]:
            owners.setdefault(package, []).append(name)
    repo_names = {identity(f["repository"]): n for n, f in families.items()}
    def issue(kind, **details):
        issues.append({"kind": kind, **details})
    locks = sorted(files(root, "Cargo.lock")) if lockfiles is None else lockfiles
    if not locks:
        issue("missing-lockfile")
    for lock in locks:
        graph = {}
        for package in tomllib.loads(lock.read_text()).get("package", []):
            name = package["name"]
            source = package.get("source", "path/workspace")
            canonical = aliases.get(name, name)
            upstream = any(name == x['package'] and package['version'] == x['version'] and source == 'registry+https://github.com/rust-lang/crates.io-index' for x in manifest.get('upstream_coexistence', []))
            if upstream:
                continue
            source_repo = identity(source.split("#")[0]) if source.startswith("git+") else ""
            family_name = repo_names.get(source_repo)
            if canonical not in owners and not family_name and not name.startswith(("mui-", "moose-")):
                continue
            entry = {"lock": str(lock.relative_to(root)), "package": name, "version": package["version"], "source": source}
            resolved.append(entry)
            graph.setdefault(canonical, set()).add(source)
            if name in aliases:
                issue("custom-alias", package=name, represents=canonical, lock=entry["lock"])
            if not family_name or family_name not in owners.get(canonical, []):
                issue("unregistered-source", **entry)
            elif source.rsplit("#", 1)[-1] != families[family_name]["revision"]:
                issue("revision-drift", family=family_name, **entry)
        for package, sources in graph.items():
            if len(sources) > 1:
                issue("multiple-sources", lock=str(lock.relative_to(root)), package=package, sources=sorted(sources))
    def visit(value, path, manifest_path):
        if isinstance(value, dict):
            if "git" in value and isinstance(value["git"], str) and identity(value["git"]) in repo_names:
                family_name = repo_names[identity(value["git"])]
                revision = value.get("rev")
                entry = {"file": manifest_path, "key": ".".join(path), "family": family_name, "revision": revision}
                declarations.append(entry)
                if revision != families[family_name]["revision"]:
                    issue("declaration-drift", **entry)
            if len(path) >= 2 and path[0] == "patch" and "path" in value:
                package = value.get("package", path[-1])
                if package in owners or package in aliases or identity(path[1]) in repo_names:
                    issue("local-patch", file=manifest_path, key=".".join(path), package=package, path=value["path"])
            for key, child in value.items():
                visit(child, path + [key], manifest_path)
        elif isinstance(value, list):
            for child in value:
                visit(child, path, manifest_path)
    for cargo in sorted(files(root, "Cargo.toml") if manifests is None else manifests):
        visit(tomllib.loads(cargo.read_text()), [], str(cargo.relative_to(root)))
    workflow_dir = root / ".github" / "workflows"
    if workflow_dir.is_dir():
        for workflow in sorted(workflow_dir.iterdir()):
            if workflow.suffix not in (".yml", ".yaml"):
                continue
            content = workflow.read_text()
            variables = {}
            for number, binding in enumerate(content.splitlines(), 1):
                match = re.match(r'^\s*([A-Za-z_][A-Za-z_0-9]*):\s*(.*?)\s*$', binding)
                if match:
                    variables.setdefault(match.group(1), []).append((number, match.group(2)))
            for line_number, line in enumerate(content.splitlines(), 1):
                if line.lstrip().startswith("#") or not re.search(r'\bcargo(?:\s+\+\S+)?\s+install\b.*\bcargo-moose\b', line):
                    continue
                rev = re.search(r'--rev(?:=|\s+)["\']?([^\s"\']+)', line)
                value = rev.group(1) if rev else ""
                binding = None
                variable = re.fullmatch(r'\$(?:([A-Za-z_][A-Za-z_0-9]*)|\{([A-Za-z_][A-Za-z_0-9]*)\})', value)
                if variable:
                    name = variable.group(1) or variable.group(2)
                    choices = variables.get(name, [])
                    literals = [re.fullmatch(r"(?:([0-9a-f]{40})|\"([0-9a-f]{40})\"|'([0-9a-f]{40})')", item) for _, item in choices]
                    values = {next((v for v in match.groups() if v), '') for match in literals if match}
                    value = next(iter(values)) if choices and len(literals) == len(choices) and all(literals) and len(values) == 1 else ''
                    binding = {"name": name, "lines": [number for number, _ in choices]} if value else {"name": name, "lines": []}
                git = re.search(r'--git(?:=|\s+)["\']?([^\s"\']+)', line)
                repository = git.group(1) if git else ''
                entry = {"file": str(workflow.relative_to(root)), "line": line_number, "revision": value or None, "binding": binding, "repository": repository}
                tooling.append(entry)
                if value != families["moose"]["revision"] or identity(repository) != identity(families["moose"]["repository"]):
                    issue("tooling-drift", **entry)
    return {"repository": str(root), "lockfiles": [{"path": str(p.relative_to(root)), "sha256": hashlib.sha256(p.read_bytes()).hexdigest()} for p in locks], "resolved": resolved, "declarations": declarations, "tooling": tooling, "issues": issues}


def shipping_graph(root, manifest, config):
    target, mode = config['target'], config['feature_mode']
    roots, features = config['root_packages'], config['features']
    if not isinstance(target, str) or not target or mode not in ('default', 'no-default', 'all'):
        raise ValueError('shipping configuration needs a target and valid feature_mode')
    if not isinstance(roots, list) or not roots or not all(isinstance(x, str) and x for x in roots) or len(set(roots)) != len(roots):
        raise ValueError('shipping configuration needs unique root_packages')
    if not isinstance(features, list) or not all(isinstance(x, str) and x for x in features):
        raise ValueError('shipping configuration needs a features list')
    if mode == 'all' and features:
        raise ValueError('all feature mode cannot specify features')
    lock = root / config['lockfile']
    before = lock.read_bytes()
    targets = subprocess.run(['rustc', '--print', 'target-list'], capture_output=True, text=True, check=True).stdout.splitlines()
    if target not in targets:
        raise ValueError(f'unknown target: {target}')
    command = ['cargo', 'metadata', '--format-version', '1', '--locked', '--filter-platform', target]
    if mode == 'no-default':
        command.append('--no-default-features')
    elif mode == 'all':
        command.append('--all-features')
    if features:
        command.extend(['--features', ','.join(features)])
    try:
        result = subprocess.run(command, cwd=root, capture_output=True, text=True)
    finally:
        if not lock.exists() or lock.read_bytes() != before:
            raise ValueError('cargo metadata modified the lockfile')
    if result.returncode:
        raise ValueError(f'cargo metadata failed: {result.stderr.strip()}')
    metadata = json.loads(result.stdout)
    if (Path(metadata['workspace_root']) / 'Cargo.lock').resolve() != lock.resolve():
        raise ValueError('configuration lockfile is not Cargo workspace lockfile')
    packages = {p['id']: p for p in metadata['packages']}
    nodes = {n['id']: n for n in metadata['resolve']['nodes']}
    selected = [p['id'] for p in packages.values() if p['name'] in roots and p['id'] in metadata['workspace_members']]
    if len(selected) != len(roots) or {packages[i]['name'] for i in selected} != set(roots):
        raise ValueError('shipping root package missing or ambiguous')
    reachable = set()
    while selected:
        node = selected.pop()
        if node in reachable:
            continue
        if node not in nodes or node not in packages:
            raise ValueError('metadata resolve graph is incomplete')
        reachable.add(node)
        selected.extend(dep['pkg'] for dep in nodes[node]['deps']
                        if any(kind['kind'] != 'dev' for kind in dep['dep_kinds']))
    owners = {}
    for family, details in manifest['family'].items():
        for name in details['packages']:
            owners.setdefault(name, set()).add(family)
    repos = {identity(v['repository']): k for k, v in manifest['family'].items()}
    aliases = manifest.get('aliases', {})
    issues, seen, resolved = [], {}, []
    parents = {}
    for parent in reachable:
        for dep in nodes[parent]['deps']:
            if dep['pkg'] in reachable and any(kind['kind'] != 'dev' for kind in dep['dep_kinds']):
                parents.setdefault(dep['pkg'], set()).add(parent)
    for package_id in sorted(reachable):
        package = packages[package_id]
        name, source = package['name'], package.get('source') or 'path/workspace'
        canonical = aliases.get(name, name)
        family = repos.get(identity(source.split('#')[0])) if source.startswith('git+') else None
        if canonical not in owners and not family and not name.startswith(('mui-', 'moose-')):
            continue
        upstream = any(name == x['package'] and package['version'] == x['version'] and source == 'registry+https://github.com/rust-lang/crates.io-index'
                       and parents.get(package_id) and all(packages[p]['name'] == x['parent'] and packages[p].get('source') == 'registry+https://github.com/rust-lang/crates.io-index' for p in parents[package_id])
                       for x in manifest.get('upstream_coexistence', []))
        if upstream:
            continue
        entry = {'package': name, 'source': source, 'id': package_id}
        resolved.append(entry)
        seen.setdefault(canonical, []).append(package_id)
        if name in aliases:
            issues.append({'kind': 'custom-alias', **entry})
        if family not in owners.get(canonical, set()):
            issues.append({'kind': 'unregistered-source', **entry})
        elif source.rsplit('#', 1)[-1] != manifest['family'][family]['revision']:
            issues.append({'kind': 'revision-drift', **entry})
    for name, ids in seen.items():
        if len(ids) > 1:
            issues.append({'kind': 'multiple-packages', 'package': name, 'ids': ids})
    if 'windows' in target.split('-'):
        def dependency_ids(node_id, name):
            return {dep['pkg'] for dep in nodes[node_id]['deps']
                    if dep['pkg'] in reachable and packages[dep['pkg']]['name'] == name
                    and any(kind['kind'] != 'dev' for kind in dep['dep_kinds'])}
        for hal in sorted(reachable):
            if packages[hal]['name'] != 'wgpu-hal' or 'dx12' not in nodes[hal].get('features', []):
                continue
            hal_windows = dependency_ids(hal, 'windows')
            for allocator in dependency_ids(hal, 'gpu-allocator'):
                allocator_windows = dependency_ids(allocator, 'windows')
                if hal_windows != allocator_windows or len(hal_windows) != 1:
                    issues.append({'kind': 'windows-abi-mismatch', 'wgpu_hal': hal,
                                   'gpu_allocator': allocator, 'wgpu_hal_windows': sorted(hal_windows),
                                   'gpu_allocator_windows': sorted(allocator_windows),
                                   'action': 'Deliberately align the lockfile and repeat the Windows cross-check.'})
    manifests = {Path(packages[i]['manifest_path']).resolve() for i in metadata['workspace_members']}
    manifests.add((Path(metadata['workspace_root']) / 'Cargo.toml').resolve())
    if not all(p.is_relative_to(root) for p in manifests):
        raise ValueError('workspace manifests must be inside the product')
    return {'configuration': config, 'lock_sha256': hashlib.sha256(before).hexdigest(), 'resolved': resolved, 'issues': issues}, manifests


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("repositories", nargs="+", type=Path)
    parser.add_argument("--manifest", type=Path, default=ROOT / "stack.toml")
    parser.add_argument("--configuration", type=Path, help="JSON shipping configuration")
    args = parser.parse_args()
    try:
        manifest = load_manifest(args.manifest)
        reports = []
        config = json.loads(args.configuration.read_text()) if args.configuration else None
        for repo in args.repositories:
            if not repo.is_dir():
                raise ValueError(f"repository does not exist: {repo}")
            root = repo.resolve()
            locks = None
            if config:
                lock = (root / config['lockfile']).resolve()
                if not lock.is_relative_to(root):
                    raise ValueError('configuration lockfile must be inside the product')
                locks = [lock]
            graph, manifests = shipping_graph(root, manifest, config) if config else (None, None)
            reports.append(scan(root, manifest, locks, manifests))
            if config:
                reports[-1]["shipping_graph"] = graph
        print(json.dumps({"manifest_status": manifest.get("status"), "manifest_sha256": hashlib.sha256(args.manifest.read_bytes()).hexdigest(), "repositories": reports}, indent=2))
        return int(any(report["issues"] or report.get("shipping_graph", {}).get("issues") for report in reports))
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError, tomllib.TOMLDecodeError) as error:
        print(f"stack-check: {error}", file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
