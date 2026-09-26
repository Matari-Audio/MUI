# MUI Film (tools/film)

The film pipeline (formerly `tools/mui-motion`) exposes a **running native MUI editor** as discoverable, independently transformable surfaces while its original audio engine keeps running. The optional Rust host is `mui-motion-bridge`; the existing `mui-motion` crate remains MUI's spring/curve mathematics. Normal plugin builds gain no browser or audio-host dependency.

## Run

```sh
python3 -m pip install -r tools/film/requirements.txt
cargo build --manifest-path media/Cargo.toml -p mui-motion-bridge --example tone
python3 tools/film/server.py --binary media/target/debug/examples/tone --port 3022
```

Open the page and click **Connect & enable audio**. Play notes, drag the actual native controls, or choose **Arrange** to move presentation planes. The component catalog comes from the native scene. **Extract**, **Highlight**, **Fill view**, and proportional zoom work on selected components. Native layout size rebuilds the component through MUI layout; it does not stretch its pixels.

Kurv uses the same host, browser, input mapping, discovery, and recording path:

```sh
# Initialize the isolated export checkout first: videos/kurv-unfold/README.md.
CARGO_TARGET_DIR=/tmp/kurv-motion-target python3 media/tools/kurv-live/build.py --build-dir /tmp/kurv-motion-build
python3 tools/film/server.py --binary /tmp/kurv-motion-target/debug/kurv-motion-live --port 3020
```

Its working source checkout is never patched. The adapter supplies advancing host transport, the real Truce meter store, and a real LFO-to-oscillator-level route. Native LFO playheads and modulation displays consume the processor's telemetry. The independent Tone instrument demonstrates the same live-editor contract with a native knob and DSP-driven tremolo phase.

## Script the editor

The connected page exposes `window.muiMotion`. No plugin-specific component list is required.

```js
const motion = window.muiMotion;
console.table(motion.components); // All resolved IDs, parent IDs, logical bounds.
const id = motion.components.find(s => s.id === 'osc/0').id;
await motion.select([id]);         // Native paint partition: selected part + rest.
await motion.highlight(id);
await motion.animate(id, {scaleX: 1.8, scaleY: 1.8, z: 160}, 1);
await motion.focus(id);            // Center and proportionally fit this component.
await motion.resize(id, 900, 360);  // Actual MUI layout sizing, subject to its layout rules.
motion.command({op: 'note_on', note: 69, velocity: 96});
motion.explode(.8);
```

A sequential trailer script can use `perform`:

```js
await motion.perform([
  {command: {op:'record_start'}},
  {select:['osc/0','mod/1'], command:{op:'note_on',note:69}},
  {explode:.7, wait:.5},
  {highlight:'mod/1', wait:1},
  {animate:{id:'mod/1',to:{scaleX:1.5,scaleY:1.5,z:220},seconds:1}},
  {wait:1, command:{op:'panic'}},
  {command:{op:'record_stop'}}
]);
```

Commands are acknowledged on the engine clock; script waits are browser scheduling, not sample-accurate automation. Record the resulting audio/visual performance. Saved takes can be rendered with the page's **Render MP4** button or:

```sh
python3 tools/film/render.py /path/to/film --output performance.mp4
```

## Integrate another MUI plugin

Keep one real editor, model, and processor alive for the session. The [Tone example](../../crates/mui-motion-bridge/examples/tone.rs) is the complete small reference.

```rust,ignore
mui_motion_bridge::run_live(
    capabilities,
    audio, // FnMut(&[NoteEvent], &mut [[f32; 2]]) + Send + 'static
    edit,  // FnMut(&Value) -> Result<(), String>
    frame, // FnMut(revision: u64, sample_frame: u64, inputs: &[Value]) -> Result<Value,String>
)
```

The frame callback owns `Editor` and the plugin's view. `Editor::advance` delivers pointer/key/wheel/cancel events in order, preserving press/release edges and UI state. It advances time from the audio sample clock and supplies layout overrides to `Editor::layout`. Build through the plugin's existing tree/frame/after cycle. `CaptureStream::frame` returns the resolved scene manifest and changed PNG textures directly in memory.

Call `discover_parts` for the automatic partition, or use `editor.selection` when the user selects components. The catalog always carries every resolved surface. Discovery uses generic hierarchy/size heuristics; scripts can choose exact component IDs. Named surfaces are stable editing targets. Generated anonymous paths remain visible in the catalog but are not stable extraction/layout targets. Fused materials and partial compositing groups must remain atomic; unsupported extraction returns an error rather than incorrect paint.

`CaptureStream` compares native paint to reuse static raster results, then content-addresses textures so unchanged PNGs are not retransmitted. The browser preserves plane nodes, inverse-projects pointer coordinates through perspective, and skips transparent pixels when picking. Input goes to the original native controls. A presented image is never a replacement parameter model.

The plugin adapter still supplies processor/model semantics and its existing DSP-to-UI publication path. MUI cannot infer audio processing or modulation meanings from pixels. Kurv's sidebar additionally exposes its first group's VA/Noise/Filter add, reorder, delete, and oscillator parameters; the native editor itself receives the generic input stream.

## Timing, export, and limits

- Native audio: 48 kHz stereo, 480-frame blocks; notes acknowledged at block boundaries. This allocating local dev host is not a production DAW callback.
- Native frame loop targets 30 fps, bounded by scene raster cost. It does not run rasterization on the audio thread. The browser presents frames against played-audio time; late frames display when ready. Audio buffering is selectable (100/150/250 ms) before connecting, with underruns and resyncs visible.
- During preview, textures stay in memory. Recording writes content-addressed textures, frame manifests, native audio, note/edit events, and presentation poses. Takes are limited to two minutes.
- HyperFrames replays the frozen visual performance with deterministic seeking; it does not re-synthesize DSP while rendering. One live set of planes is reused during playback. Changing musical performance requires a new take.
- Recorded scopes derive from the exact exported PCM16 WAV. MP4 uses 512 kbit/s AAC; the WAV remains the lossless waveform reference.
- Planes provide 2.5D presentation. Proportional zoom preserves aspect ratio; it is distinct from layout reflow and optional nonuniform stretch. Texture resolution remains the adapter's capture resolution; zoom does not invent pixels.
- Layout reflow respects the original component and parent constraints. Fixed layouts do not automatically become responsive. Unsupported native GPU/external paint still follows the capture API restrictions.
- The gateway binds to loopback, validates host/origin, and permits one controller per adapter process. It starts a fresh process when reconnecting. Interrupted recordings are preserved.

Wire: native stdin is newline JSON; stdout is one `J`/`A` byte, little-endian u32 length, payload. `A` carries a u64 sample frame and interleaved float32 stereo. `J` carries hello, scene, note, edit, error. Live scene packets include `images` keyed by content hash; receivers retain only current-frame textures. Do not log to stdout.

## Checks

```sh
cargo test --manifest-path media/Cargo.toml -p mui-motion-bridge && cargo test -p mui-scene
PUPPETEER_MODULE=/path/to/puppeteer-core.js CHROME=/path/to/chrome \
  node tools/film/live-check.mjs http://localhost:3022
# Same check against Kurv on 3020.
python3 tools/film/check-recording.py /path/to/film
```

The browser check exercises DSP-driven native pixel changes without edits, inverse-projected native control dragging, component selection/highlighting/animation, Tone layout reflow without stretching its knob, audio health, recording, and history-independent visual seeking.

Assets: bundled Inter with OFL license, GSAP with upstream license header. The scope plots captured PCM rather than a decorative registry animation.
