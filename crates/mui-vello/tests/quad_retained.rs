//! The quad pass keeps what it drew on a target and redraws only what
//! changed. Every frame here is drawn on one kept target and compared,
//! byte for byte, with the same list drawn whole by a fresh pass on a fresh
//! target: a stale pixel anywhere fails.
#![cfg(all(feature = "gpu-quad", feature = "cpu"))]

mod quad_support;
use quad_support::*;

use mui_scene::prelude::*;
use mui_scene::{Font, Image, Paint, Painted, ResolvedScene};
use mui_vello::kurbo::Affine;
use mui_vello::quad::{ATLAS_BUDGET as ATLAS, QuadRenderer};
use std::sync::Arc;

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const SIZE: [u32; 2] = [W as u32, H as u32];

struct Kept {
    device: wgpu::Device,
    queue: wgpu::Queue,
    quad: QuadRenderer,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

impl Kept {
    fn new(quad: impl FnOnce(&wgpu::Device) -> QuadRenderer) -> Option<Self> {
        let (device, queue) = device()?;
        let quad = quad(&device);
        let texture = target(&device, SIZE);
        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        Some(Self {
            device,
            queue,
            quad,
            texture,
            view,
        })
    }

    /// Draw on a new target from now on, `size` pixels.
    fn resize(&mut self, size: [u32; 2]) {
        self.texture = target(&self.device, size);
        self.view = self
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
    }

