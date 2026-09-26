use super::*;
use std::hash::{DefaultHasher, Hash, Hasher};

struct Model {
    gain: f64,
    on: bool,
}

fn build(ui: &mut Ui, m: &mut Model) -> El {
    let k = knob(ui, "gain", "Gain", &mut m.gain, 0.0..=1.0).0.el();
    let t = toggle(ui, "bypass", &mut m.on).0.el();
    row([k, t]).gap(M).pad(M).fill(Role::Surface)
}

fn reel() -> Reel {
    Reel::new(Size::new(200.0, 120.0))
        .fps(30)
        .bpm(120.0)
        .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap())
}

/// Frame hashes, the track, and every frame's events via the audio hook.
fn play(reel: &Reel, script: &Script<Model>) -> (Vec<u64>, Value, Vec<Vec<ReelEvent>>, Model) {
    let mut m = Model {
        gain: 0.5,
        on: false,
    };
    let mut hashes = Vec::new();
    let mut heard = Vec::new();
    let mut audio = |_: &mut Model, e: &[ReelEvent], _: &mut [[f32; 2]]| heard.push(e.to_vec());
    let track = reel
        .run(
            script,
            &mut m,
            build,
            Some(&mut audio),
            &mut None,
            &mut |s| {
                let mut h = DefaultHasher::new();
                s.rgba.hash(&mut h);
                hashes.push(h.finish());
                Ok(())
            },
        )
        .unwrap();
    (hashes, track, heard, m)
}

fn gesture() -> Script<Model> {
    Script::new()
        .at(0.1)
        .move_to("gain", 0.1)
        .at(beat(0.5))
        .drag("gain", (0.0, -60.0), beats(0.5), Ease::IN_OUT)
        .at(0.9)
        .click("bypass")
        .end(beat(2.5))
}

#[test]
fn two_renders_are_byte_identical() {
    let r = reel().cursor(true);
    let (a, ta, ..) = play(&r, &gesture());
    let (b, tb, ..) = play(&r, &gesture());
    assert_eq!(a.len(), 38);
    assert_eq!(a, b);
    assert_eq!(ta, tb);
    // And the frames are not trivially all the same picture.
    assert!(a.windows(2).any(|w| w[0] != w[1]));
}

#[test]
fn beats_land_on_the_frame_at_the_tempo() {
    let r = reel();
    // 120 bpm: a beat is half a second, 15 frames at 30 fps.
    assert_eq!(r.frame_of(beat(1.0)).unwrap(), 15);
    assert_eq!(r.frame_of(At::Secs(0.5)).unwrap(), 15);
    assert!(
        Reel::new(Size::new(10.0, 10.0))
            .frame_of(beat(1.0))
            .is_err()
    );
    let s = Script::new()
        .at(beat(1.0))
        .note_on(60, 100)
        .at(beat(2.0))
        .note_off(60)
        .end(beat(3.0));
    let (frames, track, heard, _) = play(&r, &s);
    assert_eq!(frames.len(), 45);
    assert_eq!(
        heard[15],
        vec![ReelEvent::NoteOn {
            key: 60,
            velocity: 100
        }]
    );
    assert_eq!(heard[30], vec![ReelEvent::NoteOff { key: 60 }]);
    assert_eq!(track["events"][0]["t"], 0.5);
}

#[test]
fn audio_blocks_sum_to_the_duration_exactly() {
    // 7 fps does not divide 44.1 kHz: the blocks alternate lengths.
    let r = reel().fps(7).sample_rate(44_100);
    let mut m = Model {
        gain: 0.5,
        on: false,
    };
    let mut n = 0;
    let mut audio = |_: &mut Model, _: &[ReelEvent], s: &mut [[f32; 2]]| n += s.len();
    r.run(
        &Script::new().end(3.0),
        &mut m,
        build,
        Some(&mut audio),
        &mut None,
        &mut |_| Ok(()),
    )
    .unwrap();
    assert_eq!(n, 3 * 44_100);
}

