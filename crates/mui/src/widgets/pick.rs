//! A colour picker: a saturation-value square, a hue strip, an optional
//! alpha strip and a hex field, all over one [`Color`].
use mui_scene::prelude::*;

use crate::widgets::{stepped, text_input};
use crate::Ui;

/// Straight sRGB and alpha, each `0..1`.
type Rgba = [f32; 4];

/// A picker: drag in the square for saturation and value, along the strips
/// for hue and (with `alpha`) opacity, or type a hex colour -- `#rrggbb`,
/// or `#rrggbbaa` with alpha. The strips step with the arrow keys when
/// focused. Returns the picker and whether the colour changed.
///
/// The hue is remembered while the colour is a grey or black, so dragging
/// saturation back up returns to the hue it left.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::new(Theme::DEFAULT);
/// let mut tint = Color::srgb(0.2, 0.5, 0.9);
/// let (picker, changed) = color_picker(&mut ui, "tint", &mut tint, true);
/// assert!(!changed, "nothing dragged or typed");
/// ```
pub fn color_picker(ui: &mut Ui, id: &str, value: &mut Color, alpha: bool) -> (El, bool) {
    let [sv, hue, opacity, hex] = ["sv", "hue", "alpha", "hex"].map(|k| format!("{id}/{k}"));
    let rgba = srgba(*value);
    // The hue and saturation a grey no longer carries: last frame's, if the
    // colour is still the one that frame made.
    let mut hsv = match ui.stash::<([f32; 3], Rgba)>(&sv) {
        Some((hsv, was)) if near(*was, rgba) => *hsv,
        _ => to_hsv(rgba),
    };
    let mut a = rgba[3];
    let before = (hsv, a);
    if let Some((x, y)) = grab(ui, &sv) {
        (hsv[1], hsv[2]) = (x, 1.0 - y);
    }
    if let Some((x, _)) = grab(ui, &hue) {
        hsv[0] = x * 360.0;
    }
    if let Some((x, _)) = grab(ui, &opacity) {
        a = x;
    }
    let mut h = f64::from(hsv[0]);
    if stepped(ui, &hue, &mut h, &(0.0..=360.0)) {
        hsv[0] = h as f32;
    }
    let mut al = f64::from(a);
    if stepped(ui, &opacity, &mut al, &(0.0..=1.0)) {
        a = al as f32;
    }
    let mut changed = (hsv, a) != before;
    if changed {
        let [r, g, b] = to_rgb(hsv);
        *value = Color::srgba(r, g, b, a);
    }
    // Typing: the field holds its own text while focused, so a half-typed
    // `#3a` is not overwritten by the colour it does not yet make.
    let mut text = match ui.stash::<String>(&hex) {
        Some(t) if ui.focused(&hex) => t.clone(),
        _ => to_hex(srgba(*value), alpha),
    };
    let (field, typed) = text_input(ui, &hex, &mut text);
    if typed {
        if let Some(([r, g, b], ta)) = parse_hex(&text) {
            let ta = if alpha { ta.unwrap_or(a) } else { a };
            *value = Color::srgba(r, g, b, ta);
            hsv = to_hsv([r, g, b, ta]);
            changed = true;
        }
    }
    let focused = ui.focused(&hex);
    ui.set_stash(&hex, focused.then_some(text));
    ui.set_stash(&sv, Some((hsv, srgba(*value))));

    let w = ui.theme.control * 48.0;
    let (sh, bar) = (w * 0.7, ui.theme.control * 4.0);
    let [r, g, b] = to_rgb([hsv[0], 1.0, 1.0]);
    let pure = Color::srgb(r, g, b);
    let white = Color::srgb(1.0, 1.0, 1.0);
    let black = Color::srgb(0.0, 0.0, 0.0);
    let ring = |d: f64| {
        leaf(d, d)
            .pill()
            .border(white, 2.0)
            .shell(1.0, black.with_alpha(0.5))
    };
    let square = overlay([
        leaf(w, sh)
            .radius(4.0)
            .fill(Gradient::linear(90.0, [(0.0, white), (1.0, pure)])),
        leaf(w, sh).radius(4.0).fill(Gradient::linear(
            180.0,
            [(0.0, black.with_alpha(0.0)), (1.0, black)],
        )),
        ring(12.0).anchor(Align::Start, Align::Start).offset(
            f64::from(hsv[1]) * w - 6.0,
            (1.0 - f64::from(hsv[2])) * sh - 6.0,
        ),
    ])
    .size(w, sh)
    .cursor(Cursor::Crosshair)
    .id(sv);
    let strip = |key: String, label: &str, t: f32, v: f64, max: f64, fill: Gradient| {
        overlay([
            leaf(w, bar).pill().fill(fill),
            ring(bar + 4.0)
                .anchor(Align::Start, Align::Center)
                .offset(f64::from(t) * (w - bar - 4.0), 0.0),
        ])
        .size(w, bar + 4.0)
        .role(Kind::Slider {
            value: v,
            min: 0.0,
            max,
        })
        .label(label)
        .focusable()
        .id(key)
    };
    let rainbow = Gradient::linear(
        90.0,
        (0..=6).map(|k| {
            let [r, g, b] = to_rgb([k as f32 * 60.0, 1.0, 1.0]);
            (k as f32 / 6.0, Color::srgb(r, g, b))
        }),
    );
    let mut parts = vec![
        square,
        strip(
            hue,
            "Hue",
            hsv[0] / 360.0,
            f64::from(hsv[0]),
            360.0,
            rainbow,
        ),
    ];
    if alpha {
        let solid = *value;
        let ramp = Gradient::linear(
            90.0,
            [(0.0, solid.with_alpha(0.0)), (1.0, solid.with_alpha(1.0))],
        );
        parts.push(strip(opacity, "Alpha", a, f64::from(a), 1.0, ramp));
    }
    parts.push(
        row([
            leaf(bar * 2.0, bar * 2.0)
                .radius(4.0)
                .fill(*value)
                .border(Role::Ink.alpha(0.3), 1.0),
            field.grow(1.0),
        ])
        .gap(S)
        .align(Align::Center),
    );
    (column(parts).gap(S).width(w).id(id), changed)
}

