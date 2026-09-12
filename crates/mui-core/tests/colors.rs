use mui_core::*;
#[test]
fn wcag_reference_ratios_and_quantized_thresholds() {
    assert_eq!(Rgb::BLACK.contrast(Rgb::WHITE), 21.);
    assert!((Rgb(255, 0, 0).luminance() - 0.2126).abs() < 1e-10);
    assert!(Rgb(119, 119, 119).contrast(Rgb::WHITE) < 4.5);
    assert!(Rgb(118, 118, 118).contrast(Rgb::WHITE) >= 4.5);
    for bg in [Rgb::WHITE, Rgb::BLACK, Rgb(128, 128, 128), Rgb(20, 40, 90)] {
        for original in [Rgb(119, 119, 119), Rgb(250, 40, 60), Rgb(20, 230, 180)] {
            let next = original.contrast_on(&[bg], 4.5).unwrap();
            assert!(next.contrast(bg) >= 4.5);
            assert_eq!(next.contrast_on(&[bg], 4.5).unwrap(), next);
        }
    }
    assert!(Rgb::BLACK
        .contrast_on(&[Rgb::BLACK, Rgb::WHITE], 7.)
        .is_err());
    assert!(Rgb::BLACK.contrast_on(&[], 4.5).is_err());
    assert!(Rgb::BLACK.contrast_on(&[Rgb::WHITE], f64::NAN).is_err());
}
#[test]
fn every_theme_role_has_readable_content_in_both_modes() {
    for seed in [
        Rgb::WHITE,
        Rgb::BLACK,
        Rgb(255, 0, 0),
        Rgb(0, 255, 0),
        Rgb(0, 0, 255),
    ] {
        let palette = Palette {
            light: Seeds {
                primary: [seed; 3],
                neutral: seed,
                status: [seed; 4],
            },
            dark: None,
        };
        for mode in [Mode::Light, Mode::Dark] {
            let c = palette.resolve(mode).unwrap();
            for bg in [c.canvas, c.panel, c.raised] {
                assert!(c.text.contrast(bg) >= 4.5);
                assert!(c.muted.contrast(bg) >= 4.5);
                assert!(c.outline.contrast(bg) >= 3.);
            }
            for a in c.primary.into_iter().chain(c.status) {
                assert!(a.on_fill.contrast(a.fill) >= 4.5);
                assert!(a.on_soft.contrast(a.soft) >= 4.5);
            }
        }
    }
    let palette = Palette::default();
    let light = palette.resolve(Mode::Light).unwrap();
    let dark = palette.resolve(Mode::Dark).unwrap();
    assert!(light.canvas.luminance() > dark.canvas.luminance());
    assert!(dark.raised.luminance() > dark.panel.luminance());
    let custom = Palette {
        dark: Some(Seeds {
            primary: [Rgb(255, 0, 0); 3],
            ..Default::default()
        }),
        ..palette
    };
    assert_eq!(custom.resolve(Mode::Light).unwrap(), light);
    assert_ne!(custom.resolve(Mode::Dark).unwrap().primary, dark.primary);
}
