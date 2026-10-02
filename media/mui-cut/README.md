# mui-cut

A small keyframe motion editor on MUI, in the spirit of HyperFrames and
Cavalry: scenes, layers, keyframes, a graph editor with bezier handles.

It has two users working on the same file:

- **a person** in the web editor (`mui-cut serve`), which draws the viewport
  with the real MUI/Vello engine compiled to WASM;
- **an agent** editing the plain-text `*.cut.json` project and checking its
  work with `mui-cut still` / `mui-cut eval` / `mui-cut render`.

Both edit at once: `serve` keeps the project at a revision and merges edits
field by field, so an agent's edit shows up live in an open editor (even
mid-drag) and the person's edits land in the file. See "Editing together".

```sh
# from the repo root
export CARGO_TARGET_DIR=...            # optional
media/mui-cut/web/build.sh             # once: the WASM engine -> web/pkg/
cargo run --manifest-path media/Cargo.toml -p mui-cut -- \
  serve media/mui-cut/examples/demo.cut.json        # http://127.0.0.1:8740/
```

## Commands

```sh
mui-cut render PROJECT -o out.mp4|null [--scene NAME] [--mb N] [--size WxH]
                      [--renderer classic|gpu|cpu|blender] [--threads N] [--stats]
                      [--engine eevee|cycles] [--samples N]   # blender only
                      [--codec h264|h265|av1] [--encoder auto|vaapi|software]
                      [--crf N | --bitrate 12M [--maxrate 20M]] [--preset P]
                      [--pix-fmt yuv420p|yuv420p10le] [--container mp4|mkv|mov]
                      [--variant NAME | --variants all|NAME,NAME -o out/{name}.mp4]
                      [--segment SECONDS] [--range 3.2s-5.0s]
mui-cut still  PROJECT --t 1.5 -o f.png [--scene NAME] [--size WxH] [--renderer R] [--variant NAME]
mui-cut eval   PROJECT --t 1.5 [--scene NAME] [--variant NAME]   # every layer's values, JSON
mui-cut fmt    PROJECT                            # rewrite in canonical form
mui-cut serve  PROJECT [--port 8740] [--web DIR]    # plays the sound live
mui-cut midi   PROJECT --file song.mid --layer ID [--scene NAME] [--track N] [--at SECONDS]
mui-cut capture PROJECT                           # capture every missing plugin state
# for agents, see "Using mui-cut from an AI agent"
mui-cut schema | check | sheet | strip | diff | gen | mcp
```

- `render` plays every scene back to back (or just `--scene`), pipes frames
  to `ffmpeg` and needs it on `PATH`. The GPU converts each frame to BT.709
  limited-range 4:2:0 (NV12, or P010 for `yuv420p10le`, straight from the
  float shutter) in a compute pass, so the readback is 1.5 (3) bytes a pixel
  and ffmpeg converts nothing; the stream is tagged BT.709.
- Encoders: `--encoder auto` (the default) runs a 0.1 s trial encode on
  VAAPI (`/dev/dri/renderD128`) for the codec and bit depth asked for and
  uses it if it works, else software: `libx264`, `libx265`, `libsvtav1`.
  `vaapi` fails instead of falling back, `software` (or `x264`) skips the
  trial. Quality: `--crf` (software CRF, VAAPI constant QP; default 16 for
  H.264, 18 H.265, 26 AV1) or `--bitrate` with an optional `--maxrate`;
  `--preset` goes to software encoders. The container is `--container` or
  the output's extension (AV1 is refused in mov).
- Segment cache: `--segment 1` renders in 1 s spans, each encoded on its own
  (so it opens on a key frame and no frame refers outside it) and kept in
  `.mui-cut-cache/<variant>/` beside the project, named by a hash of the
  span's evaluated subframes, the encode settings, the renderer and the
  image bytes. A re-render encodes only spans whose hash changed and splices
  every chunk with ffmpeg's concat demuxer and `-c copy`; the summary says
  how many were rendered and how many cached. `--range 3.2s-5.0s` (which
  implies `--segment 1`) forces the spans it touches. Spans run across scene
  boundaries on the output timeline. Old chunks are never evicted: delete
  the directory to reclaim the space.
