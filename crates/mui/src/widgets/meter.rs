//! Level meters: one bar per channel on a decibel scale, a held peak per
//! channel that the meter keeps itself, and zones that colour the level by
//! how hot it is. A click clears the hold.
use mui_material::prelude::*;

use crate::Ui;

/// The channels a meter shows, as linear amplitudes (1.0 is 0 dBFS): one
/// number for a mono meter, a slice or array for several.
pub struct Levels(Vec<f32>);
impl From<f32> for Levels {
    fn from(v: f32) -> Self {
        Self(vec![v])
    }
}
impl From<f64> for Levels {
    fn from(v: f64) -> Self {
        Self(vec![v as f32])
    }
}
impl From<&[f32]> for Levels {
    fn from(v: &[f32]) -> Self {
        Self(v.to_vec())
    }
}
impl<const N: usize> From<[f32; N]> for Levels {
    fn from(v: [f32; N]) -> Self {
        Self(v.to_vec())
    }
}
impl From<Vec<f32>> for Levels {
    fn from(v: Vec<f32>) -> Self {
        Self(v)
    }
}

/// Where on a meter running from `floor` dB to 0 dBFS a linear `level` sits.
pub fn meter_position(level: f32, floor: f32) -> f64 {
    if level <= 0.0 {
        return 0.0;
    }
    f64::from(((20.0 * level.log10() - floor) / -floor).clamp(0.0, 1.0))
}

/// A meter, read; finish it with [`Meter::el`] or drop it into a row.
pub struct Meter {
    id: Id,
    levels: Vec<f32>,
    hold: Vec<f32>,
    floor: f32,
    segments: usize,
    ticks: bool,
    readout: bool,
    vertical: bool,
    quiet: Fill,
    w: Option<Len>,
    h: Option<Len>,
}

/// A level meter for `levels`, linear amplitudes. It holds each channel's
/// peak itself (falling slowly, cleared by a click) and colours the bar
/// with its quiet colour below -12 dB, ink up to -3 dB and the danger role above.
///
/// ```
/// use mui::prelude::*;
/// let mut ui = Ui::default();
/// let out = meter(&mut ui, "out", [0.5, 0.25]).segments(36).ticks().readout();
/// let mono = meter(&mut ui, "in", 0.1_f32).w(200);
/// ```
pub fn meter(ui: &mut Ui, id: impl Into<Id>, levels: impl Into<Levels>) -> Meter {
    let id: Id = id.into();
    let levels = levels.into().0;
    let mut hold = ui.stash::<Vec<f32>>(&id).cloned().unwrap_or_default();
    if ui.get(&id).pressed || hold.len() != levels.len() {
        hold = vec![0.0; levels.len()];
    }
    for (h, &l) in hold.iter_mut().zip(&levels) {
        *h = l.clamp(0.0, 1.0).max(*h * 0.992);
        if *h < 1e-4 {
            *h = 0.0;
        }
    }
    ui.set_stash(&id, Some(hold.clone()));
    Meter {
        id,
        levels,
        hold,
        floor: -60.0,
        segments: 0,
        ticks: false,
        readout: false,
        vertical: false,
        quiet: Role::Primary.into(),
        w: None,
        h: None,
    }
}

