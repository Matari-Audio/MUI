# mui-cut

A small keyframe motion editor on MUI, in the spirit of HyperFrames and
Cavalry: scenes, layers, keyframes, a graph editor with bezier handles.

It has two users working on the same file:

- **a person** in the web editor (`mui-cut serve`), which draws the viewport
  with the real MUI/Vello engine compiled to WASM;
- **an agent** editing the plain-text `*.cut.json` project and checking its
  work with `mui-cut still` / `mui-cut eval` / `mui-cut render`.

The editor saves into the file and reloads when the file changes on disk, so
an agent's edit shows up live in an open editor and the person's edits land
in the file.

```sh
# from the repo root
export CARGO_TARGET_DIR=...            # optional
media/mui-cut/web/build.sh             # once: the WASM engine -> web/pkg/
cargo run --manifest-path media/Cargo.toml -p mui-cut -- \
  serve media/mui-cut/examples/demo.cut.json        # http://127.0.0.1:8740/
```

## Commands

```sh
mui-cut render PROJECT -o out.mp4|null [--scene NAME] [--mb N] [--size WxH] [--cpu]
mui-cut still  PROJECT --t 1.5 -o f.png [--scene NAME] [--size WxH] [--cpu]
mui-cut eval   PROJECT --t 1.5 [--scene NAME]     # every layer's values, JSON
mui-cut fmt    PROJECT                            # rewrite in canonical form
mui-cut serve  PROJECT [--port 8740] [--web DIR]
```

