Rename: `Resolver::resolve_animated(` -> `Resolver::resolve_after(` (same arguments: spec, glide, prev)
Rename: `Resolver::len()` -> `Resolver::text_runs()` (shaped text runs held)
Manual: `Resolver::is_empty()` is gone; use `r.text_runs() == 0`
Manual: `Resolver::resolve(&spec)` now returns `&ResolvedScene` (kept for the next call's memo reuse); add `.clone()` where the scene must outlive the next resolve, and drop `recycle` calls for scenes it returned
Rename: `SceneSpec::scroll_bars` map type `BTreeMap<String, f64>` -> `FxHashMap<Id, f64>` (key with `Id::runtime(key)`)
Manual: `Stroke.fill` is `Option<Fill>` (None = unset, merged per field by `Style::over`); wrap literals in `Some(..)` and read with `.and_then(|s| s.fill)`
