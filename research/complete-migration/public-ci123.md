# PR 123 public CI diagnosis

Evaluated head `450963f3ec359254679984b5137528a3bdf22cf7` against exact main
baseline `38fc732c9dc43b753e808a6727eef568f13754f0`.

## GPUI consumer lock regression

The independent `tools/gpui-host-probe/Cargo.lock` was omitted from the renderer
upgrade. All three native GPUI jobs stop before compilation because `--locked`
rejects that old CPU/common/glifo graph:

- Ubuntu: [job 114998076415](https://github.com/Matari-Audio/MUI/actions/runs/38305392657/job/114998076415).
- Windows: [job 114998076485](https://github.com/Matari-Audio/MUI/actions/runs/38305392657/job/114998076485).
- macOS: [job 114998076359](https://github.com/Matari-Audio/MUI/actions/runs/38305392657/job/114998076359).

Job 114998076450 is the successful classic pixel comparison, not the failed
macOS GPUI job. All three GPUI jobs pass in the exact baseline's
[renderer run 38282164438](https://github.com/Matari-Audio/MUI/actions/runs/38282164438).
The native GPUI jobs have no `continue-on-error` exemption in
`.github/workflows/renderer-bench.yml`; their failures are normal workflow
failures even though GPUI remains an experimental standalone consumer.

The fix changes exactly four required registry versions: `vello_cpu` and
`vello_common` 0.2 → 0.3, glifo 0.3 → 0.4 and fearless_simd 0.4.1 → 0.7.
PNG edges follow MUI's enabled bitmap-glyph support. GPUI's exact git revision
and all unrelated registry versions remain unchanged. No probe API or behavior
changes are required.

## Informational DX12 baseline failure

The [current Windows DX12 job 114998076328](https://github.com/Matari-Audio/MUI/actions/runs/38305392721/job/114998076328)
matches [baseline job 114921791426](https://github.com/Matari-Audio/MUI/actions/runs/38282164584/job/114921791426).
Both see Microsoft's Basic Render Driver (WARP), reject its compute path with
`WARP compute shader compilation can crash; use CPU rendering`, and present CPU
frames where this probe explicitly requires GPU frames. Their embedded-editor
and multiwindow stderr files are byte-identical. Both gallery runs complete all
150 cases and exit zero, then the evidence validator rejects the CPU mode.

`ui.yml` already defines `native-probe` with `continue-on-error: true` and
explains that standard VMs do not promise acceleration. The required Windows
CPU lifecycle jobs pass separately. Workflow definitions are unchanged by this
PR. Preserve this failed GPU evidence and its existing informational status;
no renderer policy, check, label or runner change is justified.

## Independent lock inventory

Root, media, moose-baseview and XIM locks were already validated in their own
locked gates. Full locked metadata also passes for `tools/mui-migrate` and both
accessibility image fixture workspaces. The four media adapter fixture locks
pass full locked metadata; they consume separately locked historical Git MUI
snapshots, not the changed local MUI source. Preserve those compatibility
fixtures. `tools/renderer-bench` has no committed lock and generates/captures its
resolved graph in the workflow; its completed pixel jobs already pass. The
GPUI host probe is the only stale committed local-consumer graph found.

Raw job logs, artifacts and the baseline comparison are review evidence under
the session's `scratch/public-ci123/`, outside the repository.

## Focused local validation

The corrected probe graph passes full `cargo metadata --locked --offline`,
`cargo test --locked --offline` (19 tests, including CPU alpha/channel order,
bad extents, editor reflow and material pixel reuse), `cargo build --locked
--offline`, and all six media pin guard tests. The GPUI source revision is
unchanged.

An additional strict all-target Clippy run reports five pre-existing findings:
test-only imports in the shared material fixture, constant `chunks_exact_mut`,
two collapsible conditionals, and a unit-value binding. Both affected source
files are byte-identical to main38fc; its successful release-build log also
contains the same unused-import warning. The existing GPUI workflow tests and
builds the consumer but has no Clippy step. This baseline lint debt is reported
without changing a check or silently claiming a clean strict lint result.

A separate exact-main38fc worktree reproduces the same five strict Clippy
errors. Supplemental local native screenshot capture also reports the same
`specimen stage is outside the captured native window` error on both main38fc
and the upgraded consumer in this desktop environment. Neither result supports
a speculative GPUI behavior patch. The public three-platform matrix provides
the correctly configured native capture validation after this lock repair.
