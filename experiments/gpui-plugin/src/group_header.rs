//! Editable group envelope/output study, built with the same MUI fillets and GPUI controls.
use crate::{kurv::ink, live_theme::color};
use gpui::{prelude::*, *};
use mui::core::Rgb;

pub struct GroupHeader {
    name: Entity<crate::text_input::TextInput>,
    seed: Entity<crate::text_input::TextInput>,
    _observers: Vec<Subscription>,
    color: Rgb,
    picker: bool,
    error: String,
    values: [f32; 6],
    drag: Option<(usize, Point<Pixels>, f32)>,
    midi: [usize; 2],
    menu: Option<usize>,
}
impl GroupHeader {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.text_system()
            .add_fonts(vec![std::borrow::Cow::Borrowed(include_bytes!(
                "../assets/Phosphor.ttf"
            ))])
            .expect("bundled Phosphor font");
        let name =
            cx.new(|cx| crate::text_input::TextInput::with_value(cx, "GROUP 01", "Group name"));
        let seed = cx.new(|cx| {
            crate::text_input::TextInput::with_value(cx, "0.35 0 240", "Group OKLCH color")
        });
        let name_observer = cx.observe(&name, |_, _, cx| cx.notify());
        let color_observer = cx.observe(&seed, |this, input, cx| {
            match crate::live_theme::parse(input.read(cx).value()) {
                Ok(color) => {
                    this.color = color;
                    this.error.clear();
                }
                Err(error) => this.error = error,
            };
            cx.notify();
        });
        Self {
            name,
            seed,
            _observers: vec![name_observer, color_observer],
            color: Rgb::from_oklch(0.35, 0., 240.).unwrap(),
            picker: false,
            error: String::new(),
            values: [0.08, 0.25, 0.7, 0.4, 1., 0.],
            drag: None,
            midi: [0, 0],
            menu: None,
        }
    }
    fn edit(&mut self, index: usize, delta: f32, cx: &mut Context<Self>) {
        let (min, max) = if index == 5 {
            (-1., 1.)
        } else if index == 4 {
            (0., 2.)
        } else {
            (0., 1.)
        };
        self.values[index] = (self.values[index] + delta).clamp(min, max);
        cx.notify();
    }
}
fn rgb_value(c: Rgb) -> u32 {
    (u32::from(c.0) << 16) | (u32::from(c.1) << 8) | u32::from(c.2)
}
/// At 75 degrees to horizontal the run is height / tan(75°).
fn shoulder(width: f64) -> anyhow::Result<Vec<gpui::Path<Pixels>>> {
    let split = width as f32 * 0.55;
    let run = 96. / 75_f32.to_radians().tan();
    let p = |x, y| point(px(x), px(y));
    let mut path = PathBuilder::fill();
    path.move_to(p(-12., 0.));
    path.line_to(p(split - 10., 0.));
    path.cubic_bezier_to(p(split, 12.), p(split, 0.), p(split + 2., 4.));
    path.line_to(p(split - run, 108.));
    path.cubic_bezier_to(
        p(split - run + 10., 120.),
        p(split - run - 2., 116.),
        p(split - run, 120.),
    );
    path.line_to(p(0., 120.));
    path.line_to(p(0., 12.));
    path.cubic_bezier_to(p(-12., 0.), p(0., 0.), p(0., 0.));
    path.close();
    Ok(vec![path.build()?])
}
impl Render for GroupHeader {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let tint = self.color;
        let text = Rgb::WHITE.contrast_on(&[tint], 4.5).unwrap_or(Rgb::BLACK);
        let values = self.values;
        let dragging = self.drag.map(|d| d.0);
        let background = canvas(
            |b, _, _| shoulder(f64::from(b.size.width)).unwrap_or_default(),
            move |bounds, paths, window, _| {
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);
                let mut outer = PathBuilder::fill();
                let p = |x, y| bounds.origin + point(px(x), px(y));
                outer.move_to(p(-12., 0.));
                outer.line_to(p(w + 12., 0.));
                outer.cubic_bezier_to(p(w, 12.), p(w, 0.), p(w, 0.));
                outer.line_to(p(w, h));
                outer.line_to(p(0., h));
                outer.line_to(p(0., 12.));
                outer.cubic_bezier_to(p(-12., 0.), p(0., 0.), p(0., 0.));
                outer.close();
                if let Ok(path) = outer.build() {
                    window.paint_path(path, color(0x252c28));
                }
                window.paint_quad(fill(
                    Bounds::new(
                        bounds.bottom_left() - point(px(0.), px(1.)),
                        size(bounds.size.width, px(1.)),
                    ),
                    color(0x718164),
                ));
                for mut path in paths {
                    path.bounds.origin += bounds.origin;
                    for v in &mut path.vertices {
                        v.xy_position += bounds.origin;
                    }
                    window.paint_path(path, rgb(rgb_value(tint)));
                }
                let mut edge = PathBuilder::stroke(px(1.));
                edge.move_to(p(0., h));
                edge.line_to(p(0., 12.));
                edge.cubic_bezier_to(p(-12., 0.), p(0., 0.), p(0., 0.));
                edge.line_to(p(w + 12., 0.));
                edge.cubic_bezier_to(p(w, 12.), p(w, 0.), p(w, 0.));
                edge.line_to(p(w, h));
                if let Ok(path) = edge.build() {
                    window.paint_path(path, color(0x718164));
                }
                window.paint_quad(fill(
                    Bounds::new(p(0., h - 1.), size(px(w), px(1.))),
                    color(0x718164),
                ));
            },
        )
        .absolute()
        .size_full();
        let envelope = canvas(
            |_, _, _| (),
            move |b, _, window, _| {
                let w = f32::from(b.size.width);
                let h = f32::from(b.size.height);
                let mut area = PathBuilder::fill();
                let pp = |x: f32, y: f32| b.origin + point(px(x * w), px(y * h));
                area.move_to(pp(0., 1.));
                area.line_to(pp(0.08 + values[0] * 0.18, 0.));
                area.cubic_bezier_to(
                    pp(0.30 + values[1] * 0.30, 1. - values[2]),
                    pp(0.3, 0.),
                    pp(0.33, 1. - values[2]),
                );
                area.line_to(pp(0.7, 1. - values[2]));
                area.cubic_bezier_to(
                    pp(0.80 + values[3] * 0.2, 1.),
                    pp(0.8, 1. - values[2]),
                    pp(0.82 + values[3] * 0.15, 1.),
                );
                area.close();
                if let Ok(area) = area.build() {
                    let mut top = Hsla::from(rgb(rgb_value(text)));
                    top.a = 0.32;
                    let mut bottom = top;
                    bottom.a = 0.;
                    window.paint_path(
                        area,
                        linear_gradient(
                            180.,
                            linear_color_stop(top, 0.),
                            linear_color_stop(bottom, 1.),
                        ),
                    );
                }
                let mut path = PathBuilder::stroke(px(1.5));
                let p = |x: f32, y: f32| b.origin + point(px(x * w), px(y * h));
                path.move_to(p(0., 1.));
                path.line_to(p(0.08 + values[0] * 0.18, 0.));
                path.cubic_bezier_to(
                    p(0.30 + values[1] * 0.30, 1. - values[2]),
                    p(0.3, 0.),
                    p(0.33, 1. - values[2]),
                );
                path.line_to(p(0.7, 1. - values[2]));
                path.cubic_bezier_to(
                    p(0.80 + values[3] * 0.2, 1.),
                    p(0.8, 1. - values[2]),
                    p(0.82 + values[3] * 0.15, 1.),
                );
                if let Ok(path) = path.build() {
                    window.paint_path(path, rgb(rgb_value(text)));
                }
            },
        )
        .w_full()
        .h(px(46.));
        let mut env = div().relative().flex_1().h(px(62.)).child(envelope);
        for i in 0..4 {
            let label = if dragging == Some(i) {
                format!(
                    "{} {:.0}{}",
                    ["A", "D", "S", "R"][i],
                    values[i] * if i == 2 { 100. } else { 1000. },
                    if i == 2 { "%" } else { "ms" }
                )
            } else {
                String::new()
            };
            env = env.child(
                div()
                    .id(("envelope-stage", i))
                    .absolute()
                    .left(relative(match i {
                        0 => 0.08 + values[0] * 0.18,
                        1 => 0.30 + values[1] * 0.30,
                        2 => 0.7,
                        _ => 0.80 + values[3] * 0.2,
                    }))
                    .ml(px(-9.))
                    .top(px(if i == 0 {
                        0.
                    } else if i == 3 {
                        40.
                    } else {
                        (1. - values[2]) * 46.
                    }))
                    .mt(px(-8.))
                    .w(px(18.))
                    .h(px(18.))
                    .cursor(if i == 2 {
                        CursorStyle::ResizeUpDown
                    } else {
                        CursorStyle::ResizeLeftRight
                    })
                    .tab_index(i as isize + 1)
                    .role(Role::Slider)
                    .aria_label(format!(
                        "{} {:.2}",
                        ["Attack", "Decay", "Sustain", "Release"][i],
                        values[i]
                    ))
                    .child(
                        div()
                            .absolute()
                            .top(px(19.))
                            .child(ink(&label, 10., rgb_value(text))),
                    )
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                            this.drag = Some((i, e.position, this.values[i]));
                            cx.notify();
                        }),
                    )
                    .map(|el| {
                        crate::controls::parameter(
                            el,
                            crate::modulation::Target::GroupParameter(i),
                            true,
                            cx.listener(move |this, delta: &f32, _, cx| {
                                this.edit(i, delta * 0.01, cx);
                            }),
                        )
                    }),
            );
        }
        let left = div()
            .w(relative(0.55))
            .h_full()
            .pl(px(58.))
            .pr(px(35.))
            .flex()
            .items_center()
            .gap(px(20.))
            .child(
                div()
                    .id("group-name-color")
                    .cursor_pointer()
                    .tab_index(0)
                    .role(Role::Button)
                    .aria_label("Edit group name and color")
                    .flex_shrink_0()
                    .text_size(px(18.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(rgb_value(text)))
                    .child(self.name.read(cx).value().to_owned())
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.picker = !this.picker;
                        cx.notify();
                    })),
            )
            .child(env);
        let mut right = div()
            .flex_1()
            .h_full()
            .pl(px(12.))
            .pr(px(12.))
            .pt(px(40.))
            .flex()
            .flex_col()
            .gap(px(5.));
        let pan = if values[5].abs() < 0.005 {
            "C".into()
        } else {
            format!(
                "{} {:.0}",
                if values[5] < 0. { "L" } else { "R" },
                values[5].abs() * 100.
            )
        };
        right = right.child(
            div()
                .flex()
                .gap(px(28.))
                .child(self.control(
                    4,
                    "GAIN",
                    format!("{:.1} dB", 20. * values[4].max(0.0001).log10()),
                    0xe0e5da,
                    cx,
                ))
                .child(self.control(5, "PAN", pan, 0xe0e5da, cx)),
        );
        let mut midi = div().flex().gap(px(12.));
        for i in 0..2 {
            let channel = if self.midi[i] == 0 {
                "OMNI".into()
            } else {
                format!("CH {}", self.midi[i])
            };
            midi = midi.child(
                div()
                    .id(("midi-channel", i))
                    .flex()
                    .items_center()
                    .gap(px(5.))
                    .cursor_pointer()
                    .tab_index(8 + i as isize)
                    .role(Role::Button)
                    .aria_label(format!(
                        "MIDI {} channel {}",
                        if i == 0 { "input" } else { "output" },
                        channel
                    ))
                    .child(midi_icon())
                    .child(ink(if i == 0 { "IN" } else { "OUT" }, 9., 0x9ba697))
                    .child(ink(&channel, 11., 0xe0e5da))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu = if this.menu == Some(i) { None } else { Some(i) };
                        cx.notify();
                    })),
            );
        }
        right = right.child(midi);
        let mut root = div()
            .id("group-header-body")
            .relative()
            .size_full()
            .child(background)
            .child(div().flex().size_full().child(left).child(right))
            .child(div().absolute().left(relative(0.50)).top(px(68.)).child(
                crate::modulation::source(crate::modulation::Source::Group(0)),
            ))
            .on_action(cx.listener(|this, _: &crate::controls::Cancel, _, cx| {
                if this.drag.is_some() || this.picker || this.menu.is_some() {
                    if let Some((i, _, value)) = this.drag.take() {
                        this.values[i] = value;
                    }
                    this.picker = false;
                    this.menu = None;
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if let Some((i, start, value)) = this.drag {
                    this.values[i] = value;
                    this.edit(
                        i,
                        if i < 4 && i != 2 {
                            f32::from(e.position.x - start.x) * 0.005
                        } else {
                            f32::from(start.y - e.position.y) * 0.01
                        },
                        cx,
                    );
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| {
                    this.drag = None;
                    cx.notify();
                }),
            );
        if self.picker {
            let mut picker = div()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.picker = false;
                    cx.notify();
                }))
                .w(px(290.))
                .p(px(12.))
                .rounded(px(10.))
                .bg(color(0x343f32))
                .border_1()
                .border_color(color(0x718164))
                .flex()
                .flex_col()
                .gap(px(6.))
                .child(ink("GROUP NAME / COLOR", 11., 0xe0e5da))
                .child(self.name.clone())
                .child(self.seed.clone());
            let mut colors = div().flex().gap(px(5.));
            for (i, h) in [0., 40., 80., 145., 200., 260., 310.]
                .into_iter()
                .enumerate()
            {
                let c = Rgb::from_oklch(0.45, 0.10, h).unwrap();
                colors = colors.child(
                    div()
                        .id(("group-color", i))
                        .size(px(28.))
                        .rounded(px(5.))
                        .bg(rgb(rgb_value(c)))
                        .cursor_pointer()
                        .tab_index(i as isize)
                        .role(Role::Button)
                        .aria_label(format!("Group hue {h}"))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.seed.update(cx, |input, cx| {
                                input.set_value(&format!("0.35 0.10 {h}"), cx)
                            })
                        })),
                );
            }
            picker = picker
                .child(colors)
                .child(ink(&self.error, 9., 0x9ba697))
                .child(
                    div()
                        .id("close-group-picker")
                        .cursor_pointer()
                        .child(ink("DONE", 11., 0xe0e5da))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.picker = false;
                            cx.notify();
                        })),
                );
            root = root.child(
                div().absolute().left(px(58.)).top(px(34.)).child(
                    deferred(anchored().snap_to_window_with_margin(px(8.)).child(picker))
                        .with_priority(1),
                ),
            );
        }
        if let Some(which) = self.menu {
            let mut menu = div()
                .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                    this.menu = None;
                    cx.notify();
                }))
                .w(px(240.))
                .p(px(8.))
                .bg(color(0x343f32))
                .border_1()
                .border_color(color(0x718164))
                .rounded(px(8.))
                .flex()
                .flex_wrap()
                .gap(px(4.));
            for channel in 0..=16 {
                menu = menu.child(
                    div()
                        .id(("midi-option", channel))
                        .w(px(40.))
                        .p(px(5.))
                        .cursor_pointer()
                        .tab_index(channel as isize)
                        .role(Role::Button)
                        .aria_label(format!("Select MIDI channel {channel}"))
                        .child(ink(
                            &if channel == 0 {
                                "OMNI".into()
                            } else {
                                channel.to_string()
                            },
                            11.,
                            0xe0e5da,
                        ))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.midi[which] = channel;
                            this.menu = None;
                            cx.notify();
                        })),
                );
            }
            root = root.child(
                div().absolute().right(px(10.)).top(px(110.)).child(
                    deferred(
                        anchored()
                            .anchor(Anchor::TopRight)
                            .snap_to_window_with_margin(px(8.))
                            .child(menu),
                    )
                    .with_priority(1),
                ),
            );
        }
        root
    }
}
impl GroupHeader {
    fn control(
        &self,
        i: usize,
        name: &str,
        value: String,
        text: u32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = format!("{name} {value}; drag or arrow keys");
        div()
            .id(("group-value", i))
            .relative()
            .min_w(px(38.))
            .h(px(38.))
            .cursor(CursorStyle::ResizeUpDown)
            .tab_index(i as isize + 1)
            .role(Role::Slider)
            .aria_label(label)
            .child(ink(name, 9., text))
            .child(ink(&value, 12., text))
            .when(i >= 4, |el| {
                let value = self.values[i];
                el.child(
                    canvas(
                        |_, _, _| (),
                        move |b, _, window, _| {
                            let y = b.origin.y + px(2.);
                            let x = b.origin.x;
                            let w = b.size.width;
                            window.paint_quad(fill(
                                Bounds::new(point(x, y), size(w, px(2.))),
                                color(0x718164),
                            ));
                            let unit = if i == 4 {
                                value / 2.
                            } else {
                                (value + 1.) / 2.
                            };
                            let origin: f32 = if i == 4 { 0. } else { 0.5 };
                            window.paint_quad(fill(
                                Bounds::new(
                                    point(x + w * origin.min(unit), y),
                                    size(w * (unit - origin).abs(), px(2.)),
                                ),
                                color(0xbadc91),
                            ));
                            window.paint_quad(
                                fill(
                                    Bounds::new(
                                        point(x + w * unit - px(2.), y - px(2.)),
                                        size(px(4.), px(6.)),
                                    ),
                                    color(0xe0e5da),
                                )
                                .corner_radii(px(2.)),
                            );
                        },
                    )
                    .w_full()
                    .h(px(8.)),
                )
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, e: &MouseDownEvent, _, _| {
                    this.drag = Some((i, e.position, this.values[i]))
                }),
            )
            .map(|el| {
                crate::controls::parameter(
                    el,
                    crate::modulation::Target::GroupParameter(i),
                    true,
                    cx.listener(move |this, delta: &f32, _, cx| {
                        this.edit(i, delta * 0.01, cx);
                    }),
                )
            })
    }
}
pub(super) fn icon(glyph: &'static str) -> impl IntoElement {
    div()
        .font_family("Phosphor")
        .text_size(px(16.))
        .text_color(color(0xe0e5da))
        .child(glyph)
}
fn midi_icon() -> impl IntoElement {
    icon("\u{e9c8}")
}
#[::core::prelude::v1::test]
fn group_header_contract() {
    assert!((120. / 75_f64.to_radians().tan() - 32.1539).abs() < 0.001);
    for width in [648., 1000.] {
        assert!(!shoulder(width).unwrap()[0].vertices.is_empty());
    }
}
