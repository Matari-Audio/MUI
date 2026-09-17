//! Composable editor foundation: KURV's placement, local state, local modulation routing mock.
use crate::oscillator::{OscillatorGroup, label};
use gpui::{prelude::*, *};

const TEXT: u32 = 0xe0e5da;
const MUTED: u32 = 0x9ba697;
const ACCENT: u32 = 0xbadc91;

/// The three racks are independent entities and can be embedded in another editor layout.
pub struct KurvWorkspace {
    groups: Entity<OscillatorGroup>,
    warps: Entity<ModuleRack>,
    modulators: Entity<ModuleRack>,
    text_scale: f32,
    overlay: Entity<crate::modulation::Overlay>,
    seed: Entity<crate::text_input::TextInput>,
    seed_status: String,
    _seed_observer: Subscription,
    focus: FocusHandle,
    focus_lost: Option<Subscription>,
}
impl KurvWorkspace {
    pub fn new(cx: &mut Context<Self>) -> Self {
        cx.set_global(crate::modulation::Routing::new());
        crate::text_input::bind_keys(cx);
        crate::live_theme::update("0.65 0 240").expect("neutral seed");
        let seed = cx.new(|cx| {
            crate::text_input::TextInput::with_value(cx, "0.65 0 240", "OKLCH theme seed")
        });
        let observer = cx.observe(&seed, |this, input, cx| {
            this.seed_status = match crate::live_theme::update(input.read(cx).value()) {
                Ok(()) => "LIVE · derived surfaces / text / accent".into(),
                Err(e) => e,
            };
            this.groups.update(cx, |_, cx| cx.notify());
            this.warps.update(cx, |_, cx| cx.notify());
            this.modulators.update(cx, |_, cx| cx.notify());
            cx.notify();
            cx.refresh_windows();
        });
        Self {
            focus: cx.focus_handle(),
            focus_lost: None,
            overlay: cx.new(crate::modulation::Overlay::new),
            seed,
            seed_status: "Neutral · try 0.65 0.12 240".into(),
            _seed_observer: observer,
            groups: cx.new(OscillatorGroup::new),
            warps: cx.new(|cx| ModuleRack::new("WARPS", &["SPECTRAL", "PHASE", "VA FILTER"], cx)),
            modulators: cx
                .new(|cx| ModuleRack::new("MODULATORS", &["MACROS", "LFO 01", "ENVELOPE 01"], cx)),
            text_scale: 1.,
        }
    }
    pub fn check_interactions(&self, window: &mut Window, cx: &mut App) {
        let curve = self
            .modulators
            .read(cx)
            .modules
            .iter()
            .find_map(|m| m.curve.clone())
            .expect("LFO curve");
        let groups = self.groups.clone();
        window.on_next_frame(move |window, _| {
            window.on_next_frame(move |window, cx| {
                crate::curve_editor::check_interactions(&curve, window, cx, move |window, cx| {
                    let next_groups = groups.clone();
                    OscillatorGroup::check_unison_interactions(
                        &groups,
                        window,
                        cx,
                        move |window, cx| {
                            crate::modulation::check_interactions(window, &next_groups, cx);
                        },
                    );
                });
            });
            window.refresh();
        });
    }
    fn scale(&mut self, delta: f32, cx: &mut Context<Self>) {
        self.text_scale = (self.text_scale + delta).clamp(0.85, 1.2);
        self.groups
            .update(cx, |group, cx| group.set_text_scale(self.text_scale, cx));
        for rack in [&self.warps, &self.modulators] {
            rack.update(cx, |rack, cx| {
                rack.text_scale = self.text_scale;
                cx.notify();
            });
        }
        cx.notify();
    }
}
impl Render for KurvWorkspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.focus_lost.is_none() {
            self.focus_lost = Some(cx.on_focus_lost(window, |this, window, cx| {
                window.focus(&this.focus, cx);
            }));
            if window.focused(cx).is_none() {
                window.focus(&self.focus, cx);
            }
        }
        let masthead = div()
            .flex()
            .items_center()
            .justify_between()
            .h(px(96.))
            .flex_shrink_0()
            .px(px(28.))
            .bg(crate::live_theme::color(0x343f32))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(18.))
                    .child(ink("KURV", 30. * self.text_scale, TEXT))
                    .child(ink("MUI / EDITOR STUDY", 10., MUTED)),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .w(px(330.))
                    .gap(px(4.))
                    .child(ink("OKLCH SEED · LIVE TOKENS", 10., MUTED))
                    .child(self.seed.clone())
                    .child(ink(&self.seed_status, 9., MUTED))
                    .child(
                        div().flex().gap(px(6.)).children(
                            [
                                ("NEUTRAL", "0.65 0 240"),
                                ("BLUE", "0.65 0.12 240"),
                                ("WARM", "0.55 0.10 60"),
                            ]
                            .map(|(name, value)| {
                                div()
                                    .id(name)
                                    .cursor_pointer()
                                    .text_size(px(10.))
                                    .text_color(crate::live_theme::color(TEXT))
                                    .child(name)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.seed.update(cx, |input, cx| input.set_value(value, cx))
                                    }))
                            }),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(ink("TYPE", 10., MUTED))
                    .child(
                        div()
                            .id("type-smaller")
                            .px(px(14.))
                            .py(px(10.))
                            .tab_index(0)
                            .role(Role::Button)
                            .aria_label("Decrease type size, width and weight")
                            .cursor_pointer()
                            .child(ink("−", 22., TEXT))
                            .on_click(cx.listener(|this, _, _, cx| this.scale(-0.05, cx))),
                    )
                    .child(
                        div()
                            .id("type-larger")
                            .px(px(14.))
                            .py(px(10.))
                            .tab_index(1)
                            .role(Role::Button)
                            .aria_label("Increase type size, width and weight")
                            .cursor_pointer()
                            .child(ink("+", 22., TEXT))
                            .on_click(cx.listener(|this, _, _, cx| this.scale(0.05, cx))),
                    ),
            );
        // Zero layout gap; padding belongs to each component, not to the rack arrangement.
        let workspace = div()
            .flex()
            .flex_1()
            .min_h_0()
            .child(
                div()
                    .relative()
                    .w(px(240.))
                    .flex_shrink_0()
                    .h_full()
                    .child(section_background("WARPS"))
                    .child(
                        self.warps
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_w(px(760.))
                    .h_full()
                    .child(section_background("OSCILLATORS"))
                    .child(
                        self.groups
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    ),
            )
            .child(
                div()
                    .relative()
                    .w(px(280.))
                    .flex_shrink_0()
                    .h_full()
                    .child(section_background("MODULATORS"))
                    .child(
                        self.modulators
                            .clone()
                            .cached(StyleRefinement::default().size_full()),
                    ),
            );
        div().id("kurv-workspace").track_focus(&self.focus).map(crate::controls::navigation).relative().flex().flex_col().size_full().min_w(px(1280.)).bg(crate::live_theme::color(0x171d1a))
            .child(masthead) .child(workspace)
            .child(div().h(px(30.)).flex_shrink_0().px(px(28.)).flex().items_center().child(ink("CLICK SOURCE > DRAG + / DRAG SOURCE > PARAM OR INPUT · PIE: DRAG / SHIFT FINE / DOUBLE-CLICK REMOVE · ESC CANCEL",10.,MUTED)))
            .child(div().absolute().size_full().child(self.overlay.clone()))
            .on_drag_move(crate::modulation::drag_move)
            .on_drop(crate::modulation::drop_source)
            .on_action(cx.listener(|_, _: &crate::controls::Cancel, window, cx| {
                cx.stop_active_drag(window);
                crate::modulation::cancel(cx);
            }))
    }
}

