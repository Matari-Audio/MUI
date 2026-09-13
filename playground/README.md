# MUI browser playground

A static GitHub Pages application using the actual Rust MUI layout, geometry, theme and
hover engines compiled to WebAssembly. No server or runtime Rust compiler is required.

Build from the repository root:

```sh
cargo install wasm-bindgen-cli --version 0.2.128 --locked
./tools/build-playground.sh
python3 -m http.server 8000 --directory playground
```

Open http://localhost:8000. The browser must fetch WebAssembly over HTTP(S); opening
index.html directly as a file is not supported. Build output lives in `playground/pkg`.
The matching wasm-bindgen CLI version is intentional.

## Authoring

The editor accepts a constrained Rust-like expression producing `item("id")` or
`container([...])`. See `default.mui`. It is a DSL interpreted by Rust, not arbitrary
Rust: no imports, variables, closures, loops, macros, file access or network calls.
`20px` is editor shorthand for 20 pixels; native Rust uses `20.`. Numeric literals are
nonnegative decimals. Named tokens Xs/S/M/L/Xl remain theme references.

Supported methods: children, text, on_tap, scope, id, extend_to, merge, layout, width,
height, pad, gap, round, color, hover_color, stroke, center, wrap, hoverable, position,
place, cell, span, min, max, grow, shrink, pack, columns, rows.

Layouts: Row, Column, Auto, Overlay, Grid(n). Rounding: `.round(20px)` or
`.round(Rounding::separate(20px, 12px))`. Grid tracks: Track::Fixed(n),
Track::Fraction(n), Track::Hug. Colors: Canvas, Panel, Raised, Text, Muted, Outline,
Primary1, Primary2, Primary3, optionally prefixed with Color::.

The parameter inspector comes from parsed numeric tokens, including their exact UTF-16
source ranges. It ignores numbers inside strings/comments. Sliders rewrite the source;
changing an earlier number adjusts later source ranges. Grid counts/cells/spans use
integer controls. Manual numeric input allows values beyond a slider's default range.

Theme mode, global roundness, first primary seed, preview width and a layout-bounds overlay
are global controls. Explicit `.round(...)` values override global roundness by design.
The hover implementation caches the resolved scene and recalculates colors, not layout.
Tap actions are displayed as action IDs; they do not run user callbacks.

## Persistence and limits

Code and global controls are stored under a versioned localStorage key. No draft is uploaded
to a server. This is per browser/origin; clearing site data removes it. Export/import `.mui`
files for a portable backup. Reset asks before replacing the current draft. When storage
is blocked, the UI reports it and export remains available.

Errors retain the last valid preview. Limits: 64 KiB source, 8192 tokens, 96 expression
levels, plus the engine's layout/geometry budgets. The parser never evals JavaScript.
Strings are escaped before SVG output. Long text is currently single-line: canvas supplies
real browser text widths but the preview has no paragraph wrapping, shaping controls or
font selection. Stroke/clip rendering and pointer input are a demonstration, not a complete
accessible widget toolkit. Hit testing retains the core's current rectangle/occlusion limits.

`.github/workflows/playground.yml` builds and deploys this folder through GitHub Pages
on explicit workflow dispatch. Configure repository Pages to use GitHub Actions first.
The app uses relative asset paths, including for project Pages URLs.

After building, `node tools/check-playground.mjs` exercises the compiled WASM with
stubbed text metrics. It checks the example, parameters, theme changes, hover, taps
and invalid-source rollback; it does not replace browser interaction testing.
