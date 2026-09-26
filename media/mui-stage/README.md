# mui-stage

A GPU stage for MUI trailers. Vello paints each MUI layer, which is then
linearised and mipmapped. The stage sets it on a lit, extruded slab under a
perspective camera, over a WGSL background and a floor that mirrors it, and
finishes each frame with depth of field, bloom, chromatic aberration,
vignette, an ACES tonemap and grain. Motion blur averages real subframes
across the shutter.

```rust
let mut stage = Stage::new(1920, 1080)?;
stage.layer("card", ui_frame.scene, card_size, 3.0)?;   // every frame the UI changes
let frame = stage.render(t, 0.5 / 60.0, 8, &|t| Shot {
    planes: vec![Plane::new("card", w, h).rotate(0.0, 20.0 * t as f32, 0.0).depth(18.0).glow(1.2)],
    ..Shot::new(Camera::front(540.0, 35.0).orbit(-10.0, 4.0))
})?;
ffmpeg_stdin.write_all(&frame.rgba8())?;                  // or rgba16() for a 10-bit master
```

- **Time.** The caller owns it. The shot is a function of time, so the same
  code renders frame-exact at any fps.
- **Scale.** World units are logical pixels. `Camera::front(h, fov)` puts a
  layer `h` tall exactly edge to edge. With `Post::NONE`, a flat plane is the
  2D render pixel for pixel; a test holds it to that.
- **Walls.** `Plane::outline(path)` takes the walls from a surface's own
  outline, so a rounded card extrudes rounded.
- **3D text.** `Stage::text_layer(id, fonts, "GAIN", 72.0, colour, pad, 2.0)`
  returns a plane whose walls are the glyphs' own contours, so
  `.depth(24.0)` gives real extruded letters. A letter's counter faces
  inward.
- **Floor.** `Shot::floor = Some(Floor::at(y))` adds a glossy floor. A nearer
  reflection hides a farther one, and each fades into the floor colour with
  its depth below the floor and toward the floor's rim.
- **Depth of field.** `Post { focus: cam.distance(), aperture, max_blur }`
  runs a gather blur on each pixel's eye distance, and keeps a blurred
  background from bleeding over a sharp edge.
- **Reel camera.** `Camera::punch(canvas, centre, zoom)` is the reel's 2D
  punch-in in 3D; orbit after it and the subject stays centred.
- **Background.** `Stage::background(wgsl)` replaces the background with
  `fn background(uv: vec2f, t: f32) -> vec3f`, with `noise`, `fbm` and
  `hash2` in scope. A shader that does not compile returns an error and
  leaves the old background in place.
- **With the reel.** `mui_reel::Reel::render_through(.., Some(look))` hands
  every scripted subframe to a look closure as a `Take`: the scene, the
  time, the script's camera and the pointer. `mui_reel::with_cursor` paints
  the pointer into the layer, so it rides the slab. See
  `cargo run --manifest-path media/Cargo.toml -p mui-reel --example gain_reel -- /tmp/out --stage`.
- **Standalone.** `cargo run --manifest-path media/Cargo.toml -p mui-stage --example stage_shot -- /tmp/stage`
  renders a four-second fly-in with a live knob and toggle.

Needs a GPU adapter that can do 4x MSAA on `Rgba16Float`; `Stage::new` says
so if it cannot. The tests skip when there is no adapter.
