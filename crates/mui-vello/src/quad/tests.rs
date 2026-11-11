use super::*;
use mui_scene::prelude::*;

/// The pass must build for a GLES 3 / WebGL2 device: valid WGSL with no
/// optional capability, no compute stage, no storage buffer, and a GLSL ES
/// 3.00 translation of both stages.
#[test]
fn naga_validates_the_shader_for_a_downlevel_device() {
    let module = naga::front::wgsl::parse_str(SHADER).expect("WGSL parse");
    let info = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .expect("WGSL validates with no optional capability");
    for (_, global) in module.global_variables.iter() {
        assert!(
            matches!(
                global.space,
                naga::AddressSpace::Uniform | naga::AddressSpace::Handle
            ),
            "{:?} is neither a uniform nor a texture",
            global.name
        );
    }
    let stages: Vec<_> = module.entry_points.iter().map(|e| e.stage).collect();
    assert_eq!(
        stages,
        [naga::ShaderStage::Vertex, naga::ShaderStage::Fragment]
    );
    let options = naga::back::glsl::Options {
        version: naga::back::glsl::Version::Embedded {
            version: 300,
            is_webgl: true,
        },
        ..Default::default()
    };
    for entry in &module.entry_points {
        let pipeline = naga::back::glsl::PipelineOptions {
            shader_stage: entry.stage,
            entry_point: entry.name.clone(),
            multiview: None,
        };
        let mut out = String::new();
        naga::back::glsl::Writer::new(
            &mut out,
            &module,
            &info,
            &options,
            &pipeline,
            naga::proc::BoundsCheckPolicies::default(),
        )
        .and_then(|mut w| w.write())
        .unwrap_or_else(|e| panic!("GLSL ES 3.00 {}: {e}", entry.name));
    }
}

/// What the WebGL2 limits allow: 16 vertex attributes, 15 inter-stage
/// variables. The instance is sized against both.
#[test]
fn the_instance_fits_webgl2_limits() {
    let limits = wgpu::Limits::downlevel_webgl2_defaults();
    assert!(SLOTS as u32 <= limits.max_vertex_attributes);
    assert!(SLOTS as u32 <= limits.max_inter_stage_shader_variables);
}

fn resolved(root: El) -> Vec<Painted> {
    mui_scene::resolve(&SceneSpec::new(root).offered(Size::new(320., 200.)))
        .expect("resolves")
        .paint
}

#[test]
fn a_card_with_shadows_strokes_and_a_gradient_is_fully_covered() {
    let paint = resolved(
        stack![
            block(120., 60.)
                .radius(12.)
                .fill(Gradient::linear(
                    90.,
                    [(0., Color::srgb(1., 0., 0.)), (1., Color::srgb(0., 0., 1.))]
                ))
                .stroke(Color::srgb(0., 0., 0.))
                .stroke_width(2.)
                .shadow(Shadow::soft(8.))
                .shadow(Shadow::inset(4.))
                .at(20., 20.),
            stack![block(200., 40.).fill(Color::srgb(0., 1., 0.))]
                .size(100., 80.)
                .radius(16.)
                .clip()
                .at(160., 40.),
        ]
        .size(320., 200.)
        .radius(0.)
        .fill(Color::srgb(0.1, 0.1, 0.1)),
    );
    let coverage = classify(&paint);
    assert!(coverage.complete(), "{coverage:?}");
    assert_eq!(coverage.supported, paint.len());
}

#[test]
fn what_the_pass_cannot_draw_is_counted_by_reason() {
    let star = |s: Size| {
        Path::default()
            .move_to(Point::new(s.width / 2., 0.))
            .line_to(Point::new(s.width, s.height))
            .line_to(Point::new(0., s.height))
            .close()
    };
    let paint = resolved(
        stack![
            block(60., 60.).outline(star).fill(Color::srgb(1., 0., 0.)),
            // A rounded clip inside a rounded clip.
            stack![
                stack![block(40., 40.).fill(Color::srgb(0., 1., 0.))]
                    .size(50., 50.)
                    .radius(10.)
                    .clip()
            ]
            .size(80., 80.)
            .radius(12.)
            .clip()
            .at(100., 20.),
            block(60., 30.)
                .fill(Gradient::linear(
                    0.,
                    (0..6).map(|i| (i as f32 / 5., Color::srgb(i as f32 / 5., 0., 0.)))
                ))
                .at(200., 20.),
        ]
        .size(320., 200.),
    );
    let coverage = classify(&paint);
    assert_eq!(coverage.unsupported(Reason::Path), 1, "{coverage:?}");
    assert_eq!(coverage.unsupported(Reason::ClipDepth), 1, "{coverage:?}");
    assert_eq!(coverage.unsupported(Reason::GradientStops), 1, "{coverage:?}");
    let refused: usize = coverage.unsupported.iter().sum();
    assert_eq!(coverage.supported + refused, paint.len());
}

#[test]
fn the_lookup_table_is_sized_as_vello_common_sizes_it() {
    let c = Color::srgb(0., 0., 0.);
    let size = |offsets: &[f32]| build::lut_size(&offsets.iter().map(|&t| (t, c)).collect::<Vec<_>>());
    assert_eq!(size(&[0., 1.]), 256);
    assert_eq!(size(&[0., 0.5, 1.]), 512);
    // Padded out to 0 and 1: three ranges.
    assert_eq!(size(&[0.2, 0.8]), 1024);
    assert_eq!(size(&[0.1, 1.]), 512);
    assert_eq!(size(&[0., 0.001, 1.]), 1024);
    assert_eq!(size(&[0., 0.0001, 1.]), 4096);
    // A hard stop: a zero-width range.
    assert_eq!(size(&[0., 0.5, 0.5, 1.]), 4096);
}


