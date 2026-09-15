# Frozen

The TypeScript authoring layer is frozen as of 2026-09-15. It remains a
build-time reference for the existing `Item` and `SceneSpec` schemas; it has no
runtime JavaScript role and is not part of the shipped plugin.

New UI work belongs in the maintained Rust DSL (`mui::prelude`) and its generic
layout helpers. The package stays in the repository so existing examples and
downstream experiments have a readable migration reference. `tools/verify.sh`
does not build, test, or generate Rust from it. Remove the package after its
last consumer has migrated.
