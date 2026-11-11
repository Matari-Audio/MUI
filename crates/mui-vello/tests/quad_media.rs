//! Glyph runs and image fills through the quad pass: compared against
//! `vello_cpu` as a sanity check (placement, colour, order), and checked
//! directly where that is cheap: glyphs land on the pen and baseline the
//! scene gave, text is sharp, atlas cells do not bleed, image fits land the
//! right pixel, the atlas evicts under a small budget and a steady frame
//! rasterises and uploads nothing.
#![cfg(all(feature = "gpu-quad", feature = "cpu"))]

mod quad_support;
use quad_support::*;

use mui_scene::prelude::*;
use mui_scene::{Font, Image, ResolvedScene};
use mui_vello::kurbo::Affine;
use mui_vello::quad::{Error, QuadRenderer, Reason};
use std::sync::Arc;

fn hack() -> Font {
    Font::new(epaint_default_fonts::HACK_REGULAR).unwrap()
}

fn inter() -> Font {
    Font::new(ttf_inter::REGULAR).unwrap()
}

/// `root` resolved at `scale` with `font` and an emoji fallback.
fn resolve_with(root: El, scale: f64, font: Font) -> ResolvedScene {
    let mut spec = SceneSpec::new(root).offered(Size::new(W, H)).scale(scale);
    spec.font = Some(font);
    spec.fallback_fonts
        .push(Font::new(epaint_default_fonts::NOTO_EMOJI_REGULAR).unwrap());
    mui_scene::resolve(&spec).expect("resolves")
}

/// Sizes from small UI text to a title, in a static face.
fn sizes(scale: f64) -> ResolvedScene {
    let white = Color::srgb(0.95, 0.95, 0.92);
    let mut kids = Vec::new();
    let mut y = 4.;
    for size in [9., 11., 13., 16., 24., 40.] {
        kids.push(text("Quartz 0.25 dB").text_size(size).fill(white).at(6., y));
        y += size * 1.25;
    }
    resolve_with(ground(kids), scale, hack())
}

/// Weights of a variable face, a run left unhinted, combining marks and
/// GPOS offsets, and a fallback face.
fn faces(scale: f64) -> ResolvedScene {
    let white = Color::srgb(0.95, 0.95, 0.92);
    let kids = vec![
        text("Light 300")
            .text_axis("wght", 300.)
            .text_size(18.)
            .fill(white)
            .at(6., 6.),
        text("Bold 800")
            .text_axis("wght", 800.)
            .text_size(18.)
            .fill(white)
            .at(6., 32.),
        text("Unhinted run")
            .text_size(14.)
            .fill(white)
            .id("soft")
            .at(6., 60.),
        text("x\u{301}\u{302} e\u{301} \u{5e9}\u{5b8}")
            .text_size(20.)
            .fill(white)
            .at(6., 84.),
        text("Fallback A\u{1f600}B")
            .text_size(16.)
            .fill(white)
            .at(6., 116.),
    ];
    let mut scene = resolve_with(ground(kids), scale, inter());
    let glyphs = || {
        scene
            .paint
            .iter()
            .filter_map(|p| p.text.as_ref())
            .flat_map(|t| t.glyphs.iter())
    };
    assert!(
        glyphs().any(|g| g.y.abs() > 0.01),
        "no GPOS y offset to draw"
    );
    assert!(glyphs().any(|g| g.font == 1), "no fallback glyph to draw");
    for p in &mut scene.paint {
        if p.key.as_ref() == "soft"
            && let Some(t) = &mut p.text
        {
            t.hint = false;
        }
    }
    scene
}

/// Text cut by square clips, and translucent text over a gradient.
fn painted(scale: f64) -> ResolvedScene {
    let (red, blue) = (Color::srgb(0.95, 0.2, 0.1), Color::srgb(0.1, 0.3, 0.95));
    let kids = vec![
        stack![
            text("Clipped label text")
                .text_size(16.)
                .fill(Color::srgb(1., 1., 1.))
                .at(-7.5, 3.),
        ]
        .size(90., 14.)
        .radius(0.)
        .clip()
        .at(10.25, 10.5),
        stack![
            block(300., 60.)
                .radius(0.)
                .fill(Gradient::linear(90., [(0., red), (1., blue)])),
            text("Translucent over a ramp")
                .text_size(20.)
                .fill(Color::srgba(1., 1., 1., 0.6))
                .at(8., 18.),
        ]
        .size(300., 60.)
        .at(10., 40.),
    ];
    resolve_with(ground(kids), scale, hack())
}

