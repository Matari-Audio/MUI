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
        .run(script, &mut m, build, Some(&mut audio), &mut |s| {
            let mut h = DefaultHasher::new();
            s.rgba.hash(&mut h);
            hashes.push(h.finish());
            Ok(())
        })
        .unwrap();
    (hashes, track, heard, m)
}

fn gesture() -> Script<Model> {
    Script::new()
        .at(0.1)
        .move_to("gain", 0.1)
        .at(beat(0.5))
        .drag("gain", (0.0, -60.0), beats(0.5), Ease::default())
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
    assert!(Reel::new(Size::new(10.0, 10.0))
        .frame_of(beat(1.0))
        .is_err());
    let s = Script::new()
        .at(beat(1.0))
        .note_on(60, 100)
        .at(beat(2.0))
        .note_off(60)
        .end(beat(3.0));
    let (frames, track, heard, _) = play(&r, &s);
    assert_eq!(frames.len(), 45);
    assert_eq!(heard[15], vec![ReelEvent::NoteOn { key: 60, velocity: 100 }]);
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
    let mut ui = Ui::new(Theme::DEFAULT).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
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
        track["surfaces"]["bypass"].as_array().unwrap().last().unwrap().clone(),
    )
    .unwrap();
    // Centred in the 200x120 frame, and the padded node fills one axis.
    let (cx, cy) = (rect[0] + rect[2] / 2.0, rect[1] + rect[3] / 2.0);
    assert!((cx - 100.0).abs() < 0.5 && (cy - 60.0).abs() < 0.5, "{rect:?}");
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