impl Meter {
    /// The quietest level shown, in dB (default -60).
    pub fn floor(mut self, db: f32) -> Self {
        self.floor = db.min(-1.0);
        self
    }
    /// LED-style: `n` lit segments instead of a continuous bar.
    pub fn segments(mut self, n: usize) -> Self {
        self.segments = n;
        self
    }
    /// Tick marks at -48, -36, -24, -12, -6 and 0 dB.
    pub fn ticks(mut self) -> Self {
        self.ticks = true;
        self
    }
    /// The held peak, in dB, beside the bars.
    pub fn readout(mut self) -> Self {
        self.readout = true;
        self
    }
    /// Bars that rise instead of running left to right.
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }
    /// The colour of the quiet zone: a role or any fill (default `Primary`).
    pub fn color(mut self, fill: impl Into<Fill>) -> Self {
        self.quiet = fill.into();
        self
    }
    /// The bars' width.
    pub fn w(mut self, w: impl Into<Len>) -> Self {
        self.w = Some(w.into());
        self
    }
    /// The bars' height.
    pub fn h(mut self, h: impl Into<Len>) -> Self {
        self.h = Some(h.into());
        self
    }

    /// The tree, finished.
    pub fn el(self) -> El {
        let Meter {
            id,
            levels,
            hold,
            floor,
            segments,
            ticks,
            readout,
            vertical,
            quiet,
            w,
            h,
        } = self;
        let at: Vec<f64> = levels.iter().map(|&l| meter_position(l, floor)).collect();
        let held: Vec<f64> = hold.iter().map(|&l| meter_position(l, floor)).collect();
        let channels = at.len().max(1);
        let (ink, danger) = (meter_position(0.251, floor), meter_position(0.708, floor));
        let tick_at: Vec<f64> = [-48.0_f32, -36., -24., -12., -6., 0.]
            .into_iter()
            .filter(|&db| db > floor)
            .map(|db| f64::from((db - floor) / -floor))
            .collect();
        let zone = move |p: f64| -> Fill {
            if p > danger {
                Role::Danger.into()
            } else if p > ink {
                Role::Ink.into()
            } else {
                quiet.clone()
            }
        };
        let bars = canvas(move |size| {
            let (len, across) = if vertical {
                (size.height, size.width)
            } else {
                (size.width, size.height)
            };
            let marks = if ticks { 3.0 } else { 0.0 };
            let gap = 1.0;
            let bar = ((across - marks - gap * (channels - 1) as f64) / channels as f64).max(1.0);
            // A span along the travel (0..1 from quiet to loud) in band `c`.
            let span = |a: f64, b: f64, c: usize| {
                let (a, b) = (a * len, b * len);
                let x = c as f64 * (bar + gap);
                if vertical {
                    Path::rect(x, len - b, x + bar, len - a)
                } else {
                    Path::rect(a, x, b, x + bar)
                }
            };
            let off: Fill = Role::Dim.alpha(0.14);
            let mut draws = Vec::new();
            for (c, (&level, &hold)) in at.iter().zip(&held).enumerate() {
                if segments > 0 {
                    let pitch = 1.0 / segments as f64;
                    let top = ((hold * segments as f64).ceil() as usize).saturating_sub(1);
                    for i in 0..segments {
                        let (a, b) = (i as f64 * pitch, (i + 1) as f64 * pitch);
                        let lit = level > 0.0 && a < level || hold > 0.0 && i == top;
                        let fill = if lit { zone(b) } else { off.clone() };
                        draws.push(Draw::fill(span(a, b - pitch * 0.15, c), fill));
                    }
                } else {
                    draws.push(Draw::fill(span(0.0, 1.0, c), off.clone()));
                    for (a, b) in [(0.0, ink), (ink, danger), (danger, 1.0)] {
                        let b = b.min(level);
                        if b > a {
                            draws.push(Draw::fill(span(a, b, c), zone(b)));
                        }
                    }
                    if hold > 0.0 {
                        let t = 2.0 / len.max(1.0);
                        draws.push(Draw::fill(span((hold - t).max(0.0), hold, c), zone(hold)));
                    }
                }
            }
            if ticks {
                let x = channels as f64 * (bar + gap) - gap + 1.0;
                for &t in &tick_at {
                    let p = (t * len).clamp(0.0, len - 1.0);
                    draws.push(Draw::fill(
                        if vertical {
                            Path::rect(x, len - p - 1.0, x + 2.0, len - p)
                        } else {
                            Path::rect(p, x, p + 1.0, x + 2.0)
                        },
                        Role::Dim,
                    ));
                }
            }
            draws
        });
        let thick =
            Len::Px(channels as f64 * 4.0 + (channels - 1) as f64 + if ticks { 3.0 } else { 0.0 });
        let bars = if vertical {
            bars.w(w.unwrap_or(thick)).h(h.unwrap_or(Len::Pct(100.0)))
        } else {
            bars.w(w.unwrap_or(Len::Pct(100.0))).h(h.unwrap_or(thick))
        }
        .shrink(0);
        let body = if readout {
            let peak = hold.iter().copied().fold(0.0_f32, f32::max);
            let text_db = if peak <= 1e-4 {
                "-inf".to_owned()
            } else {
                format!("{:+.1}", 20.0 * peak.log10())
            };
            let reading = text(text_db)
                .fill(if peak >= 0.999 {
                    Role::Danger
                } else {
                    Role::Dim
                })
                .lines(1)
                .measure_as("-00.0");
            if vertical {
                col![bars, reading].gap(Xs).align(Align::Center)
            } else {
                row![bars, reading].gap(Xs).align(Align::Center)
            }
        } else {
            bars
        };
        body.cursor(Cursor::Hand)
            .named("Level")
            .tip("Level · click clears the held peak")
            .id(id)
    }
}
impl From<Meter> for El {
    fn from(m: Meter) -> Self {
        m.el()
    }
}
impl IntoEl for Meter {
    fn into_el(self) -> El {
        self.el()
    }
}