- A project can keep these in `"render": {"codec": "h265", "crf": 18,
  "pix_fmt": "yuv420p10le", "mb": 8, ...}` (the flag names, `pix_fmt` with
  an underscore); flags override it. `--mb N` renders N
  subframes across a 180-degree shutter and averages them in linear light
  (mui-reel's shutter). `--size` scales the whole frame. `-o null` renders
  without writing a file (ffmpeg's null muxer), for benchmarks.
- `--renderer` picks what draws: `classic` (the default: MUI's
  `GpuRenderer`, classic Vello in compute shaders), `gpu` (`vello_gpu`,
  Vello's sparse-strips renderer: strips built on the CPU, raster in plain
  render passes) or `cpu` (Vello CPU; `--cpu` is the same). No GPU adapter
  falls back to the CPU. Both GPU engines stay alive across frames; motion
  blur adds each subframe into an `Rgba16Float` texture
  (`src/shutter.wgsl`), one readback per output frame through a ring of
  three staging buffers, so the GPU draws the next frame while the last
  goes to ffmpeg. The CPU renders `--threads` output frames at once (default
  one per core), each on its own single-threaded Vello CPU, handed to ffmpeg
  in order; a `still` rasterises one frame on every core instead.
  `--stats` prints wall time per frame (p50/p95/max, the slowest frames).
- The demo at 1920x1080, `-o null`, RX 6600 + 16 threads, best of three
  while other builds loaded the box (load 20-34), frames/s:

  | renderer | `--mb 1` | `--mb 8` |
  | --- | --- | --- |
  | classic (default) | 302 | 204 |
  | gpu (`vello_gpu`) | 289 | 180 |
  | cpu, frame pool (default) | 274 | 41 |
  | cpu, one frame on 16 threads (`--threads 1`) | 159 | 7.6 |
  | cpu before this pool (serial) | 98 | 2.2 |

  `--mb 1` on the GPU is bound by the pipe to ffmpeg. The first GPU frames
  carry pipeline creation (classic: up to 1.6 s under load).
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

### Effects

A layer or a scene can carry `"effects": [...]`, run in order on the GPU:
a layer's over its own pixels before it is composited, a scene's over the
whole frame after every layer. Every parameter is a property (plain or keys);
left out, it is the default. Lengths are project pixels.

```json
"effects": [
  { "type": "blur", "radius": [{ "t": 0, "v": 0, "interp": "linear" }, { "t": 1, "v": 12 }] },
  { "type": "levels", "tint": "#ffd9a0", "tint_amount": 0.2 }
]
```

| type | parameters (default) |
| --- | --- |
| `grain` | `amount` 0.12, `size` 1.5, `seed` 0 |
| `chromatic` | `amount` 6 (pixels of red/blue split at the corners) |
| `crt` | `curvature` 0.12, `scanlines` 0.35, `line` 4, `vignette` 0.35 |
| `displace` | `amount` 16, `scale` 140, `speed` 0.5, `seed` 0 |
| `blur` | `radius` 8 (gaussian sigma) |
| `directional_blur` | `length` 40, `angle` 0 (degrees) |
| `levels` | `tint` #ffffff, `black` 0, `white` 1, `gamma` 1, `saturation` 1, `tint_amount` 0 |
| `plasma` | `color_a`, `color_b`, `scale` 120, `speed` 1: a generator |

- A **generator** (`plasma`) ignores the colours under it and draws a field
  from position and time; the layer's shape, antialiasing and opacity are its
  mask. A "shader layer" (✦ in the editor) is a rect whose stack starts with
  one.
- Effects are deterministic: noise is hashed from the pixel, the effect's
  `seed` and the output frame number, never the clock. The subframes of a
  motion-blurred frame share their frame's grain; displacement moves with
  `t`, so it blurs.
- The CPU renderer (`--cpu`, or a browser without WebGPU) draws without
  effects and says so.
- `examples/effects.cut.json` uses every one.

### Variables and variants

`variables` declares typed values; `variants` are named versions of the
project (`examples/variants.cut.json`):

```json
"variables": {
  "theme":  {"type": "enum", "options": ["dark", "light"], "value": "dark"},
  "accent": {"type": "color", "value": "#7c6cff"},
  "headline": {"type": "string", "value": "Ship it"},
  "badge":  {"type": "bool", "value": true},
  "corner": {"type": "number", "value": 32}
},
"variants": [
  {"name": "dark-wide"},
  {"name": "light-tall", "size": [1080, 1920], "vars": {"theme": "light"},
   "overrides": {"promo/title": {"font_size": 96}}}
]
```

- Any value under `scenes` (a plain value, a key's `v`, a background, an
  effect parameter, a duration) can be a binding, and `mui-cut schema`
  says so: `{"var": "accent"}`,
  `{"var": "W", "mul": 0.5, "add": 20}` (numbers; a bool is 1 or 0) or
  `{"var": "theme", "map": {"dark": "#0e0f14", "light": "#f4f1ea"}}`.
  Strings interpolate: `"text": "{headline}"`.
- `W` and `H` are the frame size, so one layout serves every aspect:
  `"x": {"var": "W", "mul": 0.5}` centres at any size.
- A variant sets `size`, `fps`, variable values (`vars`, type-checked) and
  `overrides`: JSON merged into a scene (`"promo"`), a layer
  (`"promo/title"`), or every scene or layer (`"*"`, `"*/title"`), before
  bindings resolve. An override that matches nothing is an error.
- Loading checks the defaults and every variant. `fmt` and the editor keep
  the file as written: bindings, key order and all.
- `render --variants all -o out/{name}.mp4` renders each variant in one run,
  on one GPU device resized between them.

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

### 3D scenes

`"mode": "3d"` on a scene sets its layers in a lit 3D space, drawn by
mui-stage (wgpu): each layer's 2D content is painted by the same Vello
engine into one texture atlas and composited as a plane, extruded slab or
mesh. Scenes without it render exactly as before.

- Coordinates stay project pixels: `x` right, `y` down, `z` away from the
  viewer. `rx`/`ry` pitch and yaw in degrees, `rotation` is the roll (`rz`).
  `anchor_z` moves the pivot; `extrude` gives a layer depth with its sides
  in `edge`. `cast_shadows` / `receive_shadows` (default true).
- `"overlay": true` on a layer draws it flat in screen space after the 3D
  pass and the scene's effects, as in 2D (its own effects run): captions,
  a logo. Beauty and Blender frames get it too, composited after; the
  editor's inspector ticks it. Where 3D draws flat, overlays go on top.
- `camera` layer (the last visible one shoots): orbits its target by `rx`,
  `ry` at `distance`, `fov` (vertical degrees), `rotation` rolls it, `dolly`
  moves it toward the target, `path` (SVG path data, moved along by
  `path_offset`) carries it in x/z, `look_at` aims it at a layer from where
  its own `x`, `y`, `z` put it (no orbit), `focus` and
  `aperture` add depth of field. Without one, the default camera sees the
  z=0 plane as the 2D frame. Every property is keyable like any other.
- `light` layers, `"type"`: `directional` (default), `spot`, `point`,
  `ambient`; `fill` is the colour, `intensity` times `opacity` the strength,
  `rx`/`ry` aim it, `cone`/`feather` shape a spot, `range` fades point and
  spot lights, `softness` widens the shadow filter. Directional and spot
  lights cast PCF shadow maps, point lights a cube of six. With no light at all a 3D scene is unlit and
  faces show their exact pixels.
- `model` layers: a `.glb` file (`path`), fitted to `height` times `scale`
  (its rest pose), tinted by `fill`, lit with its materials' base colour,
  metallic and roughness and their PNG maps (base colour, normal,
  metallic-roughness). Its first animation (node transforms and skins)
  plays at `time + t` seconds, held before its first key and after its
  last; key `time` to scrub it. See `examples/stage3d-overlay.cut.json`.
- Scene `ground` (`y`, `color`, `radius`, `reflect`, `contact` shadow
  strength) and `fog` (`color`, `near`, `far`); `background` is the clear.
- A scene's `effects` run on the 3D pass's output, per subframe, before
  motion blur averages them, as in 2D. A layer's own `effects` run on its
  slab: its box in the atlas gets room each side for what the stack
  spreads (3 x a blur's radius, half a directional blur, a displacement or
  chromatic amount), runs the stack there, and the slab grows to show it.
- Motion blur re-renders the 3D pass per subframe. The web editor draws
  3D on WebGPU; WebGL2 and the CPU draw 3D scenes flat with a notice, and
  an export refuses to. **Orbit** in the header swings the preview camera
  (drag, wheel to zoom) without touching the file.

See `examples/stage3d.cut.json`.

#### Beauty renders in mui-stage

`--quality beauty` (or `--samples N`) on `render` and `still` draws each 3D
frame as the mean of N samples (64 by default) through the same shutter as
motion blur. Each sample moves the pixel by a subpixel offset, the eye across
a thin lens (depth of field from the camera's `focus` and `aperture`), every
shadowing light across its area (the sizes Blender gives the same
`softness`) and turns the occlusion's slices, from a fixed Halton sequence,
so renders repeat exactly. With `--mb N` the samples are spread across the
shutter instead of drawn N times per subframe. At 1080p on an RX 6600 the
stage example takes ~135 ms a frame at 64 samples, against ~4 ms normal and
~870 ms in EEVEE. **Beauty** in the editor's header does the same on
WebGPU while paused: every draw folds in one more sample (up to 256), and
any change or playing goes back to normal frames.

#### Beauty renders in Blender

`--renderer blender` (on `render` and `still`) draws a 3D scene in Blender
instead of mui-stage, for final frames with raytraced EEVEE (or Cycles):
soft shadows, AO, bevelled slab edges. mui-cut writes a scene description
and runs the `blender` binary headless as a separate process (Blender 4.2 or
later; `MUI_CUT_BLENDER` names another binary). Nothing from Blender is
linked. `--engine eevee|cycles` (default eevee), `--samples N` (default 64
for EEVEE, 128 for Cycles). `--mb N` becomes Blender's own motion blur.
Frames go through the usual encode path (codecs, segments, `--stats`).

It maps the camera (DOF, `look_at`, `path`), sun/spot/point lights,
ambient as the world, layers and plugin parts as PNG-textured extruded
slabs, `.glb` models through Blender's glTF importer (their maps as it
reads them, their first animation's NLA strip driven to each frame's
`time + t`), the ground as a plane
and fog as a mist pass. Layer textures, the `.blend` and every frame are
cached by content hash under `.mui-cut-cache/blender/` next to the project;
a re-run renders only frames whose content changed. A 2D scene or a
missing Blender is an error. 1280x720 at 64 samples takes about 1-2 s per
frame on an RX 6600, plus Blender's startup.

### Sources

`sources` lists what was imported into the project, for the editor's
Sources panel: files (`image`, `svg`, `lottie`, `model`, `font`, `audio`,
`path` relative to the project) and plugins (`source` as a plugin layer names it).
A text layer's `font` names a font source by id (or a `.ttf`/`.otf` path);
left out, it is Inter. Every renderer draws it, Blender's textures too.

```json
"sources": [
  { "id": "logo", "kind": "svg", "path": "media/logo.svg" },
  { "id": "Serif", "kind": "font", "path": "media/Serif.otf" },
  { "id": "synth", "kind": "plugin", "source": { "cargo": "../Cargo.toml", "example": "synth" } }
]
```

The panel also shows the files and plugins layers use without listing them
(greyed). A plugin is a folder of its parts, from its fresh capture two
levels deep (`plugin::home`, which `serve` captures): panels, and the
controls in them under their panel, the same tree `plugin_parts` and
`cutParts` return. A bare plugin layer added from the panel starts in that
capture (`explode_levels` 2). An audio source (wav, mp3, flac, ogg, m4a,
anything ffmpeg decodes) dragged in makes an `audio` layer.

### Adding a plugin

```sh
mui-cut add ../KORREKT --project demo.cut.json     # a folder
mui-cut add https://github.com/Matari-Audio/KURV   # or a git URL (cloned to the cache)
```

`add` (the editor's **+ Plugin**, MCP `plugin_add`) needs no code in the
plugin. It reads the crate with `cargo metadata` (a workspace's one plugin
package is found), tells its framework from its dependencies, finds where
its editor is made, writes `{"id": package, "kind": "plugin", "source":
{"plugin": "../KORREKT"}}` into `sources`, and builds and captures it: the
Sources panel shows its part tree, named by the editor's surface ids.

The adapter is generated (`src/build.rs`, `src/adapter/`) under the cache
(`$MUI_CUT_CACHE`, else `~/.cache/mui-cut`): a small workspace that depends
on the plugin by path, opens its real editor as a host would, and serves it
headless (`mui::host::headless`, `mui_motion_bridge::run_headless`).
It builds against **this** MUI tree: `--config
patch."https://github.com/Matari-Audio/MUI".<crate>.path=…` for every MUI
crate the plugin uses (its dependencies and its `Cargo.lock`). It copies the
plugin's lock (every other crate stays at the plugin's version), its
toolchain file and its non-MUI `[patch]`es; the plugin's checkout, lock
included, is only read.

| Framework | What it finds | Needs in the plugin |
|---|---|---|
| moose, truce | `moose::plugin!` / `truce::plugin!` (its `crate::Plugin`); `editor()` opens a `MuiEditor` | nothing |
| nice-plug | the type `nice_export_clap!(T)` names | `T` public at the crate root (`pub use …::T;`) |
| plain MUI | `pub fn mui_editor() -> (Ui, (u32, u32), impl mui::host::View + Send + 'static)` | that one function; for sound, `pub fn mui_audio(sample_rate: f32) -> impl FnMut(&[(u8, u8)], &mut [[f32; 2]]) + Send + 'static` (a span's notes, `(key, velocity)` with velocity 0 a note off, at its start; it fills stereo frames) |

Every framework's editor opens its window through `mui_baseview::open`,
which hands the view to a claiming headless host instead. A library that
is not `rlib`, a missing `plugin!`, a private nice-plug type or a missing
`mui_editor` is reported as the exact line to add. Params are `set` by
parameter id or name, `field` `norm` (0..1) or `value` (plain units). The
plugin's DSP runs as a host runs it (moose and truce `process`, nice-plug
`activate` then `process`, a plain crate's `mui_audio`), on the manual
clock, so every generated adapter sounds. A layer's `preset` (a moose
host state, or a plugin preset file wrapping one, like KURV's `.kurvy`)
loads on frame 0. A git source is pulled to its latest by `add`.

Plugins follow MUI's main branch. `media/tools/mui-sync` unpins MUI in a
plugin repo, updates it, checks it and opens a "Follow MUI main" PR;
`mui-sync.py --check REPO…` fails on any `rev =` pin of a MUI crate, and
`add` lists the ones it sees.

### Parenting

`"parent": "card"` attaches a layer to another in its scene, Cavalry and
After Effects style: its `x`, `y` (and `z` in 3D) are offsets in the
parent's space, turned and scaled with it; rotation and scale add up and
opacity multiplies. In 3D the parent's space is its slab's: moved, turned
by `ry`, `rx` and `rotation`, scaled, with its face `anchor_z` in front of
the pivot, so a child rides a tilted parent (its world `rx`, `ry` and
`rotation` are the composed turn split back out; a light or camera's aim
turns with its parent). `eval` composes it, so every renderer (2D, 3D,
Blender), `check` and the editor see world values. A parent that is
missing, or a chain that loops, is a load error. Reparenting in the
editor or with MCP `layer_parent` keeps the layer where it is on screen at
the playhead by rewriting its local keys through both parents' transforms,
tilts included. Where the change mixes axes (a turned parent moves x into
y), tracks keyed at different times are keyed at all of them first, so
every key stays exact. Reparent, reset and the 2D switch run in every
variant: what they change is written resolved (into the file, and into a
variant's `scene/layer` override where its result differs), and bindings
they do not touch stay.

### Plugin layers

A `plugin` layer films a running MUI plugin editor: its real UI, split into
parts you can move, key, highlight and explode.

```json
{ "id": "synth", "kind": "plugin",
  "source": { "cargo": "../Cargo.toml", "example": "synth" },
  "params": [{ "id": "filter", "field": "cutoff", "value": [{ "t": 0.8, "v": 0.6 }, { "t": 2.2, "v": 0.95 }] }],
  "pointer_x": 541.3, "pointer_y": 331.5, "pointer_down": 0,
  "explode": [{ "t": 3.9, "v": 0 }, { "t": 4.8, "v": 0.45 }],
  "parts": { "filter": { "y": -40, "z": -120, "scale": 1.25, "highlight": 1 } } }
```

- `source`: the editor as a mui-motion-bridge live adapter. A plugin
  crate, `{"plugin": folder or git URL}` (see "Adding a plugin"), a
  prebuilt executable, `{"bin": path}`, or one built from source,
  `{"cargo": Cargo.toml, "example"|"bin": name}`. Paths are relative to the
  project. `args` are passed on. Any adapter that speaks the bridge's live
  protocol works; `examples/synth.rs` is a small one (a synth panel of
  real MUI knobs, sliders, a toggle and a meter).
- `params`: keyed values sent as `{"op": "set", "id", "field", "value"}`
  whenever they change on the frame grid. `pointer_x`/`pointer_y` (UI
  pixels; -1 is off the UI) and `pointer_down` (>= 0.5 is pressed) drive
  the pointer, so a keyed drag turns a knob the way a hand would. `select`
  names the surface ids to split into parts; empty lets the bridge
  discover them, `explode_levels` deep.
- Parts form a tree: a panel's controls are its children, addressed by
  path, `panel/control`. A control takes its whole widget (dial, caption,
  value). A panel keeps the paint its children leave behind, whole, and a
  child moved off its panel draws free of the panel's clip.
- `parts.<path>`: each part's own `x`, `y`, `scale`, `rotation`,
  `opacity` and `highlight` (a flat light outline), in the UI's pixels,
  plus `z` (depth, larger is farther), relative to its parent part. All
  of them are keyable, as `parts.filter/filter-cutoff.x` etc.
- `explode` (0..1) pulls every part away from its parent's centre by that
  fraction of its distance, and `explode × 160` UI pixels towards the
  viewer. `explode_levels` (1..8, default 1) is how many levels come
  apart; level n runs `explode_stagger` seconds behind level n-1, so
  panels separate first and their controls after. `backdrop` fades what
  is not a part.
- Depth only shows in a 3D scene (`"mode": "3d"`). There, each part is its
  own slab, placed where the 2D drawing puts it and turned with the layer,
  at `explode` depth plus its `z`. `extrude` on the layer gives every part
  a thickness. A 2D scene draws the same parts flat.
- Captures: the CLI (`still`, `render`, `sheet`, `strip`, `diff`, `check`,
  MCP) runs the adapter once for each state it does not have yet. A state
  is the source plus every command sent so far, so its key only changes
  when what the adapter was told changes. Each state is stored as
  `.cut-cache/<key>.json` next to the project, with its images named by
  content under `.cut-cache/img/`. Drawing then only reads files: `eval`
  names a state and the renderer looks it up, which keeps renders
  deterministic and fast. A rebuilt adapter (another size or mtime)
  recaptures. The segment cache hashes the manifests, so a recapture
  re-renders the spans it touches.
- In process: `render` and `still` of a project with a plugin built from
  source (`{"plugin": ...}`) build its adapter with the bridge's `cut`
  feature and run the whole command inside it (`mui_cut::inproc`). The
  first layer of that plugin replays its states on the sample clock right
  there, and its parts draw from the editor's paint as vectors, at
  whatever size the camera needs: nothing is captured, no PNGs, about 40
  times faster. A headless editor animates on that clock
  (`mui::host::headless::time`), not the wall's. `MUI_CUT_CAPTURE=1` keeps
  the capture path; `serve` and the web editor always capture (a browser
  cannot link the plugin).
- `mui-cut serve` captures missing states when the file changes and tells
  the editor (SSE `plugin`), which reloads them. In the web editor a
  plugin layer lists its parts as child layers. Pick one (in the list or
  the viewport) to inspect, drag and key it; nested parts sit indented
  under their panel. **Explode / collapse** keys `explode` at the
  playhead, and the inspector sets `explode levels` and `level stagger`.
  **Interact** turns viewport clicks into the UI's pointer: a drag keys
  `pointer_x`/`pointer_y`/`pointer_down` from the playhead, so it turns
  the knob instead of moving the part. In a 3D scene the pointer's ray
  from the camera (the orbit preview's, when on) meets the nearest part
  slab as drawn, tilted, parented and exploded (`pick` on the WASM `Cut`,
  `src/pick.rs`), and a drag stays on that slab's plane past its edge.
- MCP `plugin_parts` (and `cutParts(layer)` in the editor, `plugin_parts`
  on the WASM `Cut`) returns the part tree: each part's `id` (its path),
  `surface`, `level`, `frame`, `rects`, `thumb` image, `motion` and
  `children`, plus every surface with its frame, for aiming the pointer
  and `select`.

- `show: ["osc"]` makes a **component layer**: only those parts (and any
  nested under them, `panel/knob` under `panel`) are drawn, no backdrop,
  each where the whole UI puts it. Its outline is their captured rects.
  Dragging a part in from the Sources panel makes one.

- `view_width`/`view_height` (keyable, 0 is the plugin's own size) lay
  the editor out at that size, the way a host window resize would: the UI
  reflows rather than scaling, and each size is its own capture.

### Notes, sound and the patch

A plugin layer's `notes` (`{"t", "dur", "pitch", "vel"}`, seconds from the
scene's start, MIDI pitch, vel default 100) are played into the plugin:
its UI is captured following them (a lit key, a moving meter), and its
DSP renders them into the soundtrack. Notes go to the adapter with the
frame's `advance` on its manual clock, each at its sample, so the sound
is the same on every run and at any frame rate, and the segment keys
change with the notes. `mui-cut midi` reads a `.mid` file's notes (with
its tempo map) into a layer.

An `audio` layer plays a file (`path`, `time` seconds into it at the
scene's start, keyable to remap time) at `volume` (keyable, also on
plugin layers). It draws nothing in the viewport; the timeline shows its
waveform, and a plugin layer's notes.

`render` mixes every audio and plugin layer at the project's
`sample_rate` (default 48000) and muxes it into the video as AAC; a
project with no sound writes a video with no audio track, as before. Each state's
sound is kept beside its capture (`.cut-cache/audio/`).

A `patch` layer (`"of": "<plugin layer id>"`, `width` x `height`) draws
the plugin's patch as it plays: the parameters off their defaults and the
modulation routes, from the adapter's snapshot (`patch`, see
`HOST-PROTOCOL.md`).

`mui-cut serve` plays the sound live while the editor plays: the adapter
renders ahead of a cpal output stream at the project rate (256-frame
buffers), and the editor's playhead follows that audio clock (`GET
/transport`), so picture and sound stay in step. The header shows the
device and the latency; measured from a live note to its first sample
leaving, 16 to 25 ms (the bound is 26.7 ms at 48 kHz).
`MUI_CUT_AUDIO=null`, or no output device, runs the same clock on a null
device. **Keys** plays the selected plugin layer from the computer
keyboard (`a w s e d f t g y h u j k`, Z/X for the octave), the on-screen
keys or any Web MIDI input, and records what it plays into the layer's
`notes` while the timeline plays. The viewport shows the live UI while
it plays.

The adapter's protocol, for writing one for another plugin, is in
`HOST-PROTOCOL.md`. A moose plugin's generated adapter runs its real DSP
too: the host's notes on the sample clock, meters and transport shared with
the editor, and a `patch` of the parameters off their defaults and the notes
held. A plugin source's `features` turn on the plugin crate's Cargo features
for it (KURV sounds in its `process-lab` build). `examples/kurv.cut.json`
(KURV checked out beside this repository) explodes KURV two
levels deep in 3D while it plays a melody, resizes it, and shows its
patch.

See `examples/plugin.cut.json`: a flat scene (a keyed knob, a knob dragged
by a keyed pointer, then exploded and highlighted) and a 3D one (the UI
exploded into depth under an orbiting camera). `examples/deep.cut.json`
explodes two levels in 3D: panels, then their controls.

## The web editor

- **Viewport**: the scene at the playhead, drawn in a worker on an
  `OffscreenCanvas`, by the first that opens of: WebGPU (classic Vello),
  WebGL2 (`vello_gpu`; classic Vello needs compute shaders), Vello CPU (a
  SIMD128 build). The header names the API (`WebGPU`/`WebGL2`/`CPU`), its
  tooltip the engine. `?renderer=classic|gpu|webgl2|cpu` forces one.
- **Playback** runs on a monotonic clock from the moment play (or a seek)
  started, sampled for when the frame will be shown, so a slow draw drops
  frames rather than slowing time; only the newest frame is ever drawn.
  **fps lock** plays only the project's frame grid; **HUD** shows delivered
  frames/s, dropped frames and frame time p50/p95.
  Click a layer to select it (outlined),
  drag to move it: an animated `x`/`y` gets a key at the playhead, a plain one
  changes its value.
- **Sources** (left, collapsible): every source the project uses or has
  imported, with thumbnails. **Import** (or drop PNG, SVG, Lottie JSON,
  `.glb` or `.ttf`/`.otf` files on the left panel) copies files to `media/` beside the
  project; **+ Plugin** adds a plugin crate by folder or git URL (or, under its fold, a Cargo example/bin or a prebuilt adapter). A
  plugin opens (▸) into its part tree. Drag a source, or any part, onto
  the viewport or the layer list to make a layer (a font makes a text
  layer in it; the inspector's `font` picks one); a part becomes a
  component layer (`show`) at its spot in the plugin (on the scene's
  whole plugin layer, if there is one). Clicking a row selects the layer
  or part that shows it, and selecting in the viewport highlights the row.
- **Scenes / Layers** (left): switch scene, add a scene, add a rect, ellipse
  or text, reorder or delete layers. Children nest under their parents:
  drag a layer onto another to parent it, onto the list's empty space to
  unparent it (or pick `parent` in the inspector); either keeps it where
  it is. Deleting a parent hands its children to its own parent.
- **Reset to default** (inspector): a part back where the plugin puts it;
  a layer's transform keys and offsets cleared (to the frame's middle, or
  onto its parent), and a plugin's explode and part offsets. Undoable.
- **Scene mode** (scene inspector) switches 2D and 3D and keeps the layout:
  into 3D nothing moves (the default camera sees the z = 0 plane as the 2D
  frame); back to 2D, each layer goes where the camera shows it at the
  playhead, scaled by its distance (`place::flatten`).
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
- **Export** (header): renders every scene at project size in the viewport's
  worker and saves an MP4. On WebGPU each frame goes through the same shutter
  as `render --mb` (motion-blur samples in the dialog), effects included; on
  the CPU, no effects or blur. A WebCodecs `VideoEncoder` encodes it (H.264,
  H.265 or AV1, whichever `isConfigSupported` accepts at the project's size,
  hardware preferred) and `web/mp4.js`, a small muxer, writes ftyp, moov
  first, then one mdat. The sound is `serve`'s mix (`GET /mix.wav`, plugin
  layers captured first), encoded by an `AudioEncoder` (AAC, else Opus) into
  a second track. Progress and Cancel in the dialog.
- **Variants** (header): with `variants` in the file, a switcher previews any
  of them at its size. The scene inspector lists the variables as the chosen
  variant sets them (a select for an enum, a checkbox for a bool); an edit
  goes to that variant's `vars`, or to the declared value under "defaults".
  A bound property shows its value, read-only, and names the variable.
  Export renders the variant on screen.
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
- `src/gpu.rs`, `src/shutter.wgsl`: the same layers on a GPU engine behind
  `GpuCanvas` (shared with the web), plus `Offline`: the float shutter and
  readback ring behind `render`/`still`.
- `src/sparse.rs`: `mui_vello::Canvas` over `vello_gpu`. It comes from
  Vello git main (crates.io still ships it as `vello_hybrid` on wgpu 29)
  with its own `vello_common`: mui-vello cannot move to git main's without
  source changes, and kurbo/peniko are shared, so only brushes and image ids
  are converted.
- `src/three.rs`, `src/gpu3d.rs`: 3D scenes: camera and light evaluation
  and glTF import; the atlas and mui-stage shot a frame becomes.
- `src/plugin.rs`, `src/host.rs`: plugin layers. `plugin.rs` holds state
  keys, explode and the capture manifest, and builds for wasm. `host.rs`
  is the CLI's side: it builds or finds the adapter, replays commands and
  writes the cache.
- `src/sources.rs`: the project's `sources` and `Project::all_sources`.
  `src/place.rs`: parenting (`compose` in `eval`, `reparent`), reset to
  default and `flatten` (3D to 2D through the camera).
- `src/pool.rs`: the frame-parallel CPU export.
- `src/fx.rs`, `src/fx/`: the effect schema (`EFFECTS`), evaluation, and
  the GPU passes: one WGSL file per effect after a shared `prelude.wgsl`,
  ping-ponged between textures behind `GpuCanvas::draw`.
- `src/vars.rs`: variables, variants and binding resolution, on the JSON
  before it becomes a `Project`.
- `src/encode.rs`: the ffmpeg encoder plan (codec, VAAPI trial, container).
- `src/segments.rs`: the segment cache's keys, range and concat list.
- `src/web.rs`: the wasm-bindgen handles: `Cut` (validation, samples, CPU
  frames) and `GpuView` (the WebGPU/WebGL2 viewport).
- `src/main.rs`, `src/serve.rs`: the native CLI and the std-only local server.
- `src/check.rs`: `check`'s lints, pure apart from a CPU `Renderer` for
  contrast (so the web editor could run them too).
- `src/tools.rs`, `src/mcp.rs`, `src/script.rs`: the agent's CLI pictures
  (`sheet`, `strip`, `diff`), the MCP server and `gen`'s Rhai sandbox.
- `web/`: the editor shell (HTML/CSS/JS panels around the WASM viewport);
  `worker.js` draws the viewport, one frame in flight at a time, and runs
  exports; `mp4.js` muxes them.
- `src/shutter.rs`: the float shutter, shared by `Offline` and the export.
- `web/e2e.mjs`: the editor in headless Chrome over CDP, and its playback
  pacing (frame gap mean and deviation, free and fps-locked).
  `E2E_BACKEND=webgl2|cpu` runs it without WebGPU, `E2E_RENDERER` forces
  the renderer, `E2E_PORT`/`E2E_CDP_PORT` move it off 8790/9339.

## Using mui-cut from an AI agent

The project is plain JSON, so an agent can edit it with any tool; these make
it fast to get right and to see. [AGENT-GUIDE.md](AGENT-GUIDE.md) is the
short version for an agent: the loop, a cheat sheet, the gotchas and the
friction log they came from.

```sh
mui-cut schema > cut.schema.json    # JSON Schema from the Rust types, doc comments included
mui-cut check  PROJECT [--json]     # lints, each with a JSON path, times and a fix; exit 1 on errors
mui-cut sheet  PROJECT [-o OUT.png] [--scene NAME] [--n 8] [--times 0,1.5] [--width 1600] [--cols 4]
mui-cut strip  PROJECT --layer ID [-o OUT.png] [--scene NAME] [--n 8] [--width 1600]
mui-cut diff   A B [-o OUT.png] [--n 6] [--width 1600]
mui-cut gen    SCRIPT.rhai [-o OUT.cut.json] [--seed N] [--into PROJECT [--scene NAME]]
mui-cut mcp    [PROJECT]            # Model Context Protocol server on stdio
```

The pictures take `--renderer classic|gpu|cpu` like `still`, default to
`PROJECT` with `.sheet.png` / `.ID.strip.png` / `.diff.png` next to it, and
are sized to be read by a model (1600 px wide).

- **Schema and load errors.** A file may say `"$schema": "./cut.schema.json"`
  (kept by saves) so editors validate as you type. Load errors lead with the
  JSON path: `scenes[0].layers[1].y: key [1]: invalid type: string "oops",
  expected f64 at line 12 column 30`.
- **`check`** finds what loading lets through: `unknown_field` (a typo that
  a save would silently drop, with "did you mean"), `wrong_kind` /
  `ignored_prop` (a field this layer's kind ignores), `duplicate_key`,
  `key_outside_scene`, `overshoot` (a bezier leaving its keys' range; a
  warning where the value is clamped, like opacity), `missing_asset`,
  `never_visible`, `clipped` (at rest, most of its ink outside the frame),
  `text_overlap` (two text layers at rest whose glyphs touch), `low_contrast`
  (text against the pixels actually rendered behind its glyphs, under 3:1), `fast_motion` (faster than
  8% of the frame a frame: it strobes), `empty_frame` (nothing visible
  between things that are), `short_scene`, `empty_scene`. "At rest" means
  not moving or fading, so entrances and exits do not count. `--json` gives
  `{errors, warnings, infos, issues: [{severity, code, path, scene, layer,
  t: [from, to], message, fix}]}`.
- **`sheet`** is a contact sheet: frames at every scene boundary and key
  (thinned to `--n` per scene) or at `--times`, each captioned with scene,
  time and frame above it. **`strip`** shows one layer's move in a single
  frame: the scene faded, the layer as ghosts from faint (early) to solid,
  its path as a yellow trail with timed dots. **`diff`** compares two
  versions and shows the most-changed frames as rows of A, B and a heat map,
  with the share of pixels changed. `diff P@REV` (or `diff P --rev REV`)
  compares a git revision (A) with the file as it is (B): the project and
  the assets it names are read with `git show` into a scratch folder beside
  the project, removed afterwards.
- **`gen`** runs a [Rhai](https://rhai.rs) script: no clock, no files, no
  modules, bounded operations, and randomness only from a seed, so a script
  and `--seed` always write the same file. It returns a project map, or an
  array of layers merged by id into a scene of `--into PROJECT` (reruns
  replace their own layers). Helpers: `rand()`, `rand(a, b)`,
  `rand_int(a, b)`, `pick(arr)`, `seed(n)`, `noise(x, y)` (Perlin),
  `hsl(h, s, l)`, `rgb`/`rgba` (0..1) to hex, `key(t, v[, interp])`,
  `lerp`, `clamp`; plus Rhai's maths (`PI()`, `sin`, ...; integer `/`
  truncates, so write `2.0`). Results are validated like a load, and
  unknown fields are errors. See `examples/gen/burst.rhai` (400 sparks) and
  `examples/gen/grid.rhai` (a 144-tile wave).

### The MCP server

```sh
claude mcp add mui-cut -- /path/to/mui-cut mcp            # Claude Code
claude mcp add mui-cut -- /path/to/mui-cut mcp demo.cut.json
```

Tools: `open` (with `create` and the new project's size, fps, scene,
duration, mode and background), `schema` (or one definition with `def`),
`list` (scenes, layers, animated properties with key times), `get` (a JSON
Pointer), `patch` (RFC 6902, all or nothing), `set`, `key`, `motion` (named
entrances and exits: fade, rise, slide and pop in and out, and per glyph
typewriter, cascade, cascade_out, pop), `batch` (several edit calls, all
or nothing, one write and one check), `add_layer`, `remove_layer`, `eval`,
`check`, `still` (with `samples` for a 3D beauty frame), `sheet`, `strip`,
`diff` (images come back as PNG image content),
`gen`, `render` + `render_status` (a background job), `plugin_parts`,
`sources_list` (sources with the layers using them and a plugin's part
tree), `source_add`, `notes_set`/`notes_add` (a plugin layer's notes),
`plugin_play` (renders a span's sound to a wav, or with picture to an
mp4, and says its length and peak), `patch_get` (the plugin's patch at a
time), `layer_parent` (parent or detach, keeping the screen
position), `editor_state`, `editor_goto`. Pointers may name scenes and layers by name/id:
`/scenes/intro/layers/title/x`. A plugin layer's `source` may name an
imported source by id. A layer, scene, tool, preset or argument that is not
there gets a "did you mean".

Every tool reads the file and every edit writes it: validated, refused if a
save would drop a field (a typo), canonical and atomic, followed by a
`check` summary. With a `mui-cut serve` open on the same file, an edit goes
through it instead, merged with the person's (see "Editing together"), and
the person's edits are in the file for the next tool call.

Resources: `mui-cut://schema`, `mui-cut://project` (the open file),
`mui-cut://examples/NAME` (every example project and `gen` script) and
`mui-cut://host-protocol` (HOST-PROTOCOL.md). Prompts: `promo_from_plugin`
(`plugin`, `seconds`, `project`: onboard a plugin and make a promo of it)
and `review_cut` (`project`, `rev`): each walks the agent through `check`,
`sheet`, fixing and checking again.

### Editing together

`serve` holds the project at a revision. An edit is a list of JSON Pointer
operations made against the revision its author last saw (`POST /patch
{base, ops, by}`; layers, scenes and sources are addressed by id or name,
so a reorder elsewhere does not move them). The server applies them to its
newest revision, validates, writes the file and pushes the merged document
to every open editor (SSE `doc`: `{rev, by, conflicts, doc}`). So edits to
different fields both land, whoever made them; where both sides changed
the same field the later edit wins and is listed in `conflicts`, which the
editor shows as a short notice. The editor applies a new revision onto its
own document in place, so a drag in progress keeps going on top of it, and
its undo replays only its own gestures, skipping fields somebody has
changed since. The MCP tools, the editor and an outside write to the file
(a new revision `by: "disk"`) all go through this. `GET /doc` is the newest
`{rev, project, doc}`; `PUT /project` still takes a whole document, as the
patch from the newest revision to it.

The editor reports its scene, playhead, selection, graphed property and
play state to the server (`PUT /state`); `editor_state` reads it (with
`same_project` and how old it is), so the agent sees what the person is
looking at. `editor_goto` moves the editor there (`POST /control`, relayed
to open editors as an SSE `control` event) to show the person something.
`serve` writes `.NAME.serve` (`{"port", "pid"}`) next to the project
`NAME` while it runs and removes it when it exits (Ctrl+C and SIGTERM
too), so the MCP server finds the editor on the open project by itself;
`editor_port` on `open` or `port` on a tool overrides it, and with
neither and no file it tries 8740.

The keyframe curves are not `mui_motion::curve::Curve`: that type is a
normalized `0..1` phase/value shaper that clamps values, while a property
track needs unbounded values and absolute times. The `Ease::Cubic` of
`mui_motion::Keys` is the same normalized CSS form. Undo is per gesture,
document before and after, in the editor rather than `CurveHistory`, for the
same reason.

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
- Bound keyframe values do not draw in the graph editor; edit them in the
  file.
- `check` measures outlines (each layer drawn alone, its alpha mask) only
  where a box says clipped or two text boxes overlap: a shape whose box
  is inside but whose ink is mostly out is not caught, and
  `never_visible` / `empty_frame` still go by boxes.
- An edit made against a revision more than 64 old still applies but
  reports no conflicts. An item added to an array lands at its index in
  the author's view, so a layer added while another was too can sit one
  place off in the paint order.
- `diff P@REV` takes assets inside the project's folder from the revision
  (a `../` path is left out) and shares the plugin capture cache (on
  Unix; elsewhere a revision's plugin layers draw as placeholders unless
  captured).
- `vello_gpu` draws MUI's backdrop blur sharp (no filter layer wired yet).
  It is pinned to Vello 9dfe53e; moving to newer main means following
  #1942 (`pop_clip_path` renamed) and #1944 (fallible glyph drawing) in
  `src/sparse.rs`.
- 3D: glTF is `.glb` only: triangles, material factors and PNG maps (a
  JPEG map is skipped: no decoder here), the first animation, skins on
  the CPU; no morph targets, cameras or lights from the file, maps
  without mips; `check`'s pixel lints skip 3D scenes. A layer's effects in 3D run in its own texture, so
  `chromatic` pulls towards the layer's centre, not the frame's.
- Sources and parenting: a reparent through a turn keeps every key exact,
  but a key baked into a bezier segment splits it into two eases, so the
  curve between keys can drift. A tilted child edge on (`rx` ±90) puts
  its whole roll in `ry`. Back to 2D drops `rx`/`ry` foreshortening. A
  rewrite in a project with variants replaces a changed binding with
  resolved values (per variant), so it no longer follows its variable.
- Plugin layers: the web editor shows the captures `serve` made, not the
  plugin running in WASM. A new state (a param or pointer edit) appears
  once `serve` has captured it, which takes a few seconds for a Cargo
  source. The adapter runs natively, so a
  plugin must build as a bridge live adapter. A 3D scene composites
  a translucent capture in linear light, so its soft edges read slightly
  brighter than in 2D.
- Sound: a generated adapter's patch reads modulation routes only from
  `<X> Source` / `<X> Target` / `<X> Amount` parameters (moose, truce);
  routes a plugin keeps in its private state (KURV's) are not visible to a
  host, and nice-plug and plain crates report none. `preset` is moose
  only. A plugin's keyboard does not light the notes the host plays unless
  the plugin draws them from its DSP (KURV lights its pointer's key only);
  the patch lists them. `serve` opens the device at the project's
  rate when it starts; a rate change needs a restart. The live view's old
  images are not freed in the viewport worker. `plugin_play` to an mp4
  renders the whole scene and trims it. The web export gets its sound
  from `serve` (`GET /mix.wav`): opened without it, it exports silent;
  it encodes AAC where the browser can (Chrome on Linux: Opus), with the
  encoder's priming samples left in (a few ms).
- `--renderer blender`: layer `effects` are skipped; scene `effects` and
  `overlay` layers are drawn by mui-cut over Blender's frames (on the GPU);
  a model's `fill` tint is ignored; frames are 8-bit PNG; point and spot
  light strength is matched to mui-stage at the nearest subject the light
  faces, so inverse-square falloff differs elsewhere; metals mirror the
  plain world (no HDRI). `serve`, `open` and `retarget` do not use it.
- The browser CPU fallback is single-threaded: wasm threads need
  cross-origin isolation, which `serve` does not set up.
