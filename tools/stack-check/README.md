# Shared framework dependency checks

This public export contains the generic checker and updater. It contains no private product inventory or release receipts. The private coordination repository chooses the candidate, updates the complete product cohort, and verifies release evidence.

Run `python3 tools/stack-check/check.py PRODUCT --manifest STACK.toml` to inventory declarations and lockfiles. Add `--configuration CONFIG.json` to check one shipping workspace lock and its reachable Cargo metadata graph. The configuration records `target`, `feature_mode`, `features`, `root_packages`, and `lockfile`. Feature mode is `default`, `no-default`, or `all`. Cargo metadata unifies workspace features, so this graph can over-approximate a shipping build. Each actual shipping configuration still needs a separate locked build.

Run `python3 tools/stack-check/update.py PRODUCT --manifest STACK.toml` to review proposed dependency and literal cargo-moose pin edits. Add `--apply` to write them in a clean worktree. Local framework forks and unsupported declaration forms require a migration first. Resolve and commit Cargo.lock deliberately, then repeat the checks. The updater does not delete source or resolve the lockfile.

`stack.toml` is an unqualified candidate. Passing the checker does not qualify a release. The explicit registry Vello encoding exception describes an upstream shader dependency, not permission for a second MUI source. Products must pass the complete candidate manifest explicitly in CI. Pin both the public checker revision and the framework inputs by full SHA.

The reusable `.github/workflows/framework-stack.yml` runs each JSON configuration in the caller's configuration directory. Private callers use the existing Matari runner pool and may supply their dependency setup token. Public callers use GitHub-hosted runners and do not need access to private coordination data.

Run `python3 -m unittest discover -s tools/stack-check -p 'test_*.py'` for the checker and updater contract tests. This directory is exported from the coordination tools; changes must update the export and its recorded source hashes together.
