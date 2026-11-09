#!/usr/bin/env python3
"""Plan conservative framework pin edits. Apply only with --apply."""
import argparse
import difflib
import re
import subprocess
import sys
import tomllib
from pathlib import Path
from check import files, identity, load_manifest, scan, ROOT


def plan(root, contract):
    report = scan(root, contract)
    blocked = [i for i in report['issues'] if i['kind'] in ('local-patch', 'custom-alias', 'unregistered-source')]
    if blocked:
        raise ValueError(f'unsafe local, alias, or unregistered source: {blocked}')
    repos = {identity(f['repository']): (name, f) for name, f in contract['family'].items()}
    owners = {package for family in contract['family'].values() for package in family['packages']}
    edits = {}
    for path in sorted(files(root, 'Cargo.toml')):
        original = path.read_text()
        parsed = tomllib.loads(original)
        lines = original.splitlines(keepends=True)
        changes = []
        def visit(value, keys):
            if not isinstance(value, dict):
                return
            package = value.get('package', keys[-1] if keys else '')
            if isinstance(package, str) and package in owners and ('path' in value or ('git' in value and identity(value['git']) not in repos)):
                raise ValueError(f'{path}: local or forked owned package {package}')
            if 'git' in value and identity(value['git']) in repos:
                family, record = repos[identity(value['git'])]
                package = value.get('package', keys[-1])
                if package not in record['packages'] or any(k in value for k in ('path', 'branch', 'tag')):
                    raise ValueError(f'{path}: unsupported declaration {keys}')
                changes.append((keys[-1], value, record['revision']))
                return
            for key, child in value.items():
                visit(child, keys + [key])
        visit(parsed, [])
        for key, value, revision in changes:
            # Match a complete single-line TOML inline table, then verify its parsed value.
            matches = []
            for index, line in enumerate(lines):
                match = re.match(r'^(\s*(?:"[^"]+"|\'[^\']+\'|[A-Za-z0-9_-]+)\s*=\s*)(\{.*\})(\s*(?:#.*)?\n?)$', line)
                if not match:
                    continue
                try:
                    item = tomllib.loads(line)['' + key]
                except (KeyError, tomllib.TOMLDecodeError):
                    continue
                if item == value:
                    matches.append((index, match))
            if len(matches) != 1:
                raise ValueError(f'{path}: declaration {key} is ambiguous or not a single-line inline table')
            index, match = matches[0]
            table = match.group(2)
            rev = re.search(r'(?<![\w-])rev\s*=\s*(["\'])([^"\']*)\1', table)
            if rev:
                table = table[:rev.start(2)] + revision + table[rev.end(2):]
            else:
                table = table[:-1].rstrip() + ', rev = "' + revision + '" }'
            lines[index] = match.group(1) + table + match.group(3)
        updated = ''.join(lines)
        tomllib.loads(updated)
        if updated != original:
            edits[path] = (original, updated)
    for entry in report['tooling']:
        path = root / entry['file']
        original, updated = edits.get(path, (path.read_text(), path.read_text()))
        lines = updated.splitlines(keepends=True)
        index = entry['line'] - 1
        line = lines[index]
        rev = re.search(r'--rev(?:=|\s+)([0-9a-f]{40})(?=\s|$)', line)
        if not rev or identity('https://github.com/Matari-Audio/moose') not in identity(line):
            raise ValueError(f'{path}:{index + 1}: unsupported cargo-moose command')
        lines[index] = line[:rev.start(1)] + contract['family']['moose']['revision'] + line[rev.end(1):]
        updated = ''.join(lines)
        if updated != original:
            edits[path] = (original, updated)
    return edits


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('repository', type=Path)
    parser.add_argument('--manifest', type=Path, default=ROOT / 'stack.toml')
    parser.add_argument('--apply', action='store_true')
    args = parser.parse_args()
    try:
        root = args.repository.resolve()
        if args.apply:
            result = subprocess.run(['git', '-C', str(root), 'status', '--porcelain', '--untracked-files=all'], check=True, capture_output=True, text=True)
            if result.stdout:
                raise ValueError('worktree must be clean before applying')
        edits = plan(root, load_manifest(args.manifest))
        for path, (old, new) in edits.items():
            print(''.join(difflib.unified_diff(old.splitlines(True), new.splitlines(True), fromfile=str(path), tofile=str(path))), end='')
        if args.apply:
            for path, (old, _) in edits.items():
                if path.read_text() != old:
                    raise ValueError(f'{path}: changed during planning')
            written = []
            try:
                for path, (old, new) in edits.items():
                    path.write_text(new)
                    written.append((path, old))
            except OSError:
                for path, old in reversed(written):
                    path.write_text(old)
                raise
        print(f'{len(edits)} files proposed' + (' and applied' if args.apply else '; dry run'))
        return 0
    except (OSError, ValueError, KeyError, subprocess.CalledProcessError, tomllib.TOMLDecodeError) as error:
        print(f'stack-update: {error}', file=sys.stderr)
        return 2

if __name__ == '__main__':
    sys.exit(main())
