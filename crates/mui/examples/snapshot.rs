//! Every widget, rendered on the CPU with no window and no GPU, at the three
//! scales a plugin host actually hands a plugin.
//!
//!     cargo run -p mui --features cpu --example snapshot -- /tmp/widgets.png
//!
//! Writes one file per scale (`widgets@1x.png`, `@1.5x`, `@2x`). The scale
//! contract they demonstrate is in `ARCHITECTURE.md`: the tree is laid out in
//! logical units at every scale, and the scale is a paint transform plus a
//! snapping grid -- never a multiplier on a length.

use mui::prelude::*;
use mui::vello::vello_cpu::{Pixmap, RenderContext, Resources};

/// The gallery card: two knobs, a slider and a toggle row.
fn gallery(ui: &mut Ui, state: &mut (f64, f64, f64, bool)) -> El {
    let (cutoff, res, gain, on) = state;
    let go = button(ui, "go", "Trigger").el;
    let card = col([
        row([
            knob(ui, "cutoff", "Cutoff", cutoff, 0.0..=1.0).el.into_el(),
            knob(ui, "res", "Res", res, 0.0..=1.0).el.into_el(),
        ])
        .gap(L)
        .justify(Justify::Center),
        slider(ui, "gain", "Gain", gain, -24.0..=6.0).el.into_el(),
        row([
            text("Bypass").fill(Role::Dim),
            toggle(ui, "bypass", "Bypass", on).el.into_el(),
            spacer(),
            go.el(),
        ])
        .gap(S)
        .align(Align::Center),
    ])
    .gap(M)
    .pad(L)
    .radius(20.0)
    .fill(Role::Surface)
    .shadow(Shadow::soft(16.0))
    .anchor(Align::Center, Align::Center);
    stack([card]).fill(Role::Background)
}

/// The logical size of the gallery, at every scale.
const SIZE: Size = Size {
    width: 360.0,
    height: 300.0,
};

/// One render at one scale: the device pixels, and the geometry the test
/// checks the contract against.
struct Shot {
    /// Device pixels per logical unit.
    scale: f64,
    width: u16,
    height: u16,
    /// Straight (unpremultiplied) RGBA, `width * height * 4`.
    rgba: Vec<u8>,
    /// Per named surface: the unsnapped layout frame and the snapped painted
    /// bounds, both in logical units, as `[x, y, right, bottom]`.
    rects: Vec<(String, [f64; 4], [f64; 4])>,
    /// Every glyph run's font size, in logical units, in paint order.
    text: Vec<f32>,
}

fn shot(scale: f64) -> Shot {
    let (width, height) = (
        (SIZE.width * scale).round() as u16,
        (SIZE.height * scale).round() as u16,
    );
    let mut ui =
        Ui::new(Theme::DEFAULT).font(Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    ui.set_scale(Some(scale));
    let mut state = (0.35, 0.7, -6.0, true);
    // Two frames: the first has no gesture state, the second is what a real
    // host draws every frame.
    let mut out = None;
    for _ in 0..2 {
        let root = gallery(&mut ui, &mut state);
        let frame = ui
            .frame(root, Some(SIZE), PointerInput::default(), 1.0 / 60.0)
            .expect("frame");
        let mut ctx = RenderContext::new(width, height);
        let mut res = Resources::default();
        mui::vello::paint(
            &mut mui::vello::Cpu {
                ctx: &mut ctx,
                resources: &mut res,
                cache: &mut mui::vello::Cache::default(),
            },
            frame.scene,
            // The whole scale story on the paint side: one transform. Glyph
            // outlines are rasterised through it, so 2x is a 2x rendering and
            // not a resampled 1x.
            mui::vello::kurbo::Affine::scale(scale),
        )
        .expect("paint");
        ctx.flush();
        let mut pix = Pixmap::new(width, height);
        ctx.render(&mut pix, &mut res);
        out = Some(Shot {
            scale,
            width,
            height,
            rgba: pix
                .take_unpremultiplied()
                .iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a])
                .collect(),
            rects: frame
                .scene
                .surfaces()
                .filter_map(|s| {
                    let b = s.rect?.bounds();
                    Some((
                        s.key.to_string(),
                        [s.frame.x, s.frame.y, s.frame.right(), s.frame.bottom()],
                        [b.x0, b.y0, b.x1, b.y1],
                    ))
                })
                .collect(),
            text: frame
                .scene
                .paint
                .iter()
                .filter_map(|p| Some(p.text.as_ref()?.size))
                .collect(),
        });
    }
    out.expect("two frames")
}