/// A run painted with a ramp fitted to its box.
fn gradient(scale: f64) -> ResolvedScene {
    let (red, blue) = (Color::srgb(0.95, 0.2, 0.1), Color::srgb(0.1, 0.3, 0.95));
    let kids = vec![
        text("Gradient ink")
            .text_size(28.)
            .fill(Gradient::linear(90., [(0., red), (1., blue)]))
            .at(10.3, 20.),
        text("Radial 0.5")
            .text_size(15.)
            .fill(Gradient::radial(
                (0.5, 0.5),
                0.6,
                [(0., Color::srgb(1., 1., 1.)), (1., red)],
            ))
            .at(10., 90.),
    ];
    resolve_with(ground(kids), scale, hack())
}

fn device_gpu(quad: impl FnOnce(&wgpu::Device) -> QuadRenderer) -> Option<Gpu> {
    let (device, queue) = device()?;
    let quad = quad(&device);
    Some(Gpu {
        device,
        queue,
        quad,
    })
}

fn standard(device: &wgpu::Device) -> QuadRenderer {
    QuadRenderer::new(device, wgpu::TextureFormat::Rgba8Unorm)
}

/// Both renderers draw glyphs from a quarter-pixel horizontal bin with the
/// same hinting, so the masks agree; what is left is how each multiplies and
/// blends 8-bit coverage, and that `vello_cpu` covers a cell's whole-pixel
/// box with a path while the pass reads it texel for texel.
#[test]
fn text_matches_vello_cpu() {
    let Some(mut gpu) = device_gpu(standard) else {
        return;
    };
    let scope = gpu.device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut failures = Vec::new();
    let f = &mut failures;
    compare_keep(&mut gpu, f, "text-sizes", TEXT, true, sizes);
    compare_keep(&mut gpu, f, "text-faces", FACES, true, faces);
    compare_keep(&mut gpu, f, "text-painted", TEXT, true, painted);
    compare_keep(&mut gpu, f, "text-gradient", GRADIENT, true, gradient);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    if let Some(error) = pollster::block_on(scope.pop()) {
        panic!("wgpu validation: {error}");
    }
}

/// Measured on RADV: max 2 at every scale, mean at most 0.025.
const TEXT: Limit = Limit { max: 2, mean: 0.03 };
/// The same, for the case with a variable face, unhinted runs and combining
/// marks. DX12 on the WARP software adapter measured max 3 at 1x and 4 at 2x
/// (RADV: 2), means 0.016 / 0.0078, well inside the limit: one or two 8-bit
/// steps where a glyph's thin stems are covered at a few percent, WARP
/// rounding the blend differently. Not shader behaviour: the mean and the
/// other text cases are unchanged on WARP.
const FACES: Limit = Limit { max: 4, mean: 0.03 };
/// `vello_cpu` keeps only solid runs in its atlas: it fills a gradient run's
/// outlines at their exact position, while the pass takes them from the
/// same quarter-pixel bins as solid text. Edge pixels differ by up to an
/// eighth of a pixel's coverage, a thin stem two such edges (measured: max
/// 39, mean 0.068); the ramp
/// itself agrees. Neither is visibly better: exact placement is a hair
/// truer, binned placement keeps a gradient label's glyphs identical to the
/// same label in a solid colour.
const GRADIENT: Limit = Limit {
    max: 40,
    mean: 0.07,
};

/// A 2x2 image, red green / blue white, straight alpha.
fn quadrants() -> Arc<Image> {
    #[rustfmt::skip]
    let px: Vec<u8> = vec![
        255, 0, 0, 255,  0, 255, 0, 255,
        0, 0, 255, 255,  255, 255, 255, 255,
    ];
    Arc::new(Image::rgba(2, 2, px).unwrap())
}

/// A 24x16 photo-like ramp with a translucent corner.
fn photo() -> Arc<Image> {
    let (w, h) = (24u32, 16u32);
    let px: Vec<u8> = (0..h)
        .flat_map(|y| {
            (0..w).flat_map(move |x| {
                let a = if x < 6 && y < 4 { 120 } else { 255 };
                [(x * 10) as u8, (y * 15) as u8, ((x + y) * 6) as u8, a]
            })
        })
        .collect();
    Arc::new(Image::rgba(w, h, px).unwrap())
}