/// A composable rack of section bodies; these first bodies expose UI-local macro values.
pub struct ModuleRack {
    title: &'static str,
    modules: Vec<Module>,
    text_scale: f32,
    drag: Option<(usize, Point<Pixels>, f32)>,
}
struct Module {
    title: &'static str,
    value: f32,
    expanded: bool,
    enabled: bool,
    curve: Option<Entity<crate::curve_editor::CurveEditor>>,
}
impl ModuleRack {
    pub fn new(title: &'static str, names: &[&'static str], cx: &mut Context<Self>) -> Self {
        Self {
            title,
            modules: names
                .iter()
                .map(|title| Module {
                    title,
                    curve: title.starts_with("LFO").then(|| {
                        cx.new(|cx| crate::curve_editor::CurveEditor::new(Default::default(), cx))
                    }),
                    value: 0.5,
                    expanded: true,
                    enabled: true,
                })
                .collect(),
            text_scale: 1.,
            drag: None,
        }
    }
}
impl Render for ModuleRack {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut rack = div().flex().flex_col().w_full().px(px(48.)).child(
            div()
                .h(px(28.))
                .flex_shrink_0()
                .px(px(16.))
                .flex()
                .items_center()
                .child(div()),
        );
        for (i, module) in self.modules.iter().enumerate() {
            let heading = div()
                .flex()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .id(("expand", i))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .tab_index((i * 3) as isize)
                        .role(Role::Button)
                        .aria_label(format!("Expand or collapse {}", module.title))
                        .child(crate::group_header::icon(if module.expanded {
                            "\u{e136}"
                        } else {
                            "\u{e13a}"
                        }))
                        .child(ink(module.title, 16. * self.text_scale, TEXT))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.modules[i].expanded = !this.modules[i].expanded;
                            cx.notify();
                        })),
                )
                .child(
                    div()
                        .id(("power", i))
                        .w(px(24.))
                        .h(px(24.))
                        .rounded(px(3.))
                        .bg(crate::live_theme::color(if module.enabled {
                            ACCENT
                        } else {
                            MUTED
                        }))
                        .cursor_pointer()
                        .tab_index((i * 3 + 1) as isize)
                        .role(Role::Button)
                        .aria_label(format!("Toggle {}", module.title))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.modules[i].enabled = !this.modules[i].enabled;
                            if let Some(curve) = &this.modules[i].curve {
                                let enabled = this.modules[i].enabled;
                                curve.update(cx, |curve, cx| curve.set_enabled(enabled, cx));
                            }
                            crate::modulation::forget_source(
                                cx,
                                if this.title == "MODULATORS" {
                                    crate::modulation::Source::Modulator(i)
                                } else {
                                    crate::modulation::Source::Warp(i)
                                },
                            );
                            this.drag = None;
                            cx.notify();
                        })),
                );
            let source = if self.title == "MODULATORS" {
                crate::modulation::Source::Modulator(i)
            } else {
                crate::modulation::Source::Warp(i)
            };
            let count = crate::modulation::source_count(cx, source) + 1;
            let left = self.title == "MODULATORS";
            let enabled = module.enabled;
            let shell = canvas(
                move |b, _, _| {
                    use mui::geometry::{Point, Polygon};
                    let w = f64::from(b.size.width);
                    let h = f64::from(b.size.height);
                    let mut shapes = vec![Polygon::rectangle(0., 0., w, h).ok()?.into()];
                    if enabled {
                        shapes.extend(
                            crate::pie_container::PieContainer::port(count, 144., left)
                                .outer_shapes(Point::new(if left { -12. } else { w + 12. }, 19.))
                                .ok()?,
                        );
                    }
                    let path = crate::pie_container::rounded(&shapes).ok()?;
                    Some((
                        crate::oscillator::Ink::geometry(&path, 0).ok()?.path,
                        crate::oscillator::border_ink(&path).ok()?.path,
                    ))
                },
                |b, paths, window, _| {
                    if let Some((fill, border)) = paths {
                        for (mut path, role) in [(fill, 0x343f32), (border, 0x718164)] {
                            path.bounds.origin += b.origin;
                            for v in &mut path.vertices {
                                v.xy_position += b.origin;
                            }
                            window.paint_path(path, crate::live_theme::color(role));
                        }
                    }
                },
            )
            .absolute()
            .left(px(0.))
            .top(px(0.))
            .size_full();
            let mut body = div()
                .id(("module", i))
                .relative()
                .flex()
                .flex_col()
                .w_full()
                .p(px(16.))
                .when(self.title == "MODULATORS", |d| d.pl(px(20.)))
                .when(self.title == "WARPS", |d| d.pr(px(20.)))
                .rounded(px(12.))
                .child(shell)
                .child(heading);
            if module.expanded {
                if let Some(curve) = &module.curve {
                    body = body.child(curve.clone());
                } else {
                    let value = module.value;
                    let enabled = module.enabled;
                    let track = canvas(
                        |_, _, _| (),
                        move |bounds, _, window, _| {
                            let mut builder = PathBuilder::stroke(px(2.));
                            let y = bounds.origin.y + bounds.size.height / 2.;
                            builder.move_to(point(bounds.origin.x, y));
                            builder.line_to(point(bounds.origin.x + bounds.size.width * value, y));
                            if let Ok(path) = builder.build() {
                                window.paint_path(
                                    path,
                                    crate::live_theme::color(if enabled { ACCENT } else { MUTED }),
                                );
                            }
                        },
                    )
                    .w_full()
                    .h(px(80.));
                    body = body
                        .child(track)
                        .child(ink("PREVIEW AMOUNT", 10. * self.text_scale, MUTED))
                        .child(
                            div()
                                .id(("amount", i))
                                .mt(px(6.))
                                .h(px(38.))
                                .cursor(CursorStyle::ResizeUpDown)
                                .tab_index((i * 3 + 2) as isize)
                                .role(Role::Slider)
                                .aria_label(format!(
                                    "{} preview amount {:.0} percent",
                                    module.title,
                                    value * 100.
                                ))
                                .child(ink(
                                    &format!("{:.0}%", value * 100.),
                                    20. * self.text_scale,
                                    if enabled { ACCENT } else { MUTED },
                                ))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, e: &MouseDownEvent, _, cx| {
                                        if this.modules[i].enabled {
                                            if e.click_count == 2 {
                                                this.modules[i].value = 0.5;
                                                cx.notify();
                                            } else {
                                                this.drag =
                                                    Some((i, e.position, this.modules[i].value));
                                            }
                                        }
                                    }),
                                )
                                .map(|el| {
                                    crate::controls::parameter(
                                        el,
                                        crate::modulation::Target::ModuleParameter(source, 0),
                                        enabled,
                                        cx.listener(move |this, delta: &f32, _, cx| {
                                            if this.modules[i].enabled {
                                                this.modules[i].value = (this.modules[i].value
                                                    + delta * 0.01)
                                                    .clamp(0., 1.);
                                                cx.notify();
                                            }
                                        }),
                                    )
                                }),
                        );
                }
            }
            if module.enabled {
                let source = if self.title == "MODULATORS" {
                    crate::modulation::Source::Modulator(i)
                } else {
                    crate::modulation::Source::Warp(i)
                };
                body = body.child(
                    div()
                        .absolute()
                        .top(px(7.))
                        .when(self.title == "MODULATORS", |d| d.left(px(-24.)))
                        .when(self.title == "WARPS", |d| d.right(px(-24.)))
                        .child(crate::modulation::source(source)),
                );
            }
            rack = rack.child(body);
        }
        div()
            .id(SharedString::from(self.title))
            .size_full()
            .overflow_y_scroll()
            .child(rack)
            .on_action(cx.listener(|this, _: &crate::controls::Cancel, _, cx| {
                if let Some((i, _, initial)) = this.drag.take() {
                    this.modules[i].value = initial;
                    cx.notify();
                } else {
                    cx.propagate();
                }
            }))
            .on_mouse_move(cx.listener(|this, e: &MouseMoveEvent, _, cx| {
                if let Some((i, start, value)) = this.drag {
                    let d = f32::from(start.y - e.position.y) + f32::from(e.position.x - start.x);
                    let fine = if e.modifiers.shift { 0.1 } else { 1. };
                    this.modules[i].value = (value + d * 0.01 * fine).clamp(0., 1.);
                    cx.notify();
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.drag = None),
            )
    }
}
/// All headings use the same real variable outlines as the oscillator, including its width axis.
pub(super) fn ink(text: &str, size: f32, color: u32) -> AnyElement {
    if crate::oscillator::native_text() {
        return div()
            .id(SharedString::from(text.to_owned()))
            .role(Role::Label)
            .aria_label(text.to_owned())
            .font_family("Roboto")
            .font_weight(crate::oscillator::text_font(size).weight)
            .text_size(px(size))
            .line_height(px(size * 1.3))
            .text_color(crate::live_theme::color(color))
            .whitespace_nowrap()
            .child(text.to_owned())
            .into_any_element();
    }
    let drawing = match label(text, 0., size, size, false, color) {
        Ok(v) => v,
        Err(e) => return div().child(e.to_string()).into_any_element(),
    };
    let width = drawing.path.bounds.size.width + px(3.);
    let semantic = text.to_owned();
    div()
        .id(SharedString::from(semantic.clone()))
        .role(Role::Label)
        .aria_label(semantic)
        .w(width)
        .h(px(size * 1.3))
        .child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let mut path = drawing.path;
                    path.bounds.origin += bounds.origin;
                    for vertex in &mut path.vertices {
                        vertex.xy_position += bounds.origin;
                    }
                    window.paint_path(path, crate::live_theme::color(color));
                },
            )
            .size_full(),
        )
        .into_any_element()
}

fn section_background(label: &'static str) -> impl IntoElement {
    div()
        .absolute()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .border_r_1()
        .border_color(crate::live_theme::color(0x343f32))
        .child(div().opacity(0.055).child(ink(
            label,
            if label == "OSCILLATORS" { 82. } else { 25. },
            TEXT,
        )))
}
