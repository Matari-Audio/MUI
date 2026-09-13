# MUI text

Parley shapes and wraps text; Taffy allocates space; a renderer draws the resulting glyphs.
This crate adds no renderer or window dependency. Enable `mui`'s `text` feature to use it
through `mui::text`. Bundled fonts work on native and WASM. Native system-font discovery
is separately opt-in through `system-fonts`. On Linux that feature requires
`pkg-config` and Fontconfig development libraries. The bundled-font path needs neither.
This environment verified the bundled-font native/WASM paths; system-font compilation
was blocked by missing Linux development packages.

```rust,ignore
use mui::prelude::*;
use mui::text::{TextStyle, TextSystem};

let mut text = TextSystem::default(); // Keep for the editor's lifetime.
text.register_font(include_bytes!("assets/Inter.ttf").to_vec())?;
let typography = TextStyle {
    family: "Inter, sans-serif".into(),
    size: 16.0,
    line_height: 22.0,
    ..Default::default()
};
let ui = item("description")
    .text("Real text measurement, with automatic wrapping.")
    .width(240.0).pad(M).build()?;
let frame = text.resolve(&ui, &typography)?;
let paragraph = frame.paragraph("description").unwrap();
// Draw paragraph.layout() glyph runs at paragraph.frame().x/y.
```

`resolve_with` selects a TextStyle per stable item ID, once per resolution. Size, line
height, weight and family inputs are validated. Family is a Parley family-list string;
register the font bytes before resolving. Missing family/glyph coverage follows Parley's
fallback behavior, and is not a guarantee of complete script coverage. The bundled test
font is a fixture only and is not included in the library binary.

All measurements use logical pixels. Known dimensions take precedence; intrinsic width
queries use Parley's min/max content widths. Paragraphs are reflowed at final content width
after layout, including fixed-size leaves whose measurement callback may never run.
`Layout::content_frame` exposes scene-relative bounds excluding padding. Paragraph glyph
positions are relative to that content origin. Height-constrained text can overflow: clipping,
ellipsis and scrolling remain renderer/runtime policy, not silently truncated text.

Keep a TextScene's geometry and paragraphs together. A new resolve returns a new result;
failure leaves the previous result available. Re-resolve after typography, font collection,
content or layout changes. TextScene is not a cache tied to mutable Ui state. Context scratch
allocations are reused, but paragraphs are currently reshaped each resolution; incremental
shaping caches and typography theme tokens remain future work. Only family, weight, size,
line height and alignment are exposed in TextStyle in this first integration. Variable-axis
settings, rich spans, selection, IME and editing widgets are not yet wired into MUI.

The browser playground still uses canvas measurement plus SVG text. It must ship/load fonts
and render these positioned glyphs before it can claim Parley/Vello rendering parity.