fn images(scale: f64) -> ResolvedScene {
    let img = photo();
    let mut kids = Vec::new();
    for (i, fit) in [Fit::Cover, Fit::Contain, Fit::Fill]
        .into_iter()
        .enumerate()
    {
        for (j, radius) in [0., 12.].into_iter().enumerate() {
            kids.push(
                block(80., 50. + 20. * j as f64)
                    .radius(radius)
                    .fill(Fill::Image(img.clone(), fit))
                    .at(10. + 100. * i as f64, 10. + 80. * j as f64),
            );
        }
    }
    scene_at(ground(kids), scale)
}

/// Inside a box the two agree to the rounding of bilinear weights (the GPU
/// filter's against `vello_cpu`'s 8-bit ones); the max is the rounded
/// corners, as in `quad_pixels` (measured: max 48, mean 0.13).
const IMAGES: Limit = Limit {
    max: 50,
    mean: 0.15,
};

#[test]
fn images_match_vello_cpu_and_land_in_their_quadrants() {
    let Some(mut gpu) = device_gpu(standard) else {
        return;
    };
    let mut failures = Vec::new();
    compare_keep(&mut gpu, &mut failures, "images", IMAGES, true, images);
    assert!(failures.is_empty(), "{}", failures.join("\n"));

    let img = quadrants();
    let at = |px: &[u8], w: u32, x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    };
    let (red, green, blue, white) = (
        [255, 0, 0, 255],
        [0, 255, 0, 255],
        [0, 0, 255, 255],
        [255, 255, 255, 255],
    );
    for scale in SCALES {
        let s = |v: f64| (v * scale) as u32;
        let scene = scene_at(
            stack![
                block(100., 100.)
                    .radius(0.)
                    .fill(Fill::Image(img.clone(), Fit::Fill))
                    .at(0., 0.),
                block(100., 60.)
                    .radius(0.)
                    .fill(Fill::Image(img.clone(), Fit::Cover))
                    .at(110., 0.),
                block(100., 60.)
                    .radius(0.)
                    .fill(Fill::Image(img.clone(), Fit::Contain))
                    .at(220., 0.),
                block(60., 60.)
                    .radius(30.)
                    .fill(Fill::Image(img.clone(), Fit::Fill))
                    .at(0., 120.),
            ]
            .size(W, H),
            scale,
        );
        let size = sized(scale);
        let px = gpu_pixels(&mut gpu, &scene.paint, Affine::scale(scale), size);
        let w = size[0];
        // Sampled where bilinear filtering reads one texel only.
        // Fill: each source pixel owns a quadrant.
        for ((x, y), want) in [
            ((12., 12.), red),
            ((88., 12.), green),
            ((12., 88.), blue),
            ((88., 88.), white),
        ] {
            assert_eq!(at(&px, w, s(x), s(y)), want, "fill at {x},{y} {scale}x");
        }
        // Cover on a wide box: scaled to the width, top and bottom cropped.
        for ((x, y), want) in [
            ((122., 2.), red),
            ((198., 2.), green),
            ((122., 58.), blue),
            ((198., 58.), white),
        ] {
            assert_eq!(at(&px, w, s(x), s(y)), want, "cover at {x},{y} {scale}x");
        }
        // Contain: scaled to the height, centred, the sides left clear.
        for ((x, y), want) in [
            ((242., 2.), red),
            ((298., 2.), green),
            ((242., 58.), blue),
            ((298., 58.), white),
        ] {
            assert_eq!(at(&px, w, s(x), s(y)), want, "contain at {x},{y} {scale}x");
        }
        for x in [221., 238., 302., 319.] {
            assert_eq!(
                at(&px, w, s(x), s(30.))[3],
                0,
                "contain spilled at {x} {scale}x"
            );
        }
        // The rounded corner still cuts the image away.
        assert_eq!(
            at(&px, w, s(1.), s(121.))[3],
            0,
            "image past the corner {scale}x"
        );
    }
}

