#!/usr/bin/env python3
"""Keep MUI plugins on MUI's main branch: unpin, update, check, open a PR.

For each plugin repository (a folder, or a crate inside a repository):

1. a worktree off `origin/main` at `<work>/sync-<name>` (the checkout
   itself is never touched; without a remote, its HEAD, and no PR);
2. every `rev =` pin on a MUI git dependency dropped;
3. `cargo update` of the MUI crates to the latest main;
4. `cargo check --workspace --all-targets` with the repo's own toolchain;
5. green: commit, push `chore/follow-mui-main` and open a PR titled
   "Follow MUI main" (never merged here); red: the compile errors are
   reported, since they are API drift to fix, not to pin away.

    media/tools/mui-sync/mui-sync.py ../synth ../app ../relay/plugin
    media/tools/mui-sync/mui-sync.py --dry-run ../synth   # check, no push or PR
    media/tools/mui-sync/mui-sync.py --check ../synth ../app

`--check` is the guard: it exits 1 if any Cargo.toml in the given repos
pins a MUI git dependency to a `rev` (plugins follow main; the lock file
records the commit they built with).
"""
import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path

MUI_GIT = 'https://github.com/Matari-Audio/MUI'
BRANCH = 'chore/follow-mui-main'
TITLE = 'Follow MUI main'
SKIP = {'target', 'vendor', '.git', 'node_modules'}
PIN = re.compile(r'''(,\s*rev\s*=\s*"[^"]*"|rev\s*=\s*"[^"]*"\s*,\s*)''')
TRAILER = 'Co-Authored-By: Claude Opus 5.5 <noreply@anthropic.com>'
PR_BODY = '''Drops any `rev` pin on MUI crates and updates them to MUI's latest `main`, so this plugin builds against current MUI. `cargo check --workspace --all-targets` passes with the repo's toolchain.

Made by `media/tools/mui-sync` in the MUI repo.

🤖 Generated with [Claude Code](https://claude.com/claude-code)
'''


def manifests(root):
    """Every Cargo.toml under `root`, less vendored and built trees."""
    for dirpath, dirs, files in os.walk(root):
        dirs[:] = [d for d in dirs if d not in SKIP]
        if 'Cargo.toml' in files:
            yield Path(dirpath) / 'Cargo.toml'


def pins(text):
    """The MUI dependencies `text` (a Cargo.toml) pins to a rev."""
    out = []
    for line in text.splitlines():
        s = line.strip()
        if s.startswith('#') or MUI_GIT not in s:
            continue
        if re.search(r'(^|[{,])\s*rev\s*=', s.split('=', 1)[1] if '=' in s else ''):
            out.append(s.split('=', 1)[0].strip())
    return out


def unpin(text):
    """`text` with the rev pins of its MUI dependencies dropped."""
    return '\n'.join(PIN.sub('', l) if MUI_GIT in l and not l.strip().startswith('#') else l
                     for l in text.split('\n'))


def check(paths):
    bad = []
    for root in paths:
        for m in manifests(Path(root)):
            for dep in pins(m.read_text()):
                bad.append(f'{m}: `{dep}` pins MUI to a rev; follow main (media/tools/mui-sync)')
    for b in bad:
        print(b)
    return 1 if bad else 0


def run(cmd, cwd, **kw):
    env = dict(os.environ)
    env.pop('RUSTUP_TOOLCHAIN', None)  # the repo's own rust-toolchain.toml
    return subprocess.run(cmd, cwd=cwd, env=env, text=True, capture_output=True, **kw)


def git(repo, *args):
    r = run(['git', '-C', str(repo), *args], repo)
    if r.returncode:
        raise RuntimeError(f'git {" ".join(args)}: {r.stderr.strip()}')
    return r.stdout.strip()


def mui_packages(lock):
    """The `cargo update` specs of the MUI git packages in a lock: the name,
    or its full package ID when another source has a package of that name."""
    blocks = [b for b in lock.split('[[package]]')]
    count = {}
    for b in blocks:
        m = re.search(r'name = "([^"]+)"', b)
        if m:
            count[m.group(1)] = count.get(m.group(1), 0) + 1
    specs = []
    for b in blocks:
        if f'source = "git+{MUI_GIT}' in b:
            name = re.search(r'name = "([^"]+)"', b).group(1)
            version = re.search(r'version = "([^"]+)"', b).group(1)
            spec = f'git+{MUI_GIT}#{name}@{version}' if count[name] > 1 else name
            if spec not in specs:
                specs.append(spec)
    return specs


def errors(stderr, limit=40):
    """The compile errors in cargo's output, each with its location."""
    lines = stderr.splitlines()
    out = []
    for i, l in enumerate(lines):
        if l.startswith('error') or ': error' in l:
            out.append(l)
            out += [x for x in lines[i + 1:i + 3] if x.strip().startswith('-->')]
    return out[:limit] or lines[-limit:]