#[test]
fn a_drag_brackets_the_knob_and_moves_it() {
    let (_, _, heard, m) = play(&reel(), &gesture());
    let all: Vec<_> = heard.iter().flatten().collect();
    let begin = all.iter().position(|e| {
        **e == ReelEvent::Edit {
            id: "gain".into(),
            begin: true,
        }
    });
    let end = all.iter().position(|e| {
        **e == ReelEvent::Edit {
            id: "gain".into(),
            begin: false,
        }
    });
    let (begin, end) = (begin.expect("no Begin"), end.expect("no End"));
    assert!(begin < end);
    assert!(
        all[begin..end]
            .iter()
            .any(|e| matches!(e, ReelEvent::Value { id, .. } if id == "gain")),
        "no value change inside the bracket: {all:?}"
    );
    // 60 px up of the knob's 120 px travel.
    assert!((m.gain - 1.0).abs() < 0.02, "gain {}", m.gain);
    assert!(m.on, "the click did not flip the toggle");
}

#[test]
fn track_rects_are_surface_frames_at_scale() {
    let r = reel().scale(2.0);
    let mut m = Model {
        gain: 0.5,
        on: false,
    };
    let mut ui =
        Ui::new(Theme::DEFAULT).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    ui.scale = Some(2.0);
    let root = build(&mut ui, &mut m);
    let f = ui
        .frame(root, Some(r.size), Input::default(), 1.0 / 30.0)
        .unwrap()
        .scene
        .surface("gain")
        .unwrap()
        .frame;
    let (_, track, ..) = play(&r, &Script::new().end(0.1));
    let got = &track["surfaces"]["gain"][0];
    let want = [f.x, f.y, f.size.width, f.size.height].map(|v| round(v * 2.0));
    for (g, w) in got.as_array().unwrap().iter().zip(want) {
        assert!((g.as_f64().unwrap() - w).abs() <= 0.1, "{got} vs {want:?}");
    }
    assert_eq!(track["size"], json!([400, 240]));
}

