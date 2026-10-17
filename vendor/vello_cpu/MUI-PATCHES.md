# MUI CPU renderer vendor

Source: crates.io `vello_cpu` 0.3.0, archive SHA-256
`ed1fec424fbba6acf6cbc78bb526447eb77590d95f40419d598870fe37535212`.
Apache-2.0 OR MIT; upstream copyright headers and both licenses are retained.

Only src/, the normalized manifest (unused example targets removed), README and
licenses are imported. No build output or registry cache metadata is vendored.

Local extension: expose the existing nonzero-origin coarse bucketer for a
single-threaded `RasterizerSettings::viewport` render. Record geometry at its
original global coordinates, then crop recorded strips rather than translating
curves before flattening. This preserves coverage bits and paint sampling while
rasterizing only a dirty region. Ordinary renders keep the upstream default.
The dependency version and all transitive dependency versions remain unchanged.
