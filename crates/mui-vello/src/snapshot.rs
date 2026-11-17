use super::*;
use mui_material::prelude::*;
use mui_scene::{ResolvedScene, TextGlyph};
use vello_common::pixmap::Pixmap;

#[test]
fn subtree_rotation_preserves_bitmap_vector_glyphs_and_device_paint() {
    use mui_scene::{Image, Vector, VectorCommand};
    let bitmap = Arc::new(Image::rgba(1, 1, Arc::<[u8]>::from([255, 0, 0, 255])).unwrap());
    let vector = Arc::new(
        Vector::new(
            6.,
            2.,
            vec![VectorCommand::Fill {
                path: Rect::new(0., 0., 6., 2.).to_path(0.01),
                transform: Affine::IDENTITY,
                brush: peniko::Brush::Solid(peniko::Color::from_rgba8(0, 255, 0, 255)),
                brush_transform: Affine::IDENTITY,
                rule: peniko::Fill::NonZero,
            }],
        )
        .unwrap(),
    );
    let spec = |angle| {
        SceneSpec::new(
            stack([stack([
                block(6., 2.)
                    .fill(Fill::Image(bitmap.clone(), Fit::Fill))
                    .anchor(Align::Start, Align::Start)
                    .id("bitmap"),
                block(6., 2.)
                    .offset(8., 0.)
                    .fill(Fill::Vector(vector.clone(), Fit::Fill))
                    .anchor(Align::Start, Align::Start)
                    .id("vector"),
                text("R")
                    .text_size(6.)
                    .offset(0., 3.)
                    .fill(Color::srgb(1., 1., 1.))
                    .anchor(Align::Start, Align::Start)
                    .id("glyph"),
            ])
            .size(20., 8.)
            .offset(10., 10.)
            .anchor(Align::Start, Align::Start)
            .rotation(angle)])
            .size(40., 40.)
            .clip(),
        )
        .font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap())
    };
    let plain = resolve(&spec(0.)).unwrap();
    let rotated = resolve(&spec(std::f64::consts::FRAC_PI_2)).unwrap();
    for key in ["bitmap", "vector", "glyph"] {
        assert_eq!(
            plain.surface(key).unwrap().frame,
            rotated.surface(key).unwrap().frame
        );
        let before = plain
            .paint
            .iter()
            .find(|p| p.key.as_str() == key && matches!(p.layer, Layer::Fill | Layer::Text))
            .unwrap();
        let after = rotated
            .paint
            .iter()
            .find(|p| p.key.as_str() == key && matches!(p.layer, Layer::Fill | Layer::Text))
            .unwrap();
        assert_ne!(before.transform, after.transform);
        assert_eq!(
            before.paint, after.paint,
            "source artwork is not raster-rotated or replaced"
        );
        assert!(scene_box(after, &bez_path(&after.path, ARC_TOLERANCE).unwrap()).is_finite());
    }
    assert!(
        rotated.paint.iter().any(
            |p| p.layer == Layer::Text && p.text.as_ref().is_some_and(|t| !t.glyphs.is_empty())
        )
    );
    for scale in [1u16, 2] {
        let mut ctx = vello_cpu::RenderContext::new(40 * scale, 40 * scale);
        let mut resources = vello_cpu::Resources::default();
        paint(
            &mut Cpu {
                ctx: &mut ctx,
                resources: &mut resources,
                cache: &mut Cache::default(),
            },
            &rotated,
            Affine::scale(scale as f64),
        )
        .unwrap();
        ctx.flush();
        let mut pixels = Pixmap::new(40 * scale, 40 * scale);
        ctx.render(&mut pixels, &mut resources);
        let at = |x: usize, y: usize| {
            pixels.data()[y * scale as usize * (40 * scale) as usize + x * scale as usize]
        };
        let red = at(23, 5);
        let green = at(23, 13);
        assert!(
            red.r > 200 && red.g < 30,
            "bitmap rotation lost at{scale}x: {red:?}"
        );
        assert!(
            green.g > 200 && green.r < 30,
            "vector rotation lost at{scale}x: {green:?}"
        );
        assert_eq!(
            at(11, 11).a,
            0,
            "paint stayed at the unrotated bitmap position"
        );
        assert!(
            (3..10).any(|y| (14..22).any(|x| {
                let p = at(x, y);
                p.r > 80 && p.g > 80 && p.b > 80
            })),
            "glyphs must paint in their rotated region at{scale}x"
        );
    }
}