/// Where the press holding `id` is, as shares of its box, clamped to it.
fn grab(ui: &Ui, id: &str) -> Option<(f32, f32)> {
    let r = ui.get(id);
    if !(r.pressed || r.held) {
        return None;
    }
    let p = ui.local(id)?;
    let s = ui.scene()?.surface(id)?.frame.size;
    let share = |v: f64, n: f64| {
        if n > 0.0 {
            (v / n).clamp(0.0, 1.0) as f32
        } else {
            0.0
        }
    };
    Some((share(p.x, s.width), share(p.y, s.height)))
}

fn srgba(c: Color) -> Rgba {
    c.to_srgb().components
}

fn near(a: Rgba, b: Rgba) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

fn to_hsv([r, g, b, _]: Rgba) -> [f32; 3] {
    let max = r.max(g).max(b);
    let d = max - r.min(g).min(b);
    let h = if d <= 0.0 {
        0.0
    } else if max == r {
        60.0 * ((g - b) / d).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / d + 2.0)
    } else {
        60.0 * ((r - g) / d + 4.0)
    };
    [h, if max > 0.0 { d / max } else { 0.0 }, max]
}

fn to_rgb([h, s, v]: [f32; 3]) -> [f32; 3] {
    let f = |n: f32| {
        let k = (n + h / 60.0).rem_euclid(6.0);
        v - v * s * k.min(4.0 - k).clamp(0.0, 1.0)
    };
    [f(5.0), f(3.0), f(1.0)]
}

fn to_hex([r, g, b, a]: Rgba, alpha: bool) -> String {
    let byte = |x: f32| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut s = format!("#{:02x}{:02x}{:02x}", byte(r), byte(g), byte(b));
    if alpha {
        s += &format!("{:02x}", byte(a));
    }
    s
}

/// `#rrggbb` or `#rrggbbaa`, the `#` optional; the alpha when it was given.
fn parse_hex(s: &str) -> Option<([f32; 3], Option<f32>)> {
    let s = s.trim().trim_start_matches('#');
    if !s.is_ascii() || !(s.len() == 6 || s.len() == 8) {
        return None;
    }
    let byte = |i: usize| {
        u8::from_str_radix(&s[i..i + 2], 16)
            .ok()
            .map(|v| f32::from(v) / 255.0)
    };
    let a = if s.len() == 8 { Some(byte(6)?) } else { None };
    Some(([byte(0)?, byte(2)?, byte(4)?], a))
}