/// White text on a transparent target: the alpha channel is coverage.
fn coverage_of(gpu: &mut Gpu, scene: &ResolvedScene, scale: f64) -> (Vec<u8>, u32) {
    let size = sized(scale);
    let px = gpu_pixels(gpu, &scene.paint, Affine::scale(scale), size);
    (px.chunks(4).map(|p| p[3]).collect(), size[0])
}

/// The alpha-weighted mean column of `a`.
fn centroid(a: &[u8], w: u32) -> f64 {
    let (mut sum, mut total) = (0., 0.);
    for (i, &v) in a.iter().enumerate() {
        sum += f64::from(v) * (i as u32 % w) as f64;
        total += f64::from(v);
    }
    sum / total
}

fn letter(scale: f64, x: f64, size: f64) -> ResolvedScene {
    let root = stack![
        text("H")
            .text_size(size)
            .fill(Color::srgb(1., 1., 1.))
            .id("h")
            .at(x, 40.)
    ]
    .size(W, H);
    resolve_with(root, scale, hack())
}

fn run(scene: &ResolvedScene) -> (&mui_scene::Painted, &mui_scene::Text) {
    scene
        .paint
        .iter()
        .find_map(|p| Some((p, p.text.as_ref()?)))
        .expect("a run")
}

/// The run's baseline, in scene units.
fn origin(scene: &ResolvedScene) -> f64 {
    let (p, t) = run(scene);
    (p.transform * mui_vello::kurbo::Point::new(t.origin.x + p.offset.x, t.origin.y + p.offset.y)).y
}

/// The run's pen, in scene units.
fn pen(scene: &ResolvedScene) -> f64 {
    let (p, t) = run(scene);
    (p.transform * mui_vello::kurbo::Point::new(t.origin.x + p.offset.x, t.origin.y + p.offset.y)).x
}

#[test]
fn glyphs_land_on_the_pen_and_baseline_and_are_sharp() {
    let Some(mut gpu) = device_gpu(standard) else {
        return;
    };
    for scale in SCALES {
        let at = letter(scale, 20., 16.);
        let (a, w) = coverage_of(&mut gpu, &at, scale);
        // The pen: moving the run by 7.25 moves its ink by as much, to the
        // quarter-pixel bin glyphs are placed at.
        let there = letter(scale, 27.25, 16.);
        let (b, _) = coverage_of(&mut gpu, &there, scale);
        let moved = centroid(&b, w) - centroid(&a, w);
        let pen = (pen(&there) - pen(&at)) * scale;
        assert!(pen > 0., "the scene did not move the run");
        assert!(
            (moved - pen).abs() <= 0.13,
            "moved {moved}, pen {pen} at {scale}x"
        );
        // The baseline: a hinted H stands on it, ink above, none below.
        let baseline = (origin(&at) * scale).round() as usize;
        let row = |r: usize| {
            a[r * w as usize..(r + 1) * w as usize]
                .iter()
                .copied()
                .max()
                .unwrap_or(0)
        };
        assert!(
            row(baseline - 1) >= 250,
            "no ink on the row above the baseline at {scale}x"
        );
        assert!(
            row(baseline) < 40,
            "ink below the baseline at {scale}x: {}",
            row(baseline)
        );
        // Sharp: hinting puts the crossbar on whole rows, so some row holds
        // a run of solid pixels, and most ink is solid rather than grey.
        let solid_run = (0..a.len() / w as usize).any(|r| {
            a[r * w as usize..(r + 1) * w as usize]
                .windows(3)
                .any(|v| v.iter().all(|&c| c >= 250))
        });
        assert!(solid_run, "no solid run in the H at {scale}x");
        let inked = a.iter().filter(|&&c| c >= 20).count();
        let grey = a.iter().filter(|&&c| (20..235).contains(&c)).count();
        assert!(
            grey * 2 < inked,
            "{grey} of {inked} ink pixels grey at {scale}x"
        );
    }
}

/// Every printable ASCII glyph at three sizes, so the atlas is packed.
fn crowd(scale: f64) -> ResolvedScene {
    let line: String = (33u8..127).map(char::from).collect();
    let kids: Vec<El> = [9., 14., 22.]
        .into_iter()
        .enumerate()
        .map(|(i, size)| {
            text(line.clone())
                .text_size(size)
                .fill(Color::srgb(1., 1., 1.))
                .at(0., 10. + 40. * i as f64)
        })
        .collect();
    resolve_with(stack(kids).size(W, H), scale, hack())
}