fn main() {
    let out = std::env::args().nth(1).unwrap_or("widgets.png".into());
    let stem = out.strip_suffix(".png").unwrap_or(&out);
    for scale in [1.0, 1.5, 2.0] {
        let s = shot(scale);
        let path = format!("{stem}@{scale}x.png");
        let file = std::fs::File::create(&path).expect("create");
        let mut enc = png::Encoder::new(
            std::io::BufWriter::new(file),
            s.width.into(),
            s.height.into(),
        );
        enc.set_color(png::ColorType::Rgba);
        enc.write_header()
            .and_then(|mut e| e.write_image_data(&s.rgba))
            .expect("png");
        println!(
            "wrote {path}: {}x{} device px at {}x, {} snapped surfaces, {} glyph runs",
            s.width,
            s.height,
            s.scale,
            s.rects.len(),
            s.text.len()
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A device-space pixel as `[r, g, b, a]`, or black outside the image.
    fn px(s: &Shot, x: u32, y: u32) -> [u8; 4] {
        let i = (y as usize * s.width as usize + x as usize) * 4;
        s.rgba
            .get(i..i + 4)
            .map_or([0; 4], |c| [c[0], c[1], c[2], c[3]])
    }

    #[test]
    fn every_surface_scales_linearly_and_snaps_within_half_a_device_pixel() {
        let one = shot(1.0);
        for s in [shot(1.5), shot(2.0)] {
            assert_eq!(
                s.rects.len(),
                one.rects.len(),
                "the tree changed with the scale"
            );
            for ((key, frame, painted), (k1, f1, _)) in s.rects.iter().zip(&one.rects) {
                assert_eq!(key, k1);
                // Layout is scale-free: the same logical rect at every scale.
                for (a, b) in frame.iter().zip(f1) {
                    assert!(
                        (a - b).abs() < 1e-9,
                        "{key}: layout moved at {}x ({a} vs {b})",
                        s.scale
                    );
                }
                // And the painted edge is that rect on the device grid: at
                // most half a device pixel from where the layout put it, so
                // the 2x snapshot is 2x the 1x one with no drift.
                for (a, b) in painted.iter().zip(frame) {
                    assert!(
                        (a * s.scale - b * s.scale).abs() <= 0.5 + 1e-9,
                        "{key}: {}x edge drifted {} device px",
                        s.scale,
                        (a - b).abs() * s.scale
                    );
                }
            }
        }
    }

    #[test]
    fn a_2x_snapshot_is_rendered_at_2x_and_not_upscaled() {
        let (one, two) = (shot(1.0), shot(2.0));
        assert_eq!((two.width, two.height), (one.width * 2, one.height * 2));
        // The glyph runs are the same logical sizes -- text is a length like
        // any other -- so the 2x run is rasterised at twice the device size.
        assert_eq!(two.text, one.text);
        assert!(!one.text.is_empty(), "no text in the gallery");
        // Nearest-neighbour upscaling the 1x render would reproduce the 2x
        // one exactly. It does not: the 2x glyph and curve edges land on
        // device pixels the 1x grid cannot express.
        let differing = (0..one.height as u32)
            .flat_map(|y| (0..one.width as u32).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                let (a, b) = (px(&one, x, y), px(&two, x * 2, y * 2));
                a.iter().zip(b).any(|(p, q)| p.abs_diff(q) > 8)
            })
            .count();
        assert!(
            differing * 100 > one.width as usize * one.height as usize,
            "the 2x render is within rounding of an upscaled 1x ({differing} pixels differ)"
        );
    }
}
