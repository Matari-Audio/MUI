//! Reference integration: MUI owns layout/paths; GPUI owns text and interaction.
use super::{Edit, ProbeView};
use gpui::{prelude::*, *};
use mui::{
    core::{Color, Ui},
    geometry,
    layout::{Available, Fill, Flow, Size as MuiSize},
};
use std::{cell::Cell, rc::Rc};

fn fixture(width: f64, gain: f64) -> anyhow::Result<Ui> {
    use mui::prelude::{container, item};
    Ok(container([
        item("heading").text("MUI • layout, geometry, text and input").width(Fill),
        item("gain").text(format!("Gain: {:.0}% — click or press Enter", gain * 100.))
            .width(Fill).min(0., 64.).pad(16.).round(20.).color(Color::Panel).on_tap("gain"),
        item("description").text("This paragraph is measured and drawn by the same GPUI text system. MUI wraps the panel layout; scrolling clips the geometry, text and input together.").width(Fill),
        item("name-label").text("Editable preset name — selection, clipboard and IME input").width(Fill),
        item("name").width(Fill).height(40.),
        item("space").height(260.),
        item("footer").text("End of scrollable content").width(Fill),
    ]).layout(Flow::Column).gap(16.).pad(16.).width(width).build()?)
}
fn color(c: mui::core::Rgb) -> Hsla {
    rgb((u32::from(c.0) << 16) | (u32::from(c.1) << 8) | u32::from(c.2)).into()
}
fn run(text: &str, color: Hsla) -> TextRun {
    TextRun {
        len: text.len(),
        font: font("DejaVu Sans"),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    }
}
fn shape(
    window: &Window,
    text: &str,
    width: Option<f64>,
    color: Hsla,
) -> anyhow::Result<Vec<WrappedLine>> {
    Ok(window
        .text_system()
        .shape_text(
            text.to_owned().into(),
            px(16.),
            &[run(text, color)],
            width.map(|w| px(w as f32)),
            None,
        )?
        .into_vec())
}
fn measure(
    window: &Window,
    text: &str,
    width: Option<f64>,
    minimum: bool,
) -> anyhow::Result<MuiSize> {
    let lines = shape(window, text, width, white().into())?;
    Ok(MuiSize::new(
        lines
            .iter()
            .map(|line| {
                if !minimum {
                    return f64::from(line.size(px(24.)).width);
                }
                let mut previous = 0.;
                line.wrap_boundaries
                    .iter()
                    .map(|b| {
                        line.unwrapped_layout.runs[b.run_ix].glyphs[b.glyph_ix]
                            .position
                            .x
                    })
                    .chain(std::iter::once(line.unwrapped_layout.width))
                    .map(|end| {
                        let end = f64::from(end);
                        let width = (end - previous).abs();
                        previous = end;
                        width
                    })
                    .fold(0., f64::max)
            })
            .fold(0., f64::max),
        lines
            .iter()
            .map(|l| f64::from(l.size(px(24.)).height))
            .sum(),
    ))
}
impl ProbeView {
    fn drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((start, value, active)) = self.drag.as_mut() else {
            return;
        };
        let distance = f64::from(event.position.x - start.x);
        if !*active && distance.abs() < 3. {
            return;
        }
        if !*active {
            *active = true;
            self.click_armed = false;
            let _ = self.edits.send(Edit::Begin);
        }
        let next = (*value + distance / 300.).clamp(0., 1.);
        if self.value != next {
            self.value = next;
            let _ = self.edits.send(Edit::Value(next));
            cx.notify();
        }
        cx.stop_propagation();
    }
    pub(super) fn end_drag(&mut self) {
        if let Some((_, _, active)) = self.drag.take() {
            if active {
                self.click_armed = false;
                let _ = self.edits.send(Edit::End);
            }
        }
    }

    fn panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> anyhow::Result<AnyElement> {
        let ui = fixture(f64::from(window.viewport_size().width).max(1.), self.value)?;
        let scene = ui.resolve_with(|_, text, input| {
            let width = input.known.width.or(match input.width {
                Available::Definite(w) => Some(w),
                _ => None,
            });
            let result =
                if matches!(input.width, Available::MinContent) && input.known.width.is_none() {
                    // GPUI breaks long words at glyph boundaries; zero width asks its own shaper
                    // for that minimum rather than inventing a second line-breaking algorithm.
                    measure(window, text, Some(0.), true)
                } else {
                    measure(window, text, width, false)
                };
            result
                .map(|measured| {
                    MuiSize::new(
                        input.known.width.unwrap_or(measured.width),
                        input.known.height.unwrap_or(measured.height),
                    )
                })
                .map_err(|e| mui::layout::Error::Backend(e.to_string()))
        })?;
        let styles = ui.resolved_styles(None)?;
        let tolerance = 0.1 / f64::from(window.scale_factor());
        let mut surfaces = Vec::new();
        for (surface, _) in ui.outlines(&scene) {
            if let Some(fill) = styles[surface.id.as_str()].fill {
                surfaces.push((surface.path.flatten(tolerance, 100_000)?, color(fill)));
            }
        }
        let mut paragraphs = Vec::new();
        for (id, info) in ui.items() {
            if let Some(text) = &info.text {
                let frame = scene.layout.content_frame(id).unwrap();
                paragraphs.push((
                    frame,
                    shape(window, text, Some(frame.size.width), color(styles[id].text))?,
                ));
            }
        }
        let semantics = ui
            .items()
            .filter_map(|(id, info)| {
                let text = info.text.as_ref()?;
                if id == "gain" {
                    return None;
                }
                let frame = scene.layout.content_frame(id)?;
                Some(
                    div()
                        .id(SharedString::from(id.to_owned()))
                        .role(Role::Label)
                        .aria_label(text.clone())
                        .absolute()
                        .left(px(frame.x as f32))
                        .top(px(frame.y as f32))
                        .w(px(frame.size.width as f32))
                        .h(px(frame.size.height as f32)),
                )
            })
            .collect::<Vec<_>>();
        let gain = scene.layout.frame("gain").unwrap();
        let name = scene.layout.frame("name").unwrap();
        let gain_path = scene.surface("gain").unwrap().path.clone();
        let origin = Rc::new(Cell::new(Point::default()));
        let paint_origin = origin.clone();
        let width = px(scene.layout.size.width as f32);
        let height = px(scene.layout.size.height as f32);
        let drag_target = cx.entity().downgrade();
        let canvas = canvas(
            move |bounds, _, _| {
                paint_origin.set(bounds.origin);
            },
            move |bounds, _, window, cx| {
                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = drag_target.update(cx, |view, cx| view.drag_move(event, cx));
                    }
                });
                for (contours, fill) in surfaces {
                    let mut builder = PathBuilder::fill().with_style(PathStyle::Fill(
                        FillOptions::default().with_fill_rule(FillRule::NonZero),
                    ));
                    for ring in contours {
                        for (i, p) in ring.iter().enumerate() {
                            let point = bounds.origin + point(px(p.x as f32), px(p.y as f32));
                            if i == 0 {
                                builder.move_to(point);
                            } else {
                                builder.line_to(point);
                            }
                        }
                        builder.close();
                    }
                    match builder.build() {
                        Ok(path) => window.paint_path(path, fill),
                        Err(e) => eprintln!("MUI path: {e}"),
                    }
                }
                for (frame, lines) in paragraphs {
                    let mut origin = bounds.origin + point(px(frame.x as f32), px(frame.y as f32));
                    for line in lines {
                        if let Err(e) =
                            line.paint(origin, px(24.), TextAlign::Left, None, window, cx)
                        {
                            eprintln!("MUI text: {e}");
                        }
                        origin.y += line.size(px(24.)).height;
                    }
                }
            },
        )
        .absolute()
        .size_full();
        let press_path = gain_path.clone();
        let press_origin = origin.clone();
        let gain = div()
            .id("gain")
            .absolute()
            .left(px(gain.x as f32))
            .top(px(gain.y as f32))
            .w(px(gain.size.width as f32))
            .h(px(gain.size.height as f32))
            .role(Role::Button)
            .aria_label(format!("Gain {:.0} percent; toggle", self.value * 100.))
            .track_focus(&self.gain_focus)
            .cursor_pointer()
            .focus(|s| s.border_2().border_color(white()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    this.click_armed = false;
                    let p = event.position - press_origin.get();
                    if press_path
                        .contains(
                            geometry::Point::new(f64::from(p.x), f64::from(p.y)),
                            tolerance,
                            100_000,
                        )
                        .unwrap_or(false)
                    {
                        this.click_armed = true;
                        window.focus(&this.gain_focus, cx);
                        this.drag = Some((event.position, this.value, false));
                    }
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _, _, _| this.end_drag()),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, _| {
                    this.click_armed = false;
                    this.end_drag();
                }),
            )
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, _, cx| {
                if event.keystroke.key == "escape" {
                    this.click_armed = false;
                    this.end_drag();
                    cx.stop_propagation();
                }
            }))
            .on_click(cx.listener(move |this, event: &ClickEvent, _, cx| {
                if matches!(event, ClickEvent::Mouse(_)) && !std::mem::take(&mut this.click_armed) {
                    return;
                }
                if !matches!(event, ClickEvent::Keyboard(_)) {
                    let p = event.position() - origin.get();
                    if !gain_path
                        .contains(
                            geometry::Point::new(f64::from(p.x), f64::from(p.y)),
                            tolerance,
                            100_000,
                        )
                        .unwrap_or(false)
                    {
                        return;
                    }
                }
                this.toggle(cx);
            }));
        Ok(div()
            .id("mui-scroll")
            .tab_group()
            .track_scroll(&self.scroll)
            .on_key_down(cx.listener(|_, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "tab" {
                    if event.keystroke.modifiers.shift {
                        window.focus_prev(cx);
                    } else {
                        window.focus_next(cx);
                    }
                    cx.stop_propagation();
                }
            }))
            .size_full()
            .overflow_y_scroll()
            .bg(rgb(0x181c24))
            .child(
                div()
                    .relative()
                    .w(width)
                    .h(height)
                    .child(canvas)
                    .children(semantics)
                    .child(gain)
                    .child(
                        div()
                            .absolute()
                            .left(px(name.x as f32))
                            .top(px(name.y as f32))
                            .w(px(name.size.width as f32))
                            .h(px(name.size.height as f32))
                            .overflow_hidden()
                            .child(self.name.clone()),
                    ),
            )
            .into_any_element())
    }
}
impl Render for ProbeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.panel(window, cx).unwrap_or_else(|error| {
            div()
                .text_color(rgb(0xff8080))
                .child(format!("MUI panel: {error:#}"))
                .into_any_element()
        })
    }
}