#[test]
fn camera_focus_settles_on_the_node() {
    let r = reel();
    let s = Script::new()
        .at(0.1)
        .camera_focus("bypass", 4.0, Spring::new(0.3, 1.0))
        .end(2.0);
    let (_, track, ..) = play(&r, &s);
    let last = |k: &str| track[k].as_array().unwrap().last().unwrap().clone();
    let rect: Vec<f64> = serde_json::from_value(
        track["surfaces"]["bypass"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .clone(),
    )
    .unwrap();
    // Centred in the 200x120 frame, and the padded node fills one axis.
    let (cx, cy) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
    assert!(
        (cx - 100.0).abs() < 0.5 && (cy - 60.0).abs() < 0.5,
        "{rect:?}"
    );
    let zoom = last("camera")[2].as_f64().unwrap();
    let fits_w = ((rect[2] / zoom + 8.0) * zoom - 200.0).abs() < 1.0;
    let fits_h = ((rect[3] / zoom + 8.0) * zoom - 120.0).abs() < 1.0;
    assert!(zoom > 1.5 && (fits_w || fits_h), "zoom {zoom}, {rect:?}");
}

#[test]
fn wav_header_is_float_stereo() {
    let b = wav(&[[0.5, -0.5]; 3], 48_000);
    assert_eq!(&b[..4], b"RIFF");
    assert_eq!(u16::from_le_bytes([b[20], b[21]]), 3);
    assert_eq!(b.len(), 44 + 24);
}

#[test]
fn an_off_grid_beat_fires_on_the_next_frame_and_keeps_its_exact_time() {
    // 116 bpm at 60 fps is 31.03 frames a beat.
    let r = reel().fps(60).bpm(116.0);
    assert_eq!(r.frame_of(beat(1.0)).unwrap(), 32);
    let s = Script::new().at(beat(1.0)).note_on(60, 1).end(beat(2.0));
    let (_, track, heard, _) = play(&r, &s);
    assert!(heard[31].is_empty() && !heard[32].is_empty());
    assert_eq!(track["events"][0]["t"], 0.517);
}

#[test]
fn motion_blur_averages_subframes_without_changing_the_gesture() {
    let r = reel().motion_blur(4);
    let (a, _, heard, m) = play(&r, &gesture());
    let (b, ..) = play(&r, &gesture());
    let (sharp, ..) = play(&reel(), &gesture());
    assert_eq!(a, b, "blurred renders are deterministic too");
    assert_eq!(a.len(), sharp.len());
    assert_ne!(a, sharp);
    // Not exactly the sharp 1.0: at 4x the input rate the runtime's drag
    // slop is crossed in smaller steps and swallows a little more travel,
    // which is what a 120 Hz mouse does to the real editor as well.
    assert!(m.gain > 0.95 && m.on, "gain {} on {}", m.gain, m.on);
    assert!(
        heard
            .iter()
            .flatten()
            .any(|e| matches!(e, ReelEvent::Edit { begin: false, .. }))
    );
}

#[test]
fn without_ffmpeg_the_folder_is_png_sequences_and_says_so() {
    let dir = std::env::temp_dir().join(format!("mui-reel-png-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut m = Model {
        gain: 0.5,
        on: false,
    };
    let mut silence = |_: &mut Model, _: &[ReelEvent], _: &mut [[f32; 2]]| {};
    let manifest = reel()
        .encode(false)
        .render(
            &Script::new().layers(&["gain"]).end(0.1),
            &dir,
            &mut m,
            build,
            Some(&mut silence),
        )
        .unwrap();
    assert!(manifest["encoder"].as_str().unwrap().starts_with("none"));
    for f in [
        "frames/00002.png",
        "layers/gain/00002.png",
        "layers/rest/00000.png",
        "audio.wav",
        "track.js",
    ] {
        assert!(dir.join(f).exists(), "{f} missing");
    }
    assert!(!dir.join("frames/00003.png").exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_look_draws_the_take_and_sees_every_subframe_time() {
    let r = reel().fps(10).motion_blur(2);
    let (w, h) = r.pixels();
    let mut times = Vec::new();
    let mut look = |take: &Take| {
        times.push(take.t);
        Ok(vec![200; usize::from(w) * usize::from(h) * 4])
    };
    let mut m = Model {
        gain: 0.5,
        on: false,
    };
    let mut frames = Vec::new();
    r.run(
        &Script::new().end(0.3),
        &mut m,
        build,
        None,
        &mut Some(&mut look),
        &mut |s| {
            frames.push(s.rgba);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(times, [0.0, 0.05, 0.1, 0.15, 0.2, 0.25]);
    assert!(frames.iter().all(|f| f.iter().all(|&b| b == 200)));
    // The wrong size is an error, not a garbled stream.
    let mut short = |_: &Take| Ok(vec![0; 4]);
    assert!(
        r.run(
            &Script::new().end(0.1),
            &mut m,
            build,
            None,
            &mut Some(&mut short),
            &mut |_| Ok(())
        )
        .is_err()
    );
}

#[test]
fn motion_blur_averages_light_not_bytes() {
    // White over black for one subframe of two: half the light, which is
    // sRGB 188, not the byte mean 128. Alpha averages plainly.
    let mut acc = vec![0.0; 8];
    accumulate(&mut acc, &[255, 255, 255, 255, 0, 0, 0, 0]);
    accumulate(&mut acc, &[0, 0, 0, 255, 0, 0, 0, 0]);
    assert_eq!(resolve(&acc, 2), [188, 188, 188, 255, 0, 0, 0, 0]);
    // A pixel covered only half the time keeps its colour and halves its alpha.
    let mut acc = vec![0.0; 4];
    accumulate(&mut acc, &[200, 100, 50, 255]);
    accumulate(&mut acc, &[0, 0, 0, 0]);
    assert_eq!(resolve(&acc, 2), [200, 100, 50, 128]);
}

#[test]
fn the_cursor_paints_into_the_scene_on_top() {
    let scene = mui::scene::resolve(&mui::scene::SceneSpec::new(
        block(40.0, 40.0).fill(Role::Surface),
    ))
    .unwrap();
    let with = with_cursor(&scene, Point::new(10.0, 10.0), false);
    assert_eq!(with.paint.len(), scene.paint.len() + 2);
    assert_eq!(&*with.paint.last().unwrap().key, "mui-reel/cursor");
}