#[test]
fn a_glyph_reads_only_its_own_cell() {
    let Some(mut gpu) = device_gpu(standard) else {
        return;
    };
    let mut fresh = standard(&gpu.device);
    for scale in SCALES {
        let m = letter(scale, 50.3, 22.);
        gpu_pixels(
            &mut gpu,
            &crowd(scale).paint,
            Affine::scale(scale),
            sized(scale),
        );
        let packed = gpu_pixels(&mut gpu, &m.paint, Affine::scale(scale), sized(scale));
        std::mem::swap(&mut gpu.quad, &mut fresh);
        let alone = gpu_pixels(&mut gpu, &m.paint, Affine::scale(scale), sized(scale));
        std::mem::swap(&mut gpu.quad, &mut fresh);
        assert!(packed == alone, "a neighbouring cell bled in at {scale}x");
    }
}

/// One label at many sizes, one size a frame.
fn label(size: f64) -> ResolvedScene {
    let root = stack![
        text("The quick brown fox, 0.25 dB")
            .text_size(size)
            .fill(Color::srgb(1., 1., 1.))
            .at(4., 10.)
    ]
    .size(W, H);
    resolve_with(root, 1., hack())
}

#[test]
fn a_small_atlas_evicts_and_still_draws_every_frame_right() {
    let Some(mut gpu) = device_gpu(standard) else {
        return;
    };
    // Two 128-pixel pages: a few sizes of the label fill it.
    let budget = 2 * 128 * 128;
    let mut small =
        QuadRenderer::with_atlas_budget(&gpu.device, wgpu::TextureFormat::Rgba8Unorm, budget);
    let size = sized(1.);
    let mut again = false;
    let sizes = (9..=40)
        .map(|p| (false, p))
        .chain((9..=12).map(|p| (true, p)));
    for (repeat, px) in sizes {
        let scene = label(f64::from(px));
        let want = gpu_pixels(&mut gpu, &scene.paint, Affine::IDENTITY, size);
        std::mem::swap(&mut gpu.quad, &mut small);
        let got = gpu_pixels(&mut gpu, &scene.paint, Affine::IDENTITY, size);
        let frame = gpu.quad.frame();
        std::mem::swap(&mut gpu.quad, &mut small);
        assert!(got == want, "the small atlas drew {px}px wrong");
        assert!(frame.atlas_bytes <= budget as u64, "{frame:?}");
        // A size drawn before had to be rasterised again: it was evicted.
        again |= repeat && frame.rasterised > 0;
    }
    assert!(again, "the small atlas never let go of a size");
    // A frame that cannot fit even in an empty atlas is refused, by entry.
    let huge = label(120.);
    std::mem::swap(&mut gpu.quad, &mut small);
    let refused = try_gpu_pixels(&mut gpu, &huge.paint, Affine::IDENTITY, size);
    std::mem::swap(&mut gpu.quad, &mut small);
    match refused {
        Err(Error::Unsupported {
            index,
            reason: Reason::GlyphBudget,
        }) => assert!(huge.paint[index].text.is_some(), "refused a non-text entry"),
        other => panic!("not refused for its budget: {:?}", other.map(|_| ())),
    }
}

#[test]
fn a_steady_frame_rasterises_and_uploads_nothing() {
    let Some(mut gpu) = device_gpu(standard) else {
        return;
    };
    let scale = 1.5;
    let scene = faces(scale);
    let xf = Affine::scale(scale);
    gpu_pixels(&mut gpu, &scene.paint, xf, sized(scale));
    assert!(gpu.quad.frame().rasterised > 0);
    gpu_pixels(&mut gpu, &scene.paint, xf, sized(scale));
    let steady = gpu.quad.frame();
    assert_eq!((steady.rasterised, steady.uploaded), (0, 0), "{steady:?}");
    // Scrolled by a fraction: new bins once, then steady again.
    let scrolled = Affine::translate((0.37, 0.61)) * xf;
    gpu_pixels(&mut gpu, &scene.paint, scrolled, sized(scale));
    gpu_pixels(&mut gpu, &scene.paint, scrolled, sized(scale));
    let steady = gpu.quad.frame();
    assert_eq!((steady.rasterised, steady.uploaded), (0, 0), "{steady:?}");
    assert_eq!(steady.draws, 1);
}
