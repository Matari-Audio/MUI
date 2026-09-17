//! Live seed editor uses the framework's gamut mapping and contrast-resolved palette.
use gpui::{Hsla, rgb};
use mui::core::{Colors, Mode, Palette, Rgb, Seeds};
use std::cell::RefCell;

fn derive(seed: Rgb) -> Result<Colors, String> {
    let mut colors = Palette {
        light: Seeds {
            neutral: seed,
            primary: [seed; 3],
            ..Seeds::default()
        },
        dark: None,
    }
    .resolve(Mode::Dark)
    .map_err(|e| e.to_string())?;
    let blend = |base: Rgb| {
        Rgb(
            ((base.0 as u16 * 4 + seed.0 as u16) / 5) as u8,
            ((base.1 as u16 * 4 + seed.1 as u16) / 5) as u8,
            ((base.2 as u16 * 4 + seed.2 as u16) / 5) as u8,
        )
    };
    colors.canvas = blend(colors.canvas);
    colors.panel = blend(colors.panel);
    colors.raised = blend(colors.raised);
    let backgrounds = [colors.canvas, colors.panel, colors.raised];
    colors.text = colors
        .text
        .contrast_on(&backgrounds, 4.5)
        .map_err(|e| e.to_string())?;
    colors.muted = colors
        .muted
        .contrast_on(&backgrounds, 4.5)
        .map_err(|e| e.to_string())?;
    colors.outline = colors
        .outline
        .contrast_on(&backgrounds, 3.)
        .map_err(|e| e.to_string())?;
    colors.primary[0].fill = seed
        .contrast_on(&backgrounds, 4.5)
        .map_err(|e| e.to_string())?;
    Ok(colors)
}
thread_local! {
    // ponytail: one theme per UI thread in this mock; pass a theme per embedded editor when hosting multiple editors.
    static LAST_PRIMARY: std::cell::Cell<Option<Rgb>> = const { std::cell::Cell::new(None) };
    static TOKENS: RefCell<Colors> = RefCell::new(derive(Rgb(128,128,128)).expect("neutral palette"));
}
pub fn parse(value: &str) -> Result<Rgb, String> {
    let value = value.trim();
    let value = if let Some(value) = value.strip_prefix("oklch(") {
        value
            .strip_suffix(')')
            .ok_or("Close the OKLCH parenthesis")?
    } else {
        value
    };
    let parts: Vec<_> = value.split_whitespace().collect();
    if parts.len() != 3 {
        return Err("Use oklch(65% 0.12 240) or 0.65 0.12 240".into());
    }
    let l = parts[0]
        .trim_end_matches('%')
        .parse::<f64>()
        .map_err(|_| "Invalid lightness")?
        / if parts[0].ends_with('%') { 100. } else { 1. };
    let c = parts[1]
        .trim_end_matches('%')
        .parse::<f64>()
        .map_err(|_| "Invalid chroma")?
        * if parts[1].ends_with('%') { 0.004 } else { 1. };
    let h = parts[2]
        .trim_end_matches("deg")
        .parse::<f64>()
        .map_err(|_| "Invalid hue")?;
    Rgb::from_oklch(l, c, h).map_err(|e| e.to_string())
}
pub fn set_secondary(seed: Rgb) { TOKENS.with(|t| t.borrow_mut().primary[1].fill = seed); }
pub fn set_tertiary(seed: Rgb) { TOKENS.with(|t| t.borrow_mut().primary[2].fill = seed); }
pub fn outline() -> Rgb { TOKENS.with(|t| t.borrow().outline) }
pub fn primary() -> Rgb { TOKENS.with(|t| t.borrow().primary[0].fill) }
pub fn set_primary(seed: Rgb) -> Result<(), String> {
    if LAST_PRIMARY.with(|last| last.get()==Some(seed)) { return Ok(()); }
    let colors = derive(seed)?;
    LAST_PRIMARY.with(|last| last.set(Some(seed)));
    TOKENS.with(|t| *t.borrow_mut() = colors);
    Ok(())
}
pub fn update(value: &str) -> Result<(), String> {
    set_primary(parse(value)?)
}
pub fn color(role: u32) -> Hsla {
    TOKENS.with(|t| {
        let t = t.borrow();
        let c = match role {
            0x171d1a => t.canvas,
            0x252c28 => t.panel,
            0x343f32 => t.raised,
            0xe0e5da => t.text,
            0x9ba697 => t.muted,
            0x718164 => t.outline,
            0xbadc91 => t.primary[0].fill,
            0x2cc2f6 => t.primary[1].fill,
            0xffad7c => t.primary[2].fill,
            _ => return rgb(role).into(),
        };
        rgb((u32::from(c.0) << 16) | (u32::from(c.1) << 8) | u32::from(c.2)).into()
    })
}
#[test]
fn live_seed_contract() {
    assert_eq!(
        parse("oklch(50% 0 240)").unwrap(),
        parse("0.5 0 -120deg").unwrap()
    );
    for s in [
        "",
        "0.5 0",
        "NaN 0 0",
        "1.1 0 0",
        "0.5 -0.1 0",
        "0.5 inf 0",
        "oklch(50% 0 0",
    ] {
        assert!(parse(s).is_err(), "{s}");
    }
    assert_ne!(
        derive(parse("0.3 0 240").unwrap()).unwrap().panel,
        derive(parse("0.8 0 240").unwrap()).unwrap().panel,
        "lightness must change derived surfaces"
    );
    let colors = derive(parse("0.65 0.3 260").unwrap()).unwrap();
    assert!(colors.text.contrast(colors.panel) >= 4.5);
    assert!(colors.outline.contrast(colors.panel) >= 3.);
}

#[test]
fn tertiary_role_tracks_seed() {
    set_tertiary(Rgb(17, 34, 51));
    let actual = color(0xffad7c);
    let expected: Hsla = rgb(0x112233).into();
    assert!((actual.h - expected.h).abs() < 1e-6);
    assert!((actual.s - expected.s).abs() < 1e-6);
    assert!((actual.l - expected.l).abs() < 1e-6);
}