    /// `paint` on the kept target, checked against a whole redraw.
    fn check(&mut self, what: &str, paint: &[Painted], xf: Affine) {
        let size = [self.texture.width(), self.texture.height()];
        self.quad
            .render(&self.device, &self.queue, paint, xf, &self.view, size)
            .expect("quad render");
        let got = readback(&self.device, &self.queue, &self.texture, size);
        let fresh = target(&self.device, size);
        let view = fresh.create_view(&wgpu::TextureViewDescriptor::default());
        QuadRenderer::new(&self.device, FORMAT)
            .render(&self.device, &self.queue, paint, xf, &view, size)
            .expect("quad render");
        let want = readback(&self.device, &self.queue, &fresh, size);
        if got != want {
            let dir = out_dir();
            save(&dir.join(format!("retained-{what}-got.png")), size, &got);
            save(&dir.join(format!("retained-{what}-want.png")), size, &want);
            panic!("{what}: stale pixels; images in {}", dir.display());
        }
    }
}

fn target(device: &wgpu::Device, size: [u32; 2]) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

// One font for every frame, as a host keeps it: a fresh `Font` is a new
// face, so every run would differ.
static HACK: std::sync::LazyLock<Font> =
    std::sync::LazyLock::new(|| Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());

/// Overlapping translucent cards with shadows and labels, and an image.
fn cards(label: &str, tint: f32, image: Option<Arc<Image>>) -> ResolvedScene {
    sized_cards(label, 11., tint, image)
}

fn sized_cards(label: &str, size: f64, tint: f32, image: Option<Arc<Image>>) -> ResolvedScene {
    let mut kids: Vec<El> = (0..6)
        .map(|i| {
            let x = 12. + 48. * i as f64;
            col(vec![
                text(if i == 2 {
                    label.to_owned()
                } else {
                    format!("Card {i}")
                })
                .text_size(if i == 2 { size } else { 11. })
                .fill(Color::srgb(0.95, 0.95, 0.9)),
                block(30., 8.).radius(4.).fill(Color::srgb(
                    if i == 3 { tint } else { 0.5 },
                    0.4,
                    0.3,
                )),
            ])
            .pad(6.)
            .radius(8.)
            .fill(Color::srgba(0.2, 0.3, 0.5, 0.8))
            .shadow(Shadow::soft(5.))
            .id(format!("card{i}"))
            .at(x, 20. + 9. * i as f64)
        })
        .collect();
    if let Some(img) = image {
        kids.push(
            block(60., 40.)
                .radius(6.)
                .fill(Fill::Image(img, Fit::Cover))
                .at(240., 130.),
        );
    }
    let mut spec = SceneSpec::new(ground(kids)).offered(Size::new(W, H));
    spec.font = Some(HACK.clone());
    mui_scene::resolve(&spec).expect("resolves")
}

fn photo(seed: u8) -> Arc<Image> {
    let px: Vec<u8> = (0..16 * 16)
        .flat_map(|i| [seed.wrapping_mul(i as u8), 90, 200 - seed, 255])
        .collect();
    Arc::new(Image::rgba(16, 16, px).unwrap())
}

#[test]
fn a_kept_target_matches_a_whole_redraw_after_every_change() {
    let Some(mut k) = Kept::new(|d| QuadRenderer::new(d, FORMAT)) else {
        return;
    };
    let scope = k.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let id = Affine::IDENTITY;
    let img = photo(3);
    let base = cards("Cutoff 440 Hz", 0.8, Some(img.clone()));
    k.check("first", &base.paint, id);
    assert_eq!(k.quad.frame().damage, None);

    // Unchanged: nothing drawn, nothing stale.
    k.check("unchanged", &base.paint, id);
    assert!(!k.quad.frame().drawn, "{:?}", k.quad.frame());

    // A fill in the middle of the list, under an overlapping card.
    let tinted = cards("Cutoff 440 Hz", 0.1, Some(img.clone()));
    k.check("middle", &tinted.paint, id);
    let damage = k.quad.frame().damage.expect("a partial frame");
    assert!(damage[2] * damage[3] < SIZE[0] * SIZE[1] / 4, "{damage:?}");

    // A label's text, longer and then shorter.
    let longer = cards("Cutoff 12000 Hz wide", 0.1, Some(img.clone()));
    k.check("label", &longer.paint, id);
    assert!(k.quad.frame().damage.is_some());
    k.check("label back", &tinted.paint, id);

    // An entry moved by its own transform.
    let mut moved = tinted.clone();
    let at = moved
        .paint
        .iter()
        .position(|p| p.key.as_ref() == "card4")
        .expect("card4");
    moved.paint[at].transform = Affine::translate((-31.5, 17.25)) * moved.paint[at].transform;
    k.check("moved", &moved.paint, id);

    // Inserted and removed entries.
    let mut inserted = moved.clone();
    let mut extra = inserted.paint[at].clone();
    extra.paint = Paint::Solid(Color::srgb(1., 0.9, 0.1));
    inserted.paint.insert(at + 1, extra);
    k.check("inserted", &inserted.paint, id);
    k.check("removed", &moved.paint, id);
    let mut shorter = moved.clone();
    shorter.paint.remove(at);
    k.check("removed one", &shorter.paint, id);

    // The app swaps the image for another and drops the first, then drops
    // the image entry altogether.
    let swapped = cards("Cutoff 440 Hz", 0.8, Some(photo(77)));
    k.check("first again", &base.paint, id);
    drop((base, img));
    k.check("image swapped", &swapped.paint, id);
    assert!(k.quad.frame().damage.is_some());
    let none = cards("Cutoff 440 Hz", 0.8, None);
    k.check("image dropped", &none.paint, id);

    // The transform, then the size the pass is told, change under it.
    k.check("scrolled", &none.paint, Affine::translate((0.37, 1.61)));
    assert_eq!(k.quad.frame().damage, None);
    k.check("scaled", &none.paint, Affine::scale(1.25));
    k.resize([SIZE[0] - 40, SIZE[1] - 30]);
    k.check("resized", &none.paint, Affine::scale(1.25));
    assert_eq!(k.quad.frame().damage, None);
    k.check("resized, unchanged", &none.paint, Affine::scale(1.25));
    assert!(!k.quad.frame().drawn);
    k.resize(SIZE);
    k.check("resized back", &none.paint, Affine::scale(1.25));
    assert_eq!(k.quad.frame().damage, None);

    // Something else drew on the target: told so, the pass redraws whole.
    k.quad.invalidate();
    k.check("invalidated", &none.paint, Affine::scale(1.25));
    assert_eq!(k.quad.frame().damage, None);
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("wgpu validation: {error}");
    }
}

/// A small atlas evicts and reuses cells while one label changes each
/// frame and the rest of the target is kept: glyphs drawn earlier must not
/// pick up the cells later glyphs took over.
#[test]
fn atlas_eviction_under_a_kept_target_leaves_no_stale_glyph() {
    let Some(mut k) = Kept::new(|d| QuadRenderer::with_atlas_budget(d, FORMAT, 2 * 128 * 128))
    else {
        return;
    };
    let id = Affine::IDENTITY;
    let (mut partial, mut evicted) = (0, false);
    for i in 0..90 {
        // Every few frames a new size fills the atlas with fresh glyphs.
        let label = format!("Gain {:.2} dB {}", i as f32 * 0.37, "x".repeat(i % 7));
        let scene = sized_cards(&label, 9. + (i / 3 % 12) as f64, 0.5, None);
        k.check(&format!("frame {i}"), &scene.paint, id);
        let frame = k.quad.frame();
        partial += usize::from(frame.damage.is_some());
        evicted |= i > 70 && frame.rasterised > 0;
    }
    assert!(partial > 60, "only {partial} partial frames");
    assert!(evicted, "the atlas never had to rasterise again");
}

