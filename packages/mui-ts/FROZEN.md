# Frozen

The TypeScript authoring layer is frozen as of 2026-09-15. It targets the
`SurfaceSpec` list API that `mui-core` no longer has; the Rust DSL
(`mui::prelude`) is the single source of truth, and it is shorter than the
TS it replaced. The package stays for reference and is not built, tested, or
shipped by `tools/verify.sh`. Delete it when nothing references it.