- `render` plays every scene back to back (or just `--scene`), pipes frames
  to `ffmpeg` (H.264, CRF 16) and needs it on `PATH`. `--mb N` renders N
  subframes across a 180-degree shutter and averages them in linear light
  (mui-reel's shutter). `--size` scales the whole frame. `-o null` renders
  without writing a file (ffmpeg's null muxer), for benchmarks.
- `render` and `still` draw on the GPU by default: MUI's Vello GPU renderer,
  kept alive across frames. Motion blur adds each subframe into an
  `Rgba16Float` texture (`src/shutter.wgsl`), one readback per output frame
  through a ring of three staging buffers, so the GPU draws the next frame
  while the last goes to ffmpeg. `--cpu` (or no GPU adapter) uses Vello CPU.
  1920x1080 `--mb 8` on an RX 6600: about 45 frames/s on the GPU against
  1.7 on the CPU.
- `still` is the agent's eyes: one PNG of one scene at one time, seconds from
  the scene's start.
- `eval` prints what the evaluator computes at a time, for checking numbers
  without looking at pixels.
- `fmt` validates and rewrites the file the way the editor saves it.

## The project file

```json
{
  "size": [1280, 720],
  "fps": 30.0,
  "scenes": [
    {
      "name": "shapes",
      "duration": 3.0,
      "background": "#12131a",
      "layers": [
        {
          "id": "card",
          "kind": "rect",
          "x": [
            { "t": 0.0, "v": 240.0, "interp": "bezier", "out": [0.6, 0.0] },
            { "t": 1.5, "v": 1040.0, "interp": "bezier", "in": [-0.2, 60.0], "out": [0.3, 0.0] },
            { "t": 3.0, "v": 640.0, "interp": "hold", "in": [-0.6, 0.0] }
          ],
          "y": 300.0,
          "width": 180.0,
          "height": 180.0,
          "radius": 12.0,
          "fill": "#7c6cff"
        }
      ]
    }
  ]
}
```

- **Scenes** play in order; `t` in a key is seconds from the scene's start.
  `background` defaults to `#101014`.
- **Layers** paint bottom first (later layers are on top). `id` is unique in
  its scene. `kind` is one of:

  | kind | its own fields | draws |
  | --- | --- | --- |
  | `rect`, `ellipse` | | `width` x `height`, `radius` corners |
  | `text` | `text`, `align` (`left`/`center`/`right`) | Inter outlines, a line per `\n`, the block centred on `x`, `y` |
  | `image` | `path` | a PNG relative to the project file |
  | `path` | `d` | SVG path data in pixels around `x`, `y` |
  | `duplicator` | `shape` (`rect`/`ellipse`/`path`), `d`, `layout` (`grid`/`radial`/`linear`/`path`), `along`, `orient` | `count` copies, see below |
  | `svg` | `path` | an SVG file as vectors, centred |
  | `lottie` | `path`, `speed` (1), `loop` (true) | a Lottie JSON file, centred, playing |

  See `examples/showcase.cut.json` for every one of them.
- **Properties**, each either a plain value or a key list:

  | name | default | notes |
  | --- | --- | --- |
  | `x`, `y` | 0 | the layer's centre in project pixels; also its pivot |
  | `scale` | 1 | uniform |
  | `rotation` | 0 | degrees, clockwise |
  | `opacity` | 1 | 0..1 |
  | `width`, `height` | 100 | text ignores them: it is as wide as its words |
  | `radius` | 0 | corner radius (rect, image) |
  | `fill` | `#ffffff` | `#rrggbb` or `#rrggbbaa`; keys lerp per channel |
  | `font_size` | 64 | text |
  | `weight` | 600 | text, Inter's weight axis 100..900 |
  | `line_height` | 1.2 | text, times `font_size` |
  | `tracking` | 0 | text, extra pixels after every glyph |
  | `stroke`, `stroke_width` | `#00000000`, 0 | text, path, duplicator: an outline |
  | `trim_start`, `trim_end`, `trim_offset` | 0, 1, 0 | vector kinds (all but rect, ellipse, image): each contour cut to that fraction of its length, shifted by the offset, wrapping |
  | `count` | 12 | duplicator, rounded |
  | `columns` | 4 | duplicator grid |
  | `spacing_x`, `spacing_y` | 120 | duplicator grid and line |
  | `ring_radius` | 200 | duplicator radial |
  | `path_offset` | 0 | duplicator along a path, 0..1 of its length |
  | `time` | 0 | lottie: seconds into the file at the scene's start |

  A property left out is its default, and a save leaves defaults out.

### Keys and tangents

A key is `{ "t": seconds, "v": value, "interp": ..., "in": [dt, dv], "out": [dt, dv] }`.

- `interp` shapes the segment **leaving** this key: `hold` (stay, then cut on
  the next key), `linear`, or `bezier` (the default).
- A bezier segment from key A to key B is the cubic through
  `(A.t, A.v)`, `A + A.out`, `B + B.in`, `(B.t, B.v)` in time/value space:
  handles are **offsets** from their key, in seconds and in the property's own
  units, like After Effects or Cavalry value-graph handles. `out` points
  forward (`dt >= 0`), `in` backward (`dt <= 0`).
- A missing handle is a third of the segment, flat (`dv = 0`): an ease.
  `"out": [0.6, 0.0]` on a 1.5 s segment is a long, slow ease out;
  `"out": [0.25, -120.0]` leaves the key heading 120 units down per quarter
  second, which is how a bounce is drawn.
- Handle times are clamped inside their segment, so the curve always has one
  value per time, however the handles are dragged.
- The curve passes exactly through every key; before the first key a property
  holds the first value, after the last it holds the last.
- Keys may be listed in any order; loading sorts them by time.

For colours a bezier segment uses only the handles' timing (the value offsets
have no meaning for a colour).

### Animators: per-glyph and per-copy motion

`"animators": [...]` on a `text` or `duplicator` layer moves each glyph or
copy on its own (After Effects' text animators, Cavalry's stagger). Every
number in one is keyable, and `mui-cut eval` lists the result per glyph or
copy as `fx`.

```json
{ "by": "char", "shape": "square", "ease": "linear", "order": "forward", "seed": 0,
  "start": 0, "end": 1, "offset": 0, "amount": 1, "stagger": 0,
  "x": 0, "y": 0, "scale": 1, "rotation": 0, "opacity": 1, "tracking": 0, "fill": "#ffffff00" }
```

- **Units**: `by` is `char`, `word` or `line` for text (spaces go with the
  word before them, newlines are not glyphs); a duplicator's units are its
  copies. `order` ranks them `forward`, `reverse` or `random` (a shuffle
  fixed by `seed`).
- **Selection**: unit `r` of `n` spans `[r/n, (r+1)/n]`; the range is
  `[start + offset, end + offset]`. `shape` `square` weighs a unit by how
  much of it the range covers; `ramp_up`, `ramp_down`, `triangle`, `round`
  and `smooth` weigh it by where its centre falls in the range (0 outside).
  `ease` curves that weight: `in`, `out`, `in_out`, or `step` (all or
  nothing, the typewriter's cut). `amount` multiplies it.
- **Stagger**: each rank reads the whole animator (range, amount, values)
  at `t - rank * stagger`, so one set of keys plays unit after unit.
- **Values** are what a fully selected unit becomes: `x`, `y` and `rotation`
  are added (layer pixels, degrees), `scale` and `opacity` scale towards
  their value, `tracking` adds advance after the glyph, and `fill` tints
  towards its colour by its alpha. Animators stack in order.
- **Presets** (the inspector's "+ animator"): `typewriter` (step ease,
  `start` 0 to 1, opacity 0), `cascade` (`amount` 1 to 0 with a 0.04 s
  stagger, y 40, opacity 0), `pop` (random order, scale 0).

### Duplicators

A `duplicator` draws `count` copies of a `rect`/`ellipse` (`width`,
`height`, `radius`) or of path data `d`, filled with `fill` and outlined with
`stroke`. `layout` places them around the layer's origin: `grid` (`columns`
wide, `spacing_x` by `spacing_y`, centred), `linear` (a centred row
`spacing_x` apart), `radial` (on a circle of `ring_radius`, the first at
twelve o'clock) or `path` (evenly by length along the path data `along`,
shifted by `path_offset`; a closed path spaces them all the way round).
With `"orient": true` a ring copy turns so its up points outward, and a path
copy so its +x follows the path. Animators give each copy its own offset.

### Deformers

`"deformers": [...]` on any vector kind moves every point of the finished
shapes (after copies, animators and trim), in the layer's pixels around its
origin, in order. Paths are flattened and split to about 4 px first, so a
rectangle bends too. All parameters are keyable; noise is seeded Perlin,
the same every render.

| kind | parameters (defaults) |
| --- | --- |
| `noise` | `amount` 20 px, `frequency` 0.01 per px, `speed` 1 (field drift per second), `seed` 0 |
| `twist` | `angle` 90 degrees at the centre, fading to none at `radius` 200 |
| `bend` | `angle` 90 degrees over `length` 400 px of the x axis |
| `wave` | `amplitude` 20, `wavelength` 200, `speed` 1 wavelength per second |

### SVG and Lottie

SVG files are parsed by usvg (text in them set in Inter) and Lottie files by
velato, both through their backend-agnostic `RenderSink`s rather than their
vello feature: what they draw comes out as kurbo paths and is painted by the
same MUI canvas as every other vector layer, on the CPU and the GPU alike.
A Lottie layer shows the file at `time + t * speed` seconds (keys on `time`
remap it), looping over its frames unless `"loop": false`, which holds the
last frame. Both are centred on the layer's `x`, `y` at their own size;
`scale` sizes them.

## The web editor

- **Viewport**: the scene at the playhead, drawn in a worker on an
  `OffscreenCanvas`: MUI's Vello GPU renderer on WebGPU when the browser has
  it, Vello CPU otherwise. The header names the one in use (`WebGPU`/`CPU`).
  Click a layer to select it (outlined),
  drag to move it: an animated `x`/`y` gets a key at the playhead, a plain one
  changes its value.
- **Scenes / Layers** (left): switch scene, add a scene, add a rect, ellipse
  or text, reorder or delete layers.
- **Inspector** (right): the layer's settings (text, path data, layout, file),
  then every keyable property at the playhead, as the engine lists them
  (`Cut::props`), animators and deformers each under a header with their
  choices and a remove button; "+ animator" (plain or a preset) and
  "+ deformer" add one. Type a value to set it (same rule as dragging). ◆ adds a key at the playhead, or removes the one
  there; removing the last key turns the property back into a plain value.
  Yellow ◆ = animated, filled = a key sits at the playhead.
- **Timeline**: drag the ruler to scrub; a row per layer, and for the selected
  layer a row per animated property. Drag diamonds in time (snapped to frames);
  a layer-row diamond moves every key of the layer at that time.
- **Graph**: the selected property's value over time, sampled from the WASM
  evaluator itself. Drag keys (time and value) and their handles; the
  opposite handle follows to keep the tangent smooth unless Alt is held.
  Double-click to add a key. Hold / Linear / Bezier / Reset handles act on the
  selected key.
- Keys: Space play/pause, K toggle a key on the graphed property, Delete the
  selected key, arrows step a frame, Ctrl+Z / Ctrl+Shift+Z undo / redo.

Every finished gesture is PUT to the server, which validates it, writes the
canonical JSON atomically and remembers what it wrote, so the watcher only
announces edits made by someone else.

## Layout

- `src/lib.rs`: the document (serde), `Anim::at`, `eval`, `Project::load` /
  `to_json`, `Layer::props` (every keyable property by path). Builds for
  `wasm32-unknown-unknown`; no filesystem or process.
- `src/motion.rs`: animators (selectors, stagger, presets) and deformers,
  evaluated inside `eval`.
- `src/vector.rs`: the vector kinds as kurbo paths (text through mui-text's
  shaping and outlines, duplicator layouts, SVG and Lottie sinks), trim and
  deformers, handed to MUI as one canvas per layer.
- `src/render.rs`: a frame to pixels. Each layer is a small MUI tree (a
  `block`, a `canvas` ellipse, a `text`, an image `block`) resolved by
  mui-scene and painted by `mui_vello::paint` on Vello CPU under the layer's
  affine, since MUI trees have no rotation.
- `src/gpu.rs`, `src/shutter.wgsl`: the same layers on MUI's `GpuRenderer`
  (`GpuCanvas`, shared with the web), plus `Offline`: the float shutter and
  readback ring behind `render`/`still`.
- `src/web.rs`: the wasm-bindgen handles: `Cut` (validation, samples, CPU
  frames) and `GpuView` (the WebGPU viewport).
- `src/main.rs`, `src/serve.rs`: the native CLI and the std-only local server.
- `web/`: the editor shell (HTML/CSS/JS panels around the WASM viewport);
  `worker.js` draws the viewport, one frame in flight at a time.
- `web/e2e.mjs`: the editor in headless Chrome over CDP; `E2E_BACKEND=cpu`
  runs it without WebGPU.

The keyframe curves are not `mui_motion::curve::Curve`: that type is a
normalized `0..1` phase/value shaper that clamps values, while a property
track needs unbounded values and absolute times. The `Ease::Cubic` of
`mui_motion::Keys` is the same normalized CSS form. Undo is whole-document
snapshots in the editor rather than `CurveHistory`, for the same reason.

## Known gaps

- The panels are HTML/canvas 2D, not MUI widgets; only the viewport is drawn
  by MUI/Vello.
- No zoom or pan in the timeline and graph; one property at a time in the graph.
- Images are PNG only; text is Inter only, and breaks only at `\n` (no wrap
  to `width`).
- SVG and Lottie keep solid fills and strokes, group opacity and transforms;
  a gradient draws as its average colour, and clips, masks, blend modes,
  embedded images and the even-odd fill rule are dropped (every shape fills
  non-zero).
- A text layer whose glyphs are all hidden has an empty outline in the
  viewport, so it cannot be clicked there (pick it in the layer list).
- Last writer wins if the person and an agent edit the same moment.