/// One draw-list memo per knob, as a host keeps them: an unchanged knob
/// hands back the same paths.
type Memos = Vec<mui_scene::CanvasCache<u64>>;

fn memos() -> Memos {
    (0..8).map(|_| mui_scene::CanvasCache::new()).collect()
}

/// Knobs whose value arcs are `amounts`, over a star-clipped panel whose
/// star has `points` points.
fn knobs(memos: &Memos, amounts: &[f64], points: usize) -> ResolvedScene {
    use std::f64::consts::PI;
    let mut kids: Vec<El> = amounts
        .iter()
        .enumerate()
        .map(|(i, &amount)| {
            mui_scene::canvas_keyed(&memos[i], amount.to_bits(), move |s: Size| {
                let c = Point::new(s.width / 2., s.height / 2.);
                let arc = |sweep: f64| {
                    Path::polyline(
                        (0..=48).map(|k| {
                            let a = 0.75 * PI + sweep * f64::from(k) / 48.;
                            Point::new(c.x + a.cos() * 18., c.y + a.sin() * 18.)
                        }),
                        false,
                    )
                };
                vec![
                    Draw::stroke(arc(1.5 * PI), Color::srgba(1., 1., 1., 0.15), 3.),
                    Draw::stroke(arc(1.5 * PI * amount), Color::srgba(0.3, 0.8, 1., 0.85), 3.),
                ]
            })
            .size(46., 46.)
            .id(format!("knob{i}"))
            .at(6. + 50. * (i % 6) as f64, 6. + 50. * (i / 6) as f64)
        })
        .collect();
    let star = move |s: Size| {
        let c = Point::new(s.width / 2., s.height / 2.);
        Path::polyline(
            (0..2 * points).map(|i| {
                let a = PI * i as f64 / points as f64;
                let r = if i % 2 == 0 { 40. } else { 18. };
                Point::new(c.x + a.cos() * r, c.y + a.sin() * r)
            }),
            true,
        )
    };
    kids.push(
        stack![
            block(120., 20.)
                .radius(0.)
                .fill(Color::srgb(0.9, 0.3, 0.2))
                .at(-10., 20.),
            block(40., 40.)
                .radius(8.)
                .fill(Color::srgb(0.2, 0.8, 0.4))
                .at(30., 40.),
        ]
        .size(90., 90.)
        .outline(star)
        .fill(Color::srgb(0.2, 0.25, 0.4))
        .clip()
        .at(220., 100.),
    );
    let spec = SceneSpec::new(ground(kids)).offered(Size::new(W, H));
    mui_scene::resolve(&spec).expect("resolves")
}

#[test]
fn a_kept_target_matches_a_whole_redraw_after_path_changes() {
    let Some(mut k) = Kept::new(|d| QuadRenderer::new(d, FORMAT)) else {
        return;
    };
    let scope = k.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let id = Affine::IDENTITY;
    let m = memos();
    let mut amounts: Vec<f64> = (0..8).map(|i| f64::from(i) / 8.).collect();
    k.check("paths first", &knobs(&m, &amounts, 5).paint, id);
    assert!(k.quad.frame().paths_built > 0);

    // The same scene, resolved again: nothing drawn, nothing built.
    k.check("paths unchanged", &knobs(&m, &amounts, 5).paint, id);
    assert!(!k.quad.frame().drawn, "{:?}", k.quad.frame());

    // One knob turning, in the middle of the list: a partial frame that
    // builds only that knob's two arcs, and the star (resolved afresh each
    // frame here, so its outline is a new, equal path).
    for step in 0..5 {
        amounts[3] = 0.1 + 0.2 * f64::from(step);
        k.check(
            &format!("knob turn {step}"),
            &knobs(&m, &amounts, 5).paint,
            id,
        );
        let frame = k.quad.frame();
        let damage = frame.damage.expect("a partial frame");
        assert!(damage[2] * damage[3] < SIZE[0] * SIZE[1] / 8, "{damage:?}");
        assert!(frame.paths_built <= 3, "{frame:?}");
    }

    // The path clip changes: a whole redraw.
    k.check("clip changed", &knobs(&m, &amounts, 7).paint, id);
    assert_eq!(k.quad.frame().damage, None);
    let back = knobs(&m, &amounts, 5);
    k.check("clip back", &back.paint, id);

    // The same list scrolled by a fraction: records are kept, only placed
    // anew.
    k.check(
        "paths scrolled",
        &back.paint,
        Affine::translate((0.37, 1.61)),
    );
    assert_eq!(k.quad.frame().paths_built, 0, "{:?}", k.quad.frame());
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("wgpu validation: {error}");
    }
}

