# mui-reel

Deterministic, offline trailer takes of a MUI editor. No window, no compositor,
no input driver, no screen recorder, no wall clock.

A `Script` of pointer, key, camera and note cues plays against the real editor
tree on a fixed step (`Ui::frame(.., dt = 1 / fps)`). The CPU rasteriser draws
each frame through a springed camera transform, so a punch-in is re-rasterised
and stays sharp instead of being upscaled. The audio closure runs once per video
frame on the same model the UI edits, so picture and sound stay sample-locked
without any threads. Two renders are byte-identical.

```rust
let reel = Reel::new(Size::new(640., 360.)).scale(3.).fps(60).bpm(116.).cursor(true).font(font);
let script = Script::new()
    .note_on(57, 100)
    .at(beat(0.25)).move_to("gain", beats(1.))
    .at(beat(1.5)).camera_focus("gain-panel", 60., Spring::new(0.7, 0.9))
    .at(beat(2.)).drag("gain", (0., -70.), beats(2.), Ease::IN_OUT)
    .at(beat(5.)).camera_reset(Spring::new(0.6, 1.))
    .at(beat(6.5)).click("bypass")
    .layers(&["gain-panel"])
    .end(beat(12.));
reel.render(&script, dir, &mut model, editor, Some(&mut dsp))?;
```

Timing rules:
- A cue fires on the first frame at or after its time. At 116 bpm and 60 fps a
  beat is 31.03 frames. `track.json` keeps the cue's exact time.
- Pointer paths and the camera are evaluated at absolute time.
- Cues that land on the same frame apply in the order they were written.
- Pointer targets are named surfaces, resolved from the scene the previous
  frame drew. Frame 0 has no scene yet, so pointer cues cannot fire on it.
- The camera changes paint only. Hit-testing stays in scene space.

## 1. Render a take

    cargo run --manifest-path media/Cargo.toml -p mui-reel --example gain_reel -- videos/mui-reel-demo/take [--master]

The output folder is the handoff:

| file | what |
| --- | --- |
| `take.mp4` | H.264 High, yuv420p, CRF 14, constant fps. Converted to BT.709 limited range and tagged as such. Use `.ten_bit(true)` for H.265 Main10. Without ffmpeg you get `frames/%05d.png`. |
| `take.mov` | With `.master(true)`: ProRes 4444 with alpha. |
| `audio.wav`, `take-with-audio.mp4` | 32-bit float stereo at 48 kHz, when an audio closure is given. |
| `layers/<id>.webm` / `.mov`, `layers/rest.*` | Each `script.layers(..)` part, plus everything else, on transparency. They line up with the take. VP9 alpha is the default. With `.master(true)` they are ProRes 4444, because VP9 alpha does not decode on Windows. |
| `track.json`, `track.js` | Per frame, in video pixels after the camera: every named surface's `[x,y,w,h]`, the pointer `[x,y,down]` and the camera `[x,y,zoom]`. Also a list of events: notes, custom events, `edit_begin`/`edit_end` gesture brackets, and slider/knob/toggle `value` changes. `track.js` sets the same data as `window.MUI_TRACK_DATA`. |
| `mui-track.js` | `MuiTrack.load(json)`, then `.rect(id, t)`, `.pointer(t)`, `.camera(t)`, `.beat(n)`, `.events(kind)`. No dependencies. Works as a classic script or through CommonJS. |
| `clip.html` | A HyperFrames sub-composition (`data-composition-id="mui-reel"`). It plays the take and the audio as framework-owned media, with the track inlined as `window.MUI_TRACK`. |
| `manifest.json` | What was written, the encoder, and the reproduction parameters: fps, sizes, scale, bpm, beats, duration, sample rate. |

`.motion_blur(n)` renders `n` subframes per frame and averages them. The UI
steps at `dt / n`, so the blur follows the real path of each spring and drag.

## 2. HyperFrames

`videos/mui-reel-demo/` is a working project. It mounts `take/clip.html`, puts
dots on the take's beat grid, and draws a GSAP callout that follows
`track.rect("gain", t)`. The bpm and beat times come from the take's own track,
so re-timing the Rust script re-times the composition.

    cd videos/mui-reel-demo && npx hyperframes check && npx hyperframes render

## 3. Remotion

Copy `remotion/MuiReel.tsx` into `src/` and the take folder into `public/<dir>/`:

```tsx
<MuiReel dir="gain-reel"><Callout /></MuiReel>
// inside Callout:
const r = useMuiTrack().rect("gain"); // at the current frame, any composition fps
```

`MuiReel.tsx` needs nothing beyond `remotion`. No Node toolchain is added to this repo.
