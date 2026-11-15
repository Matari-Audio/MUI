#![cfg(feature = "cpu")]

use mui_scene::{Image, Vector, VectorCommand, prelude::*};
use mui_vello::{
    Cache, Cpu,
    kurbo::{Affine, Rect, Shape},
    peniko,
    vello_cpu::{Pixmap, RenderContext, Resources},
};
use std::sync::Arc;

fn pixels(mask: Fill) -> Vec<u8> {
    let scene = resolve(&SceneSpec::new(
        stack![
            block(80., 40.)
                .radius(0.)
                .fill(Color::srgb(1., 0., 0.))
                .alpha_mask(mask)
                .at(20., 20.)
        ]
        .size(120., 80.)
        .radius(0.)
        .fill(Color::srgb(0., 0., 1.)),
    ))
    .unwrap();
    let mut ctx = RenderContext::new(120, 80);
    let mut resources = Resources::default();
    mui_vello::paint(
        &mut Cpu {
            ctx: &mut ctx,
            resources: &mut resources,
            cache: &mut Cache::default(),
        },
        &scene,
        Affine::IDENTITY,
    )
    .unwrap();
    ctx.flush();
    let mut pixmap = Pixmap::new(120, 80);
    ctx.render(&mut pixmap, &mut resources);
    pixmap.take_rgba8(peniko::ImageAlphaType::Alpha)
}

fn assert_pixel(pixels: &[u8], x: usize, y: usize, expected: [u8; 4]) {
    let i = (y * 120 + x) * 4;
    assert!(
        pixels[i..i + 4]
            .iter()
            .zip(expected)
            .all(|(&a, b)| a.abs_diff(b) <= 1),
        "at {x},{y}: {:?} vs {expected:?}",
        &pixels[i..i + 4]
    );
}

#[test]
fn vector_alpha_mask_preserves_colour_and_clears_unpainted_regions() {
    for alpha in [0, 128, 255] {
        // Two overlapping shapes must compose together before masking the subtree.
        let commands = [Rect::new(0., 0., 3., 4.), Rect::new(2., 0., 4., 4.)].map(|rect| {
            VectorCommand::Fill {
                path: rect.to_path(0.01),
                transform: Affine::IDENTITY,
                brush: peniko::Brush::Solid(peniko::Color::from_rgba8(0, 255, 0, alpha)),
                brush_transform: Affine::IDENTITY,
                rule: peniko::Fill::NonZero,
            }
        });
        let vector = Arc::new(Vector::new(8., 4., commands.to_vec()).unwrap());
        let px = pixels(Fill::Vector(vector, Fit::Fill));
        assert_pixel(&px, 30, 40, [alpha, 0, 255 - alpha, 255]);
        let overlap = (255. * (1. - (1. - f64::from(alpha) / 255.).powi(2))).round() as u8;
        assert_pixel(&px, 45, 40, [overlap, 0, 255 - overlap, 255]);
        assert_pixel(&px, 85, 40, [0, 0, 255, 255]);
    }
}

#[test]
fn image_alpha_mask_samples_alpha_and_applies_fit_at_nonzero_origin() {
    for alpha in [0, 128, 255] {
        let image = Arc::new(Image::rgba(2, 2, [0, 255, 0, alpha].repeat(4)).unwrap());
        for fit in [Fit::Fill, Fit::Contain, Fit::Cover] {
            let px = pixels(Fill::Image(image.clone(), fit));
            assert_pixel(&px, 60, 40, [alpha, 0, 255 - alpha, 255]);
            assert_pixel(
                &px,
                25,
                40,
                if fit == Fit::Contain {
                    [0, 0, 255, 255]
                } else {
                    [alpha, 0, 255 - alpha, 255]
                },
            );
            assert_pixel(&px, 10, 40, [0, 0, 255, 255]);
        }
    }
}