/// The whole stack on the CPU: a filled card reaches the pixels, its ink
/// reads against it, and the rounded corner stays clear.
#[test]
fn a_card_lands_on_the_pixmap() {
    let root = col([text("hi").id("t")])
        .pad(20.)
        .fill(Role::Primary)
        .stroke(Role::Ink)
        .id("card");
    let mut spec = SceneSpec::new(root).offered(Size::new(120., 60.));
    spec.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let scene = resolve(&spec).unwrap();
    assert!(
        scene
            .paint
            .iter()
            .any(|p| p.layer == mui_scene::Layer::Text)
    );

    let mut ctx = vello_cpu::RenderContext::new(120, 60);
    let mut res = vello_cpu::Resources::default();
    paint(
        &mut Cpu {
            ctx: &mut ctx,
            resources: &mut res,
            cache: &mut Cache::default(),
        },
        &scene,
        Affine::IDENTITY,
    )
    .unwrap();
    ctx.flush();
    let mut pix = Pixmap::new(120, 60);
    ctx.render(&mut pix, &mut res);
    let at = |x: usize, y: usize| pix.data()[y * 120 + x];
    let want = Theme::default().palette.primary().to_srgb().to_rgba8();
    let got = at(60, 8);
    assert!(
        (got.r as i32 - want.r as i32).abs() <= 1,
        "{got:?} vs {want:?}"
    );
    assert_eq!(at(0, 0).a, 0, "corner is rounded away");
}

/// Render `spec` on the CPU and hand back the pixels.
fn pixels(spec: &SceneSpec, w: u16, h: u16) -> Pixmap {
    let scene = resolve(spec).unwrap();
    pixels_scene(&scene, w, h)
}

fn pixels_scene(scene: &ResolvedScene, w: u16, h: u16) -> Pixmap {
    // One thread: `cpu-threads` would otherwise leave out filter layers.
    let one = vello_cpu::RenderSettings {
        num_threads: 0,
        ..Default::default()
    };
    let mut ctx = vello_cpu::RenderContext::new_with(w, h, one);
    let mut res = vello_cpu::Resources::default();
    paint(
        &mut Cpu {
            ctx: &mut ctx,
            resources: &mut res,
            cache: &mut Cache::default(),
        },
        scene,
        Affine::IDENTITY,
    )
    .unwrap();
    ctx.flush();
    let mut pix = Pixmap::new(w, h);
    ctx.render(&mut pix, &mut res);
    pix
}

/// Text reaches the pixels as a glyph run: Vello hints and caches the
/// outlines per font blob, so ink on the pixmap means the run drew.
#[test]
fn a_glyph_run_lands_pixels() {
    let mut spec = SceneSpec::new(text("HI").fill(Role::Ink).id("t")).offered(Size::new(80., 40.));
    spec.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let scene = resolve(&spec).unwrap();
    assert!(
        scene.paint.iter().any(|p| p.text.is_some()),
        "no glyphs to draw"
    );
    let pix = pixels(&spec, 80, 40);
    assert!(pix.data().iter().any(|p| p.a > 0), "the run drew nothing");
}

#[test]
fn a_gpos_mark_is_rendered_at_its_shaped_y_offset() {
    let mut spec =
        SceneSpec::new(text("ש\u{05b8}").fill(Role::Ink).id("t")).offered(Size::new(80., 40.));
    spec.font = Some(Font::new(ttf_inter::REGULAR).unwrap());
    let scene = resolve(&spec).unwrap();
    let text = scene
        .paint
        .iter()
        .find_map(|p| p.text.as_ref())
        .expect("text layer");
    assert!(
        text.glyphs.iter().any(|glyph| glyph.y.abs() > 0.01),
        "scene dropped GPOS y offsets: {:?}",
        text.glyphs
    );
    let mut without_offsets = scene.clone();
    for painted in &mut without_offsets.paint {
        let Some(text) = painted.text.as_mut() else {
            continue;
        };
        text.glyphs = text
            .glyphs
            .iter()
            .map(|glyph| TextGlyph { y: 0., ..*glyph })
            .collect();
    }
    let positioned = pixels_scene(&scene, 80, 40);
    let flattened = pixels_scene(&without_offsets, 80, 40);
    assert_ne!(
        positioned.data(),
        flattened.data(),
        "mark offset had no raster effect"
    );
}

