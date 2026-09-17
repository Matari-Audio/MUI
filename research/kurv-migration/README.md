# KURV editor migration

A read-only audit of KURV's three editor generations and the plan to rewrite the
editor on current MUI — MUI issue #8, "the item that can still change the DSL".

Nothing in KURV or in the pinned snapshot worktree was built, edited or
committed. Every number in these files came from a `wc`/`grep`/`git diff` run
against the trees, and every claim cites a line range that was opened.

| file | what it is |
|---|---|
| [`00-migration-plan.md`](00-migration-plan.md) | **start here** — verdict tables per generation, what is kept/ported/deleted and when deletion is safe, the target crate layout and per-surface line budget, the phased PR plan, and the open questions |
| [`01-egui-editor-inventory.md`](01-egui-editor-inventory.md) | generation A, the egui reference editor: 50,898 lines over 14 surfaces, 719 absolute placements, the ~6,070-line portable core, and why the "hardcoded colours" story is false |
| [`02-gpui-shell-audit.md`](02-gpui-shell-audit.md) | generation B, the live GPUI shell: the 1,135-line `render`, the 49-field `Shell`, 374 stringly-typed slot ids, and the good half — `mui-truce` and the 58-line `shell.rs` |
| [`03-mui-snapshot-vs-main.md`](03-mui-snapshot-vs-main.md) | the pinned snapshot is an *ancestor* of MUI main, not a fork; the `Item` → `El` rename table; the five things the snapshot learned from KURV that main never got |
| [`04-overlap-and-redundancy.md`](04-overlap-and-redundancy.md) | what is implemented two to five times across the three generations — themes, gestures, curves, widgets — and which copy should survive |
| [`05-ui-quality-and-breakage.md`](05-ui-quality-and-breakage.md) | what the user actually sees: the 1,324 px layout floor, hit targets that miss their handles, three hover states and no press states, and colour outside the roles |
| [`06-mui-gaps-for-kurv.md`](06-mui-gaps-for-kurv.md) | the 24 things a synth editor needs against what MUI main can express today, ranked by how many surfaces block on each — ~1,240 new MUI lines |