PATH_DEP = re.compile(r'path\s*=\s*"([^"]+)"')


def siblings(src, wt):
    """Link the path dependencies that live beside the repository (`../X`)
    beside the worktree too, so they resolve there; returns the links made."""
    made = []
    for m in manifests(wt):
        for rel in PATH_DEP.findall(m.read_text()):
            at = Path(os.path.normpath(m.parent / rel))
            if at.exists() or at.is_symlink() or at.is_relative_to(wt):
                continue
            real = Path(os.path.normpath(src / m.parent.relative_to(wt) / rel))
            if real.exists():
                at.parent.mkdir(parents=True, exist_ok=True)
                at.symlink_to(real)
                made.append(at)
    return made


def sync(target, work, dry):
    target = Path(target).resolve()
    name = target.name if target.name != 'plugin' else target.parent.name
    res = {'repo': name, 'path': str(target)}
    try:
        top = Path(git(target, 'rev-parse', '--show-toplevel'))
    except RuntimeError:
        top = None
    wt = Path(work) / f'sync-{name}'
    if wt.exists():
        if top:
            run(['git', '-C', str(top), 'worktree', 'remove', '--force', str(wt)], top)
        shutil.rmtree(wt, ignore_errors=True)
    remote = top and run(['git', '-C', str(top), 'remote', 'get-url', 'origin'], top).returncode == 0
    if remote:
        git(top, 'fetch', '-q', 'origin', 'main')
        git(top, 'worktree', 'add', '-q', '--detach', str(wt), 'origin/main')
        res['base'] = 'origin/main'
    elif top:
        git(top, 'worktree', 'add', '-q', '--detach', str(wt), 'HEAD')
        res['base'] = 'HEAD (no remote: no PR)'
    else:
        shutil.copytree(target, wt, ignore=shutil.ignore_patterns(*SKIP))
        res['base'] = 'a copy (not a git repository: no PR)'
    crate = wt / target.relative_to(top) if top else wt
    links = siblings(top or target, wt)
    ws = crate
    while not (ws / 'Cargo.lock').exists() and ws != wt:
        ws = ws.parent
    try:
        pinned = []
        for m in manifests(wt):
            text = m.read_text()
            if pins(text):
                pinned += [f'{m.relative_to(wt)}: {d}' for d in pins(text)]
                m.write_text(unpin(text))
        res['unpinned'] = pinned
        lock = ws / 'Cargo.lock'
        names = mui_packages(lock.read_text()) if lock.exists() else []
        before = lock.read_text() if lock.exists() else ''
        up = run(['cargo', 'update', *names], ws)
        if up.returncode:
            res.update(status='red', errors=errors(up.stderr))
            return res
        after = lock.read_text() if lock.exists() else ''
        res['mui'] = names
        res['updated'] = before != after
        if not pinned and before == after:
            res['status'] = 'current'
            return res
        chk = run(['cargo', 'check', '--workspace', '--all-targets', '--message-format=short'], ws)
        if chk.returncode:
            res.update(status='red', errors=errors(chk.stderr))
            return res
        res['status'] = 'green'
        if dry or not remote:
            return res
        git(wt, 'add', '-A')
        git(wt, 'commit', '-q', '-m', f'{TITLE}\n\nUnpin MUI and update it to main; cargo check passes.\n\n{TRAILER}')
        git(wt, 'push', '-q', '-f', 'origin', f'HEAD:refs/heads/{BRANCH}')
        pr = run(['gh', 'pr', 'create', '--title', TITLE, '--body', PR_BODY, '--head', BRANCH, '--base', 'main'], wt)
        res['pr'] = pr.stdout.strip() or pr.stderr.strip()
        return res
    finally:
        for link in links:
            link.unlink()
        if top:
            run(['git', '-C', str(top), 'worktree', 'remove', '--force', str(wt)], top)
        else:
            shutil.rmtree(wt, ignore_errors=True)


def main():
    p = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    p.add_argument('repos', nargs='+', type=Path)
    p.add_argument('--check', action='store_true', help='only flag rev pins (exit 1 if any)')
    p.add_argument('--dry-run', action='store_true', help='no commit, push or PR')
    p.add_argument('--work', type=Path, default=Path.home() / 'projects/mui-work')
    a = p.parse_args()
    if a.check:
        sys.exit(check(a.repos))
    results = [sync(r, a.work, a.dry_run) for r in a.repos]
    print(json.dumps(results, indent=2))
    sys.exit(0 if all(r.get('status') in ('green', 'current') for r in results) else 1)


if __name__ == '__main__':
    main()
