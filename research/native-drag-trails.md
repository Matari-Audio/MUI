# Native handoff motion fidelity

The adapter inbox introduced by the framework migration coalesced adjacent
`CursorMoved` events whenever their modifiers matched. Unlike hover, a held
gesture needs every position: `Driver` folds those samples into `Input::trail`,
which a freehand consumer uses to retain the path's bends. Replacing samples in
the native inbox discarded them before the Driver could record the trail.

The adapter now observes every native button edge before borrowing its handler,
including presses delivered directly rather than queued. The per-button set
survives inbox drains; releasing one button leaves the others held. Unfocus and
close clear the set. Only adjacent hover with equal modifiers coalesces. Ordered
edges remain barriers, and the queue has no arbitrary sample cap. This does not
change Driver frame folding or native resize coalescing.

The regression was reproduced against merged main
`55f6739080002af2ac8d72eec4e5c93126bd6091`. A six-point curved stroke spanning two
drains, with its press already delivered, reached the actual Driver/view consumer
as only `(35,20), (75,30)`. The fixed adapter retains all six points. The matching
BUFFR `NativeInputs` regression was run from its real source module through a
temporary test-only inclusion in this crate; that inclusion was removed before
committing. BUFFR's spectral paint uses this same Driver trail contract.

Local validation uses Rust 1.98.1, jobs 2, debug information off, and incremental
compilation off:

- Adapter library tests: 28 passed; 9 native-display tests ignored by default.
- Native reentrant callback fixture: passed on an isolated Xvfb display, using
  the production Adapter with its handler deliberately borrowed for both bursts.
- Native CPU presentation/reopen fixture: passed with GPU backends unavailable.
- Strict all-feature/all-target adapter Clippy: passed. Existing dependency-capped
  `zed-xim` lifetime warnings remain unchanged.
- BUFFR module tests: 6 passed, including 1,000 held samples, multiple buttons,
  unfocus/close resets, 100,000-sample hover/resize bursts, and edge barriers;
  strict lib/test Clippy passed with the temporary inclusion.
- Root and media locked/offline metadata, six MUI pin-guard tests, the verify
  command contract, and 14 CI tool tests passed. Dependency locks are unchanged.

Raw RED/GREEN and lint output is retained in the task scratch directory under
`native-drag-trails/`; no generated captures or machine-local inclusion paths
are committed. Public PR checks run on the existing free hosted matrix.
