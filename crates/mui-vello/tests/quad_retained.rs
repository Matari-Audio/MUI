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
use mui_vello::quad::QuadRenderer;
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