/// A path texture with room for only a few records: knobs turning in turn
/// reuse the space of records no longer drawn, a frame that needs more
/// than fits empties it and builds again, and the kept target never shows
/// a stale or missing arc.
#[test]
fn path_eviction_under_a_kept_target_leaves_no_stale_arc() {
    // Twelve rows of 256 texels: one frame's 17 records and little more.
    let Some(mut k) = Kept::new(|d| QuadRenderer::with_budgets(d, FORMAT, ATLAS, 12 * 256 * 16))
    else {
        return;
    };
    let id = Affine::IDENTITY;
    let m = memos();
    let mut amounts: Vec<f64> = (0..8).map(|i| f64::from(i) / 8.).collect();
    let (mut partial, mut built) = (0, 0);
    for i in 0..30 {
        amounts[i % 8] = (amounts[i % 8] + 0.37) % 1.;
        k.check(&format!("evict {i}"), &knobs(&m, &amounts, 5).paint, id);
        let frame = k.quad.frame();
        partial += usize::from(frame.damage.is_some());
        built += frame.paths_built;
        assert!(frame.path_bytes <= 12 * 256 * 16, "{frame:?}");
    }
    assert!(partial > 20, "only {partial} partial frames");
    // Far more records than the texture holds were built: space was reused.
    assert!(built > 50, "only {built} records built");
}

/// Stripes under a frosted modal fading in, as an editor under a dialog.
/// `under` tints a stripe the modal's blur reaches, `away` one it does not,
/// `inner` the modal's label; `opacity` fades the modal.
fn modal(under: f32, away: f32, inner: f32, opacity: f32) -> ResolvedScene {
    let mut kids: Vec<El> = (0..16)
        .map(|i| {
            let tint = match i {
                5 => under,
                15 => away,
                _ => 0.5,
            };
            block(10., H)
                .radius(0.)
                .fill(Color::srgb(tint, 0.3 + 0.04 * i as f32, 0.6))
                .at(20. * i as f64, 0.)
        })
        .collect();
    kids.push(
        stack![
            block(90., 18.)
                .radius(4.)
                .fill(Color::srgb(inner, 0.9, 0.9))
                .at(14., 14.)
        ]
        .size(150., 100.)
        .radius(12.)
        .fill(Color::srgba(0.1, 0.1, 0.12, 0.45))
        .shadow(Shadow::soft(6.))
        .backdrop_blur(6.)
        .opacity(opacity)
        .at(60., 50.),
    );
    let spec = SceneSpec::new(ground(kids)).offered(Size::new(W, H));
    mui_scene::resolve(&spec).expect("resolves")
}

#[test]
fn a_kept_target_matches_a_whole_redraw_under_layers_and_backdrops() {
    let Some(mut k) = Kept::new(|d| QuadRenderer::new(d, FORMAT)) else {
        return;
    };
    let scope = k.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let id = Affine::IDENTITY;
    k.check("modal first", &modal(0.5, 0.5, 0.9, 0.3).paint, id);
    let first = k.quad.frame();
    assert!(first.passes >= 5, "{first:?}");
    k.check("modal unchanged", &modal(0.5, 0.5, 0.9, 0.3).paint, id);
    assert!(!k.quad.frame().drawn);

    // Fading in: the modal's target and its blur are kept; only the frame
    // redraws, around the modal.
    for (step, opacity) in [0.4, 0.55, 0.7, 0.85].into_iter().enumerate() {
        k.check(
            &format!("fade {step}"),
            &modal(0.5, 0.5, 0.9, opacity).paint,
            id,
        );
        let frame = k.quad.frame();
        assert_eq!(frame.passes, 1, "{frame:?}");
        assert!(frame.damage.is_some(), "{frame:?}");
    }
    // A stripe under the blur: the backdrop and the layer holding it are
    // drawn again.
    k.check("under", &modal(0.1, 0.5, 0.9, 0.85).paint, id);
    let frame = k.quad.frame();
    assert!(frame.passes >= 5, "{frame:?}");
    // A stripe the blur does not reach: neither is.
    k.check("away", &modal(0.1, 0.9, 0.9, 0.85).paint, id);
    let frame = k.quad.frame();
    assert_eq!(frame.passes, 1, "{frame:?}");
    // The modal's label: its layer alone.
    k.check("inside", &modal(0.1, 0.9, 0.2, 0.85).paint, id);
    let frame = k.quad.frame();
    assert_eq!(frame.passes, 2, "{frame:?}");
    // Faded out to nothing and back.
    k.check("gone", &modal(0.1, 0.9, 0.2, 0.).paint, id);
    k.check("back", &modal(0.1, 0.9, 0.2, 0.6).paint, id);
    // Scrolled: every target drawn again.
    k.check(
        "scrolled",
        &modal(0.1, 0.9, 0.2, 0.6).paint,
        Affine::translate((3.5, -2.25)),
    );
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("wgpu validation: {error}");
    }
}

