use super::*;
use mui_scene::prelude::*;

/// Every pipeline the pass builds must build for a GLES 3 / WebGL2 device:
/// valid WGSL with no optional capability, no compute stage, no storage
/// buffer, and a GLSL ES 3.00 translation of every entry point. The quad
/// shader, the backdrop blur, the weld pass whose textures it samples, and
/// the present that puts `GpuRenderer`'s frame on the window.
#[test]
fn naga_validates_the_shaders_for_a_downlevel_device() {
    for (name, source) in [
        ("quad", SHADER),
        ("blur", BLUR_SHADER),
        ("weld", crate::effects::WELD_SHADER),
        ("present", crate::effects::PRESENT_SHADER),
    ] {
        let module = naga::front::wgsl::parse_str(source).expect("WGSL parse");
        let info = naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{name}: WGSL validates with no optional capability: {e:?}"));
        for (_, global) in module.global_variables.iter() {
            assert!(
                matches!(
                    global.space,
                    naga::AddressSpace::Uniform | naga::AddressSpace::Handle
                ),
                "{name}: {:?} is neither a uniform nor a texture",
                global.name
            );
        }
        assert!(
            module
                .entry_points
                .iter()
                .all(|e| e.stage != naga::ShaderStage::Compute),
            "{name}: a compute stage"
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
            .unwrap_or_else(|e| panic!("{name}: GLSL ES 3.00 {}: {e}", entry.name));
        }
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
    let mut paint = paint;
    // The star draws; the same entry with a path the geometry kernel
    // refuses (a line with no start) does not.
    let star = paint
        .iter()
        .position(|p| p.rect.is_none() && p.layer == mui_scene::Layer::Fill)
        .expect("the star");
    assert_eq!(classify(&paint[star..=star]).supported, 1);
    paint.push(paint[star].clone());
    paint.last_mut().expect("pushed").path = std::sync::Arc::new(mui_geometry::Path {
        commands: vec![mui_geometry::PathCommand::LineTo(Point::new(1., 1.))],
    });
    let coverage = classify(&paint);
    assert_eq!(coverage.unsupported(Reason::Path), 1, "{coverage:?}");
    assert_eq!(coverage.unsupported(Reason::ClipDepth), 1, "{coverage:?}");
    let refused: usize = coverage.unsupported.iter().sum();
    assert_eq!(coverage.supported + refused, paint.len());
}

#[test]
fn the_lookup_table_is_sized_as_vello_common_sizes_it() {
    let c = Color::srgb(0., 0., 0.);
    let size = |offsets: &[f32]| {
        super::ramp::lut_size(&offsets.iter().map(|&t| (t, c)).collect::<Vec<_>>())
    };
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

#[test]
fn a_steady_frame_reuses_its_buffers() {
    let paint = resolved(
        stack![
            block(40., 30.)
                .radius(6.)
                .fill(Color::srgb(1., 0., 0.))
                .shadow(Shadow::soft(4.))
        ]
        .size(100., 80.)
        .radius(8.)
        .clip(),
    );
    let mut cpu = Instances::new(1024, 4);
    let build = |cpu: &mut Instances| {
        cpu.begin(paint.len());
        let count = cpu.build(&paint, Affine::IDENTITY, &|_, _| true, None);
        cpu.encode();
        count
    };
    let first = build(&mut cpu).expect("supported");
    let (at, capacity) = (cpu.bytes.as_ptr(), cpu.bytes.capacity());
    let list = (cpu.list.as_ptr(), cpu.list.capacity());
    let clips = cpu.walk.clip_capacity();
    for _ in 0..3 {
        assert_eq!(build(&mut cpu), Ok(first));
        assert_eq!((cpu.bytes.as_ptr(), cpu.bytes.capacity()), (at, capacity));
        assert_eq!((cpu.list.as_ptr(), cpu.list.capacity()), list);
        assert_eq!(cpu.walk.clip_capacity(), clips);
    }
    assert_eq!(cpu.bytes.len(), first * SLOTS * 16);
}

/// Every instance's quad lies inside its device box, which damage relies
/// on: the box of each corner the vertex shader can place.
#[test]
fn an_instance_box_holds_its_quad() {
    let mut one = build::clear([10, 10]);
    // Rotated, scaled, stroked.
    let (s, c) = 0.7_f32.sin_cos();
    one[0] = [2. * c, 2. * s, -1.5 * s, 1.5 * c];
    one[1] = [40., 30., 0., 0.];
    one[2] = [-5., -3., 12., 9.];
    one[3] = [build::code(1., 0.), 0., 2., 4.];
    let b = build::device_box(&one);
    let [a, bb, cc, d] = one[0];
    let det = a * d - bb * cc;
    let e = 0.5 * (a * a + bb * bb + cc * cc + d * d);
    let grow = (e + (e * e - det * det).max(0.).sqrt()).sqrt() / det.abs() + 2.;
    for (x, y) in [
        (-5. - grow, -3. - grow),
        (12. + grow, -3. - grow),
        (-5. - grow, 9. + grow),
        (12. + grow, 9. + grow),
    ] {
        let (px, py) = (a * x + cc * y + 40., bb * x + d * y + 30.);
        assert!(
            b[0] < px && px < b[2] && b[1] < py && py < b[3],
            "{px},{py} outside {b:?}"
        );
    }
}

/// Text the atlas cannot hold at the right size or angle is refused, by
/// reason; upright text at any uniform scale is not.
#[test]
fn text_under_rotation_or_past_the_atlas_size_is_refused() {
    let mut spec = SceneSpec::new(
        stack![
            text("upright").text_size(14.),
            text("turned").text_size(14.).rotation(0.3).at(0., 40.),
            text("huge").text_size(300.).at(0., 80.),
        ]
        .size(320., 200.),
    )
    .offered(Size::new(320., 200.));
    spec.font = Some(mui_scene::Font::new(epaint_default_fonts::HACK_REGULAR).unwrap());
    let paint = mui_scene::resolve(&spec).expect("resolves").paint;
    let coverage = classify(&paint);
    assert_eq!(
        coverage.unsupported(Reason::TextTransform),
        1,
        "{coverage:?}"
    );
    assert_eq!(coverage.unsupported(Reason::TextSize), 1, "{coverage:?}");
    assert_eq!(coverage.supported + 2, paint.len(), "{coverage:?}");
}
