# mui-cut host protocol

How mui-cut drives a plugin adapter. An adapter is any process that speaks
this protocol on stdin/stdout. The usual way to write one is
`mui_motion_bridge::run_live` (`media/mui-motion-bridge/src/lib.rs`), which
implements everything below around three callbacks: `audio` (the DSP), `edit`
(parameters) and `frame` (the editor and its capture). Working adapters:

- `media/mui-cut/examples/synth.rs` is a small self-contained one, used by the tests.
- `media/tools/kurv-live/bridge.rs` is KURV's real editor and real `PluginLogic::process`.

Protocol version: `hello.version` = 1.

## Starting

mui-cut spawns the adapter with the layer's `source.args`, its working
directory set to the project directory, stderr inherited, and this environment:

| Variable | Meaning |
|---|---|
| `MUI_BRIDGE_CLOCK=manual:<rate>` | The host owns the sample clock at `<rate>` Hz (8000..=384000). No audio runs and no editor frame is drawn unless the host asks. Without it the adapter runs a wall clock at 48 kHz and draws on its own at about 30 fps (a standalone dev host, not what mui-cut uses). |

Adapters read the rate through `mui_motion_bridge::sample_rate()` and must
prepare their DSP at it.

## Wire

- **In (host to adapter):** one JSON object per line, at most 64 KiB.
- **Out (adapter to host):** packets, each a kind byte, then a little-endian
  u32 payload length, then the payload.
  - `J`: a UTF-8 JSON object.
  - `A`: audio. It holds a u64 LE sample frame (the block's first sample on
    the clock), then interleaved stereo f32 LE samples, clamped to -1..1,
    with non-finite values sent as 0.

## Adapter to host (`J` packets, by `type`)

| `type` | Fields | When |
|---|---|---|
| `hello` | `version`, `sampleRate`, `manualClock`, `blockFrames`, `plugin` (the adapter's describe object: `name`, `notes` if it plays notes, `patch` if its scenes carry a patch, `liveEditor`) | Once, first. |
| `scene` | `revision`, `frame` (sample clock when drawn), `scene` (capture manifest, below), `images` (name to base64 PNG, **only textures that changed** since the last scene), optional `patch` | After each snapshot or edit, and after the first command. |
| `edit` | `revision`, `frame`, `command` (echoed) | A command other than `input`/notes/`advance` was applied. A `snapshot` is acked this way, and the `scene` that follows the ack reflects every command sent before it. |
| `advanced` | `frame` | The clock reached an `advance`'s `to`. Every `A` packet up to `to` was sent before this. |
| `note` | `frame`, `command`, `active` (held note numbers) | A note event was applied. |
| `error` | `message` | A command was rejected. The adapter keeps running. |

## Host to adapter (by `op`)

### Clock and notes

```json
{"op": "advance", "to": 96000, "notes": [
  {"at": 48000, "op": "note_on", "note": 60, "velocity": 100},
  {"at": 72000, "op": "note_off", "note": 60}]}
```

The audio thread renders from its current sample exactly up to `to` (no
further). Blocks are at most `blockFrames` (480) and are split so each note
lands on its own sample `at` (absolute). Events on the same sample keep the
order sent, and mui-cut sends a note's off before a new on. Notes past `to`
wait for a later advance. A note on for a key that is already held releases
it first. There are at most 4096 notes per advance. `advance` requires the
manual clock.

`{"op": "note_on", "note": n, "velocity": 1..127}` (velocity defaults to 96),
`{"op": "note_off", "note": n}` and `{"op": "panic"}` without `at` land at the
current sample: at the next advance, or at the next block on the wall clock.
They are used for live playing.

mui-cut's offline rule: send the advance, **wait for `advanced`** (collecting
the `A` packets), and only then send anything else. That makes audio and
frames a pure function of the command list.

### Parameters

```json
{"op": "set", "id": "Cutoff", "field": "value", "value": 1200.0}
```

The adapter defines `id` and `field`. KURV takes a parameter id (number) or
name/short name, with `field` either `value`/`plain` (plain units) or `norm`
(0..1). The synth fixture takes `id` as a panel and `field` as a control
name, with a 0..1 value. The command is acked with `edit`, or answered with
`error`.