/// A morphing weld redraws its texture and its slot; a host texture drawn
/// into again redraws what samples it. Each frame against a whole redraw by
/// a fresh pass with a fresh weld pool.
#[test]
fn a_kept_target_matches_a_whole_redraw_as_textures_change() {
    use mui_vello::effects::{Budget, WeldTextures};
    let Some(mut k) = Kept::new(|d| QuadRenderer::new(d, FORMAT)) else {
        return;
    };
    let pool = |k: &Kept| {
        pollster::block_on(WeldTextures::new(&k.device, &k.queue, Budget::default())).expect("pool")
    };
    let mut welds = pool(&k);
    let host = k.device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 4,
            height: 4,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    let fill = |k: &Kept, rgba: [u8; 4]| {
        k.queue.write_texture(
            host.as_image_copy(),
            &rgba.repeat(16),
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(16),
                rows_per_image: None,
            },
            host.size(),
        );
    };
    let plate = |id: &str| block(60., 50.).radius(10.).fill(Role::Primary).id(id);
    let root = stack![
        row![plate("a"), plate("b")]
            .gap(12.)
            .weld(Weld::all().reach(20.).blend(40.))
            .id("join")
            .at(20., 20.),
        block(60., 40.)
            .radius(6.)
            .fill(Fill::Image(Arc::new(Image::texture(7, 4, 4)), Fit::Fill))
            .at(240., 140.),
    ]
    .size(W, H);
    let spec = SceneSpec::new(ground(vec![root])).offered(Size::new(W, H));
    let mut scene =
        mui_scene::resolve(&spec.weld_backend(mui_scene::WeldBackend::AnalyticGpu)).expect("scene");
    let check = |k: &mut Kept, welds: &mut WeldTextures, scene: &ResolvedScene, what: &str| {
        let id = Affine::IDENTITY;
        k.quad
            .render_scene(&k.device, &k.queue, scene, welds, id, &k.view, SIZE)
            .expect("quad render");
        let got = readback(&k.device, &k.queue, &k.texture, SIZE);
        let fresh = target(&k.device, SIZE);
        let view = fresh.create_view(&wgpu::TextureViewDescriptor::default());
        let mut quad = QuadRenderer::new(&k.device, FORMAT);
        quad.set_texture(7, &host);
        quad.render_scene(&k.device, &k.queue, scene, &mut pool(k), id, &view, SIZE)
            .expect("quad render");
        let want = readback(&k.device, &k.queue, &fresh, SIZE);
        if got != want {
            let dir = out_dir();
            save(&dir.join(format!("retained-{what}-got.png")), SIZE, &got);
            save(&dir.join(format!("retained-{what}-want.png")), SIZE, &want);
            panic!("{what}: stale pixels; images in {}", dir.display());
        }
    };
    fill(&k, [255, 0, 0, 255]);
    k.quad.set_texture(7, &host);
    check(&mut k, &mut welds, &scene, "textures first");
    check(&mut k, &mut welds, &scene, "textures unchanged");
    assert!(!k.quad.frame().drawn, "{:?}", k.quad.frame());
    for step in 1..=4 {
        scene
            .set_weld_morph("join", f64::from(step) / 4.)
            .expect("morph");
        check(&mut k, &mut welds, &scene, &format!("morph {step}"));
        let frame = k.quad.frame();
        let damage = frame.damage.expect("a partial frame");
        // The weld's slot, not the host texture's corner.
        assert!(damage[0] < 200 && damage[1] < 120, "{damage:?}");
    }
    fill(&k, [0, 0, 255, 128]);
    k.quad.set_texture(7, &host);
    check(&mut k, &mut welds, &scene, "host redrawn");
    let damage = k.quad.frame().damage.expect("a partial frame");
    assert!(damage[0] >= 200, "{damage:?}");
}