#[test]
fn a_missing_primary_glyph_uses_the_selected_fallback_font() {
    let root = text("A😀").fill(Role::Ink).id("t");
    let mut with_fallback = SceneSpec::new(root.clone()).offered(Size::new(100., 40.));
    with_fallback.font = Some(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    with_fallback
        .fallback_fonts
        .push(Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap());
    let fallback_scene = resolve(&with_fallback).unwrap();
    let glyphs = fallback_scene
        .paint
        .iter()
        .find_map(|p| p.text.as_ref())
        .expect("text layer")
        .glyphs
        .clone();
    assert!(
        glyphs.iter().any(|glyph| glyph.font == 1),
        "no fallback glyph was selected"
    );

    let mut primary_only = with_fallback.clone();
    primary_only.fallback_fonts.clear();
    let primary_scene = resolve(&primary_only).unwrap();
    assert_ne!(
        pixels_scene(&fallback_scene, 100, 40).data(),
        pixels_scene(&primary_scene, 100, 40).data(),
        "fallback output equals the primary .notdef output"
    );
}

/// The same retained command buffer reaches both raster and GPU encoding;
/// device scale changes paths, not a pre-rasterized image's resolution.
#[test]
fn retained_vector_preserves_gradient_hole_clip_and_device_scale() {
    use mui_scene::{Vector, VectorCommand};
    use peniko::{Brush, Fill as Winding};
    let mut hole = Rect::new(0., 0., 24., 16.).to_path(0.01);
    hole.extend(Rect::new(8., 4., 16., 12.).to_path(0.01));
    let red = AlphaColor::<Srgb>::new([1., 0., 0., 1.]);
    let blue = AlphaColor::<Srgb>::new([0., 0., 1., 1.]);
    let vector = Arc::new(
        Vector::new(
            24.,
            16.,
            vec![
                VectorCommand::PushLayer {
                    path: Rect::new(0., 0., 24., 14.).to_path(0.01),
                    transform: Affine::IDENTITY,
                    blend: peniko::BlendMode::default(),
                    alpha: 0.5,
                },
                VectorCommand::Fill {
                    path: hole,
                    transform: Affine::IDENTITY,
                    brush: Brush::Gradient(
                        peniko::Gradient::new_linear((0., 0.), (24., 0.)).with_stops([red, blue]),
                    ),
                    brush_transform: Affine::IDENTITY,
                    rule: Winding::EvenOdd,
                },
                VectorCommand::Stroke {
                    path: Rect::new(1., 1., 22., 13.).to_path(0.01),
                    transform: Affine::translate((0.5, 0.5)),
                    brush: Brush::Solid(peniko::Color::WHITE),
                    brush_transform: Affine::IDENTITY,
                    stroke: Stroke::new(0.5),
                },
                VectorCommand::PopLayer,
            ],
        )
        .unwrap(),
    );
    let root = block(48., 32.).fill(Fill::Vector(vector.clone(), Fit::Fill));
    let scene = resolve(&SceneSpec::new(root).offered(Size::new(48., 32.))).unwrap();
    assert!(
        scene.paint.iter().any(
            |p| matches!(&p.paint, Paint::Vector { vector: v, .. } if Arc::ptr_eq(v, &vector))
        )
    );
    for scale in [1u16, 2] {
        let mut ctx = vello_cpu::RenderContext::new(48 * scale, 32 * scale);
        let mut resources = vello_cpu::Resources::default();
        paint(
            &mut Cpu {
                ctx: &mut ctx,
                resources: &mut resources,
                cache: &mut Cache::default(),
            },
            &scene,
            Affine::scale(f64::from(scale)),
        )
        .unwrap();
        ctx.flush();
        let mut pixels = Pixmap::new(48 * scale, 32 * scale);
        ctx.render(&mut pixels, &mut resources);
        let at = |x: usize, y: usize| {
            pixels.data()[y * scale as usize * (48 * scale) as usize + x * scale as usize]
        };
        let left = at(8, 4);
        let right = at(40, 4);
        assert!(
            left.r > left.b && right.b > right.r,
            "gradient lost at {scale}x: {left:?}/{right:?}"
        );
        assert!(
            (120..=135).contains(&left.a),
            "group opacity lost at {scale}x: {left:?}"
        );
        assert_eq!(at(24, 16).a, 0, "even-odd hole lost at {scale}x");
        assert_eq!(at(24, 30).a, 0, "group clip lost at {scale}x");
    }
}

#[test]
fn filtered_vector_requests_physical_extent_for_cpu_and_gpu() {
    #[derive(Debug)]
    struct Source {
        requests: std::sync::Mutex<Vec<(u32, u32)>>,
        images: [mui_scene::Image; 2],
        revision: std::sync::atomic::AtomicU64,
    }
    impl mui_scene::RasterSource for Source {
        fn prepared_image(
            &self,
            width: u32,
            height: u32,
        ) -> Result<Option<mui_scene::Image>, String> {
            self.requests.lock().unwrap().push((width, height));
            Ok(self
                .images
                .iter()
                .find(|i| i.width == width && i.height == height)
                .cloned())
        }
        fn revision(&self) -> u64 {
            self.revision.load(std::sync::atomic::Ordering::Acquire)
        }
        fn retained_bytes(&self) -> usize {
            0
        }
    }
    let source = Arc::new(Source {
        requests: std::sync::Mutex::new(Vec::new()),
        revision: std::sync::atomic::AtomicU64::new(0),
        images: [(20, 15), (40, 30)].map(|(w, h)| {
            mui_scene::Image::rgba(w, h, [255, 0, 0, 128].repeat((w * h) as usize)).unwrap()
        }),
    });
    let vector = Arc::new(mui_scene::Vector::filtered(10., 10., source.clone()).unwrap());
    let scene = resolve(
        &SceneSpec::new(block(20., 15.).fill(Fill::Vector(vector.clone(), Fit::Fill)))
            .offered(Size::new(20., 15.)),
    )
    .unwrap();
    for scale in [1u16, 2] {
        let mut ctx = vello_cpu::RenderContext::new(20 * scale, 15 * scale);
        let mut resources = vello_cpu::Resources::default();
        paint(
            &mut Cpu {
                ctx: &mut ctx,
                resources: &mut resources,
                cache: &mut Cache::default(),
            },
            &scene,
            Affine::scale(scale as f64),
        )
        .unwrap();
        ctx.flush();
        let mut pixels = Pixmap::new(20 * scale, 15 * scale);
        ctx.render(&mut pixels, &mut resources);
        let center =
            pixels.data()[7 * scale as usize * (20 * scale) as usize + 10 * scale as usize];
        assert_eq!(center.r, 128, "straight alpha is premultiplied once");
        assert_eq!(center.a, 128);
    }
    assert_eq!(*source.requests.lock().unwrap(), [(20, 15), (40, 30)]);
    source
        .revision
        .fetch_add(1, std::sync::atomic::Ordering::Release);
    let ready_scene = resolve(
        &SceneSpec::new(block(20., 15.).fill(Fill::Vector(vector, Fit::Fill)))
            .offered(Size::new(20., 15.)),
    )
    .unwrap();
    assert_ne!(
        scene.paint, ready_scene.paint,
        "ready pixels on same vector Arc must invalidate retained GPU paint"
    );
}

/// A gradient shadow keeps its alpha. Both backends paint opaque black
/// for a non-solid brush on a blurred rect, so the brush is collapsed to
/// its solid stand-in first.
#[test]
fn a_gradient_shadow_does_not_paint_black() {
    let faint = mui_scene::Color::oklcha(0.0, 0.0, 0.0, 0.1);
    let root = block(20., 20.)
        .fill(Role::Primary)
        .shadow(Shadow {
            dy: 8.,
            fill: mui_scene::Gradient::vertical(faint, faint.with_alpha(0.0)).into(),
            ..Shadow::soft(4.)
        })
        .id("card");
    let pix = pixels(&SceneSpec::new(root).offered(Size::new(20., 20.)), 40, 40);
    let below = pix.data()[30 * 40 + 10];
    assert!(below.a < 40, "the shadow went opaque black: {below:?}");
}

/// The final rounded contour leaves no square shadow in its cutout.
#[test]
fn a_welded_drop_shadow_follows_the_rounded_outline_at_its_cutout() {
    let shadow = Shadow {
        dy: 1.,
        fill: Color::srgba(0., 0., 0., 0.4).into(),
        ..Shadow::soft(2.)
    };
    let spec = |body| SceneSpec::new(stack![body].pad(20.)).offered(Size::new(120., 80.));
    let welded = resolve(&spec(
        row([block(40., 40.).radius(0.), block(40., 40.).radius(0.)])
            .gap(0.)
            .union(Color::srgb(1., 1., 1.))
            .radius(18.)
            .shadow(shadow.clone())
            .id("rounded"),
    ))
    .unwrap();
    let outline = welded
        .paint
        .iter()
        .find(|p| p.key.as_str() == "rounded" && p.layer == Layer::Fill)
        .unwrap()
        .path
        .clone();
    // An independently authored copy of the final shape is the shadow
    // reference: the weld's original square participants cannot back it.
    let expected = pixels(
        &spec(
            block(80., 40.)
                .outline(move |_| (*outline).clone())
                .fill(Color::srgb(1., 1., 1.))
                .shadow(shadow),
        ),
        120,
        80,
    );
    let actual = pixels_scene(&welded, 120, 80);
    let at = |pix: &Pixmap, x: usize, y: usize| pix.data()[y * 120 + x].a;
    assert!(at(&expected, 98, 22) < 10, "rounded cutout is clear");
    assert_eq!(
        at(&actual, 98, 22),
        at(&expected, 98, 22),
        "a square backing filled the rounded cutout"
    );
    assert!(at(&actual, 60, 62) > 0, "the soft offset shadow remains");
    assert_eq!(
        actual.data(),
        expected.data(),
        "shadow follows the final outline"
    );
}

/// A welded outline's shadow is soft under the weld, falls off with
/// distance, and stays nowhere near the outline's own alpha -- which
/// is what a sharp copy would have given.
#[test]
fn a_welded_shadow_blurs() {
    let root = row([block(20., 20.).id("a"), block(20., 40.).id("b")])
        .union(Role::Surface)
        .shadow(Shadow::soft(12.))
        .id("weld");
    let spec = SceneSpec::new(root).offered(Size::new(40., 40.));
    let pix = pixels(&spec, 60, 80);
    let a = |y: usize| pix.data()[y * 60 + 10].a;
    assert!(a(29) > 0, "the outline itself is gone");
    let (near, far) = (a(34), a(46));
    assert!(near > 0, "the welded shadow is still dropped");
    assert!(far < near, "it does not fall off: {near} then {far}");
    assert!(near < a(29) / 2, "that is a sharp copy, not a blur: {near}");
}

/// A custom outline's drop shadow is its own path blurred: shade just
/// past the path's edge, falling off, and soft rather than a sharp copy.
#[test]
fn a_custom_outline_drops_a_blurred_shadow() {
    let diamond = |s: Size| {
        let p = mui_geometry::Point::new;
        let (w, h) = (s.width, s.height);
        mui_geometry::Path::default()
            .move_to(p(w / 2., 0.))
            .line_to(p(w, h / 2.))
            .line_to(p(w / 2., h))
            .line_to(p(0., h / 2.))
            .close()
    };
    let root = block(40., 40.)
        .outline(diamond)
        .fill(Role::Surface)
        .shadow(Shadow::soft(6.))
        .id("gem");
    let spec = SceneSpec::new(stack![root].pad(20.)).offered(Size::new(80., 80.));
    let pix = pixels(&spec, 80, 80);
    // Down the diamond's vertical axis: its tip is at y = 60.
    let a = |y: usize| pix.data()[y * 80 + 40].a;
    assert!(a(55) > 200, "the outline itself is gone");
    let (near, far) = (a(62), a(72));
    assert!(near > 0, "the custom-path shadow is dropped");
    assert!(far < near, "it does not fall off: {near} then {far}");
    assert!(near < 200, "that is a sharp copy, not a blur: {near}");
}

/// A multiply layer darkens what is under it. The child is the same grey
/// as the ground, so painting it straight would leave the pixel alone:
/// only the blend can make it darker.
#[test]
fn a_multiply_layer_darkens_what_is_under_it() {
    let grey = Color::oklcha(0.7, 0., 0., 1.);
    let root = stack([
        block(40., 40.).fill(grey).radius(0.).id("ground"),
        block(20., 20.)
            .fill(grey)
            .radius(0.)
            .blend(Mix::Multiply)
            .id("dim"),
    ])
    .id("root");
    let pix = pixels(&SceneSpec::new(root).offered(Size::new(40., 40.)), 40, 40);
    let at = |x: usize, y: usize| pix.data()[y * 40 + x];
    let (ground, inside) = (at(2, 2), at(20, 20));
    assert!(ground.r > 100, "no ground to darken: {ground:?}");
    assert!(
        inside.r < ground.r - 20,
        "the same grey did not multiply: {inside:?} vs {ground:?}"
    );
    assert_eq!(at(38, 38), ground, "the layer leaked outside its node");
}

/// A 2x2 image stretched over a leaf: each source pixel owns a quadrant,
/// and the leaf's rounded corner still cuts the image away.
#[test]
fn an_image_fill_lands_the_right_pixel_in_each_quadrant() {
    #[rustfmt::skip]
    let px: Vec<u8> = vec![
        255, 0, 0, 255,  0, 255, 0, 255,
        0, 0, 255, 255,  255, 255, 255, 255,
    ];
    let img = std::sync::Arc::new(mui_scene::Image::rgba(2, 2, px).unwrap());
    let root = block(20., 20.)
        .fill(Fill::Image(img.clone(), Fit::Fill))
        .radius(6.)
        .id("img");
    let pix = pixels(&SceneSpec::new(root).offered(Size::new(20., 20.)), 20, 20);
    let at = |x: usize, y: usize| pix.data()[y * 20 + x];
    // Nearest neighbour is not promised, so sample well inside a quadrant.
    for ((x, y), want) in [
        ((4, 4), [255, 0, 0]),
        ((15, 4), [0, 255, 0]),
        ((4, 15), [0, 0, 255]),
        ((15, 15), [255, 255, 255]),
    ] {
        let got = at(x, y);
        assert_eq!([got.r, got.g, got.b], want, "at {x},{y}: {got:?}");
    }
    assert_eq!(at(0, 0).a, 0, "the image spilled past the rounded corner");

    // Contain letterboxes rather than smearing the edge pixels: a 2x2
    // image in a 40x20 box leaves the sides clear.
    let wide = block(40., 20.)
        .fill(Fill::Image(img, Fit::Contain))
        .radius(0.)
        .id("wide");
    let pix = pixels(&SceneSpec::new(wide).offered(Size::new(40., 20.)), 40, 20);
    let at = |x: usize, y: usize| pix.data()[y * 40 + x];
    assert_eq!(at(1, 10).a, 0, "contain smeared into the letterbox");
    assert!(
        at(14, 5).r > 200,
        "the image itself is missing: {:?}",
        at(14, 5)
    );
}

/// The CPU path lets go of a buffer the app has dropped: its pixmap goes
/// on the next image lookup, so a panel handing over a fresh frame every
/// frame does not keep one per frame.
#[test]
fn the_pixmap_cache_drops_a_buffer_the_app_let_go_of() {
    let img = |v: u8| mui_scene::Image::rgba(1, 1, vec![v, v, v, 255]).unwrap();
    let (a, b) = (img(1), img(2));
    let gone = Arc::downgrade(&a.rgba);
    let mut ctx = vello_cpu::RenderContext::new(1, 1);
    let mut resources = vello_cpu::Resources::default();
    let mut cache = Cache::default();
    let mut cpu = Cpu {
        ctx: &mut ctx,
        resources: &mut resources,
        cache: &mut cache,
    };
    assert!(cpu.image(&a).is_some());
    assert!(cpu.image(&a).is_some());
    assert_eq!(cpu.cache.images.len(), 1, "a hit added an entry");
    drop(a);
    assert!(gone.upgrade().is_none(), "the cache kept the buffer alive");
    assert!(cpu.image(&b).is_some());
    assert_eq!(
        cpu.cache.images.len(),
        1,
        "the dropped buffer kept its entry"
    );
}

/// A clip layer actually clips: the oversized child stops at its parent.
#[test]
fn a_clipped_child_stays_inside_its_parent() {
    let child = block(200., 200.).fill(Role::Ink).id("child");
    let boxed = col([child])
        .size(40., 40.)
        .clip()
        .fill(Role::Surface)
        .anchor(Align::Start, Align::Start)
        .id("box");
    let spec = SceneSpec::new(stack([boxed])).offered(Size::new(80., 80.));
    let scene = resolve(&spec).unwrap();
    let c = scene.surface("child").expect("child").frame;
    assert!(c.right() > 40., "no overflow to clip: {c:?}");
    let pix = pixels(&spec, 80, 80);
    let at = |x: usize, y: usize| pix.data()[y * 80 + x];
    assert!(at(20, 20).a > 0, "nothing drew inside the clip");
    for (x, y) in [(60, 20), (20, 60), (60, 60)] {
        assert_eq!(at(x, y).a, 0, "painted outside the clip at {x},{y}");
    }
}