### Editor input

These are queued (at most 256) and all applied, in order, at the next
snapshot, followed by one neutral frame advanced by the time the clock moved.

| `kind` | Fields | Effect |
|---|---|---|
| `pointer` | `x`, `y` (plugin px), `buttons` (bit 1 primary, 2 secondary, 4 middle), `shift`/`ctrl`/`alt`/`meta` | Moves or presses the pointer. |
| `wheel` | `dx`, `dy` | Scrolls. |
| `key` | `key` (W3C name, `" "` for space), `text` | Types a key. |
| `cancel` | none | Cancels the current gesture. |
| `select` | `ids` (surface ids, at most 32; `[]` lets discovery choose), `depth` (1..8) | Chooses which surfaces the capture splits into parts. With no ids, `discover_tree` goes `depth` levels deep: 1 is panels, 2 is panels and their controls. |
| `resize` | `id` (a named surface), `width`, `height`, or `reset: true` | Reflows one surface through `mui::material::resize_capture`. |
| `view` | `width`, `height` (plugin px, each at least 8; 0 resets to the plugin's own size) | Lays the **whole editor** out at that size, as a host window resize would. The UI reflows (it is not scaled). The capture's `width`/`height`, parts and surfaces follow the new layout. Adapters read it with `Editor::viewport(sizes, own_size)` inside their frame and `editor.view(own_size)` after it. |

`{"op": "input", "kind": ...}` wraps each of them.

### Capture

```json
{"op": "snapshot", "tag": 7}
```

The adapter acks with `edit` (the command echoed with its `tag`) and then
sends a `scene`. The host keeps every texture it has seen, because a scene
only lists changed ones.

## The scene manifest (`scene`)

```json
{"version": 1, "width": 720, "height": 510, "scale": 2,
 "layers": [{"id": "fragment-0", "group": "background" | "<part path>",
             "part": "<surface id>", "parent": "<part path>|null",
             "origin": [x, y], "src": "<image name>", "rect": [x, y, w, h],
             "free": {"src": "...", "rect": [...]}}],
 "parts": [{"path": "osc/osc-shape", "id": "osc-shape", "parent": "osc", "frame": [x, y, w, h]}],
 "surfaces": [{"id": "...", "parent": "...", "frame": [x, y, w, h]}],
 "groups": ["<root ids>"]}
```

- Sizes and rects are in the plugin's pixels. Images are rasterised at `scale`.
- `layers` are fragments, bottom first. A fragment's `group` is its part's path (the part ids from the outermost down, joined by `/`), or `background`.
- `free` is the same paint pulled out of its ancestors' clips. It is drawn instead once the part moves.
- `parts` lists parents first.

## The patch (`scene.patch`, optional)

```json
{"plugin": "KURV",
 "params": [{"id": 12, "name": "Cutoff", "group": "Filter", "value": 1200.0, "text": "1.20 kHz", "norm": 0.61}],
 "routes": [{"source": "LFO 01", "target": "Cutoff", "depth": 0.4, "live": 0.13}],
 "held": [60, 64]}
```

- `params` lists the parameters off their defaults.
- `routes` are the modulation routes: `depth` is signed and `live` is the source's current value.
- `held` is the notes held.

mui-cut stores it with the capture and draws it in `patch` layers.

## What mui-cut sends for a plugin layer

`Layer::plugin_track` (`src/plugin.rs`) builds one command list per frame on
the project's frame grid. At frame 0 it sends `select` (with the depth that
explode and parts need). On every frame f > 0 of a layer with notes it sends
`advance` to `round(f / fps * rate)` with that span's notes. It then sends a
`view` when the keyed `view_width`/`view_height` changed (whole pixels), a
`set` for each param whose keyed value changed (rounded to 1e-6), and a
`pointer` when the keyed pointer changed. After every frame's commands it
takes a `snapshot`.

Each state's cache key is FNV-1a over the source and every command so far.
The captures sit under `.cut-cache/<key>.json` and `.cut-cache/img/<content hash>.png`.
The soundtrack (every `A` sample, plus a final advance to the scene's end)
sits under `.cut-cache/audio/<key>.f32` as raw interleaved stereo f32 LE.
