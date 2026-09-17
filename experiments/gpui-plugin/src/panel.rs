//! MUI owns layout, geometry and viewport policy; GPUI supplies rendering and native UI primitives.
use super::ProbeView;
use gpui::{prelude::*, *};
use mui::{
    core::{Color, ResolvedScene, Ui, ViewItem, ViewState},
    geometry,
    layout::{Align, Available, Fill, Flow, Measurement, Overflow, Size as MuiSize},
};
use std::{cell::Cell, rc::Rc};

fn fixture(width: f64, height: f64, gain: f64) -> anyhow::Result<Ui> {
    use mui::prelude::{container, item};
    Ok(container([
        container([
            item("heading").text("MUI • layout, geometry, text and input"),
            item("badge").text("GPUI").shrink(0.),
        ]).id("header").align(Align::Baseline).wrap().gap(12.).width(Fill),
        item("gain").text(format!("Gain: {:.0}% — click or press Enter", gain * 100.))
            .width(Fill).min(0., 64.).shrink(0.).pad(16.).round(20.).color(Color::Panel).on_tap("gain"),
        item("description").text("GPUI shapes and paints this text. MUI supplies baselines, layout and the shared clipping/hit-test snapshot.").width(Fill).shrink(0.),
        item("name-label").text("Editable preset name — selection, clipboard and IME input").width(Fill).shrink(0.),
        item("name").width(Fill).height(40.),
        container([
            item("nested-title").text("Nested GPUI scroll area").height(40.).width(Fill).shrink(0.),
            item("nested-action").text("Scroll to reveal more").height(48.).width(Fill).shrink(0.).color(Color::Raised),
            item("nested-tail").text("End of nested content").height(240.).width(Fill).shrink(0.),
        ]).id("nested").layout(Flow::Column).align(Align::Start).width(Fill).max(340., 110.).height(110.).overflow(Overflow::Scroll),
        item("space").height(220.),
        item("footer").text("End of scrollable content").width(Fill).shrink(0.),
    ]).id("viewport").layout(Flow::Column).align(Align::Start).gap(16.).pad(16.).width(width).height(height).overflow(Overflow::Scroll).build()?)
}
fn typography(id: &str) -> (Pixels, Pixels) {
    if id == "badge" {
        (px(22.), px(30.))
    } else {
        (px(16.), px(24.))
    }
}
/// Reuse GPUI scroll containers/handles; MUI routes wheel remainders and supplies paint/pick coordinates.
struct PanelLayout {
    ui: Ui,
    scene: ResolvedScene,
    scrolls: [ScrollHandle; 2],
}
impl PanelLayout {
    fn state(&self) -> anyhow::Result<ViewState> {
        let mut state = ViewState::default();
        for (id, handle) in ["viewport", "nested"].into_iter().zip(&self.scrolls) {
            let limit = self.scene.layout.scroll_limit(id).unwrap();
            let offset = handle.offset();
            let x = (-f64::from(offset.x)).clamp(0., limit.width);
            let y = (-f64::from(offset.y)).clamp(0., limit.height);
            handle.set_offset(point(px(-x as f32), px(-y as f32)));
            state.set_scroll(id, geometry::Point::new(x, y))?;
        }
        Ok(state)
    }
    fn wheel(
        &self,
        event: &ScrollWheelEvent,
        origin: Point<Pixels>,
        line_height: Pixels,
    ) -> anyhow::Result<bool> {
        let mut state = self.state()?;
        let view = state.resolve(&self.ui, &self.scene)?;
        let p = event.position - origin;
        let p = geometry::Point::new(p.x.into(), p.y.into());
        if !view
            .item("viewport")
            .unwrap()
            .viewport
            .as_ref()
            .unwrap()
            .contains(p)
        {
            return Ok(false);
        }
        let delta = event.delta.pixel_delta(line_height);
        state.scroll_at(
            &view,
            p,
            geometry::Point::new(-f64::from(delta.x), -f64::from(delta.y)),
        )?;
        let view = state.resolve(&self.ui, &self.scene)?;
        for (id, handle) in ["viewport", "nested"].into_iter().zip(&self.scrolls) {
            let offset = view.item(id).unwrap().scroll;
            handle.set_offset(point(px(-offset.x as f32), px(-offset.y as f32)));
        }
        Ok(true)
    }
    fn gain_hit(&self, point: Point<Pixels>) -> bool {
        let result = (|| -> anyhow::Result<bool> {
            let state = self.state()?;
            let view = state.resolve(&self.ui, &self.scene)?;
            Ok(view.tap_at(geometry::Point::new(point.x.into(), point.y.into()))? == Some("gain"))
        })();
        result.unwrap_or(false)
    }
}
// This adapter emits only scroll translations. General affine text/widget rendering remains a backend extension.
fn mask(entry: &ViewItem, origin: Point<Pixels>, bounds: Bounds<Pixels>) -> ContentMask<Pixels> {
    let mut bounds = bounds;
    for clip in &entry.clips {
        let p = clip
            .transform
            .apply(geometry::Point::new(clip.frame.x, clip.frame.y));
        bounds = bounds.intersect(&Bounds::new(
            origin + point(px(p.x as f32), px(p.y as f32)),
            size(
                px(clip.frame.size.width as f32),
                px(clip.frame.size.height as f32),
            ),
        ));
    }
    ContentMask { bounds }
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
    id: &str,
) -> anyhow::Result<Vec<WrappedLine>> {
    Ok(window
        .text_system()
        .shape_text(
            text.to_owned().into(),
            typography(id).0,
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
    id: &str,
) -> anyhow::Result<Measurement> {
    let lines = shape(window, text, width, white(), id)?;
    let line_height = typography(id).1;
    let baseline = lines.first().map(|l| {
        f64::from(
            (line_height - l.unwrapped_layout.ascent - l.unwrapped_layout.descent) / 2.
                + l.unwrapped_layout.ascent,
        )
    });
    Ok(Measurement {
        baseline,
        size: MuiSize::new(
            lines
                .iter()
                .map(|line| {
                    if !minimum {
                        return f64::from(line.size(line_height).width);
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
                .map(|l| f64::from(l.size(line_height).height))
                .sum(),
        ),
    })
}
impl ProbeView {
    fn drag_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let Some((start, _, active)) = self.drag.as_mut() else {
            return;
        };
        let distance = f64::from(event.position.x - start.x);
        if !*active && distance.abs() < 3. {
            return;
        }
        if !*active {
            *active = true;
            self.click_armed = false;
            self.gain.begin();
        }
        self.gain.drag(distance / 300., event.modifiers.shift);
        let next = self.gain.value();
        if self.value != next {
            self.value = next;
            cx.notify();
        }
        cx.stop_propagation();
    }
    pub(super) fn end_drag(&mut self) {
        if let Some((_, _, true)) = self.drag.take() {
            self.click_armed = false;
            self.gain.end();
        }
    }

    fn panel(&mut self, window: &mut Window, cx: &mut Context<Self>) -> anyhow::Result<AnyElement> {
        let ui = fixture(
            f64::from(window.viewport_size().width).max(64.),
            f64::from(window.viewport_size().height).max(64.),
            self.value,
        )?;
        let scene = ui.resolve_with_baseline(|id, text, input| {
            let width = input.known.width.or(match input.width {
                Available::Definite(w) => Some(w),
                _ => None,
            });
            let minimum =
                matches!(input.width, Available::MinContent) && input.known.width.is_none();
            measure(
                window,
                text,
                if minimum { Some(0.) } else { width },
                minimum,
                id,
            )
            .map(|mut measured| {
                measured.size.width = input.known.width.unwrap_or(measured.size.width);
                measured.size.height = input.known.height.unwrap_or(measured.size.height);
                measured
            })
            .map_err(|e| mui::layout::Error::Backend(e.to_string()))
        })?;
        let baseline = |id| -> anyhow::Result<f64> {
            let frame = scene.layout.content_frame(id).unwrap();
            Ok(frame.y
                + measure(
                    window,
                    ui.info(id).unwrap().text.as_ref().unwrap(),
                    Some(frame.size.width),
                    false,
                    id,
                )?
                .baseline
                .unwrap())
        };
        self.baseline_error = (baseline("heading")? - baseline("badge")?).abs() as f32;
        let styles = ui.resolved_styles(None)?;
        let tolerance = 0.1 / f64::from(window.scale_factor());
        let mut surfaces = Vec::new();
        for (surface, _) in ui.outlines(&scene) {
            if let Some(fill) = styles[surface.id.as_str()].fill {
                surfaces.push((
                    surface.id.clone(),
                    surface.path.flatten(tolerance, 100_000)?,
                    color(fill),
                ));
            }
        }
        let mut paragraphs = Vec::new();
        for (id, info) in ui.items() {
            if let Some(text) = &info.text {
                let frame = scene.layout.content_frame(id).unwrap();
                paragraphs.push((
                    id.to_owned(),
                    frame,
                    shape(
                        window,
                        text,
                        Some(frame.size.width),
                        color(styles[id].text),
                        id,
                    )?,
                ));
            }
        }
        let viewport = scene.layout.content_frame("viewport").unwrap();
        let nested = scene.layout.content_frame("nested").unwrap();
        self.nested_y = nested.y as f32;
        let outer_height =
            viewport.size.height + scene.layout.scroll_limit("viewport").unwrap().height;
        let inner_height = nested.size.height + scene.layout.scroll_limit("nested").unwrap().height;
        let gain = scene.layout.frame("gain").unwrap();
        let name = scene.layout.frame("name").unwrap();
        let semantic = |id: &str, parent: mui::layout::Frame| {
            let text = ui.info(id).unwrap().text.as_ref().unwrap();
            let frame = scene.layout.content_frame(id).unwrap();
            div()
                .id(SharedString::from(id.to_owned()))
                .role(Role::Label)
                .aria_label(text.clone())
                .absolute()
                .left(px((frame.x - parent.x) as f32))
                .top(px((frame.y - parent.y) as f32))
                .w(px(frame.size.width as f32))
                .h(px(frame.size.height as f32))
        };
        let semantics = ["heading", "badge", "description", "name-label", "footer"]
            .map(|id| semantic(id, viewport));
        let nested_semantics =
            ["nested-title", "nested-action", "nested-tail"].map(|id| semantic(id, nested));
        let layout = Rc::new(PanelLayout {
            ui,
            scene,
            scrolls: [self.scroll.clone(), self.inner_scroll.clone()],
        });
        let origin = Rc::new(Cell::new(Point::default()));
        let paint_origin = origin.clone();
        let prepaint_layout = layout.clone();
        let painted_scroll = self.painted_scroll.clone();
        let drag_target = cx.entity().downgrade();
        let scroll_target = drag_target.clone();
        let scroll_layout = layout.clone();
        let canvas = canvas(
            move |bounds, _, _| {
                paint_origin.set(bounds.origin);
                // Sample in prepaint, so native scroll changes cannot leave painting a frame behind.
                let state = prepaint_layout.state().expect("finite native scroll state");
                let view = state
                    .resolve(&prepaint_layout.ui, &prepaint_layout.scene)
                    .expect("valid MUI view");
                painted_scroll.set((
                    -(view.item("viewport").unwrap().scroll.y as f32),
                    -(view.item("nested").unwrap().scroll.y as f32),
                ));
                view.items()
                    .map(|(id, item)| (id.to_owned(), item.clone()))
                    .collect::<std::collections::BTreeMap<_, _>>()
            },
            move |bounds, entries, window, cx| {
                let line_height = window.line_height();
                window.on_mouse_event(move |event: &ScrollWheelEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        match scroll_layout.wheel(event, bounds.origin, line_height) {
                            Ok(true) => {
                                let _ = scroll_target.update(cx, |_, cx| cx.notify());
                                cx.stop_propagation();
                            }
                            Ok(false) => {}
                            Err(error) => eprintln!("MUI wheel: {error:#}"),
                        }
                    }
                });

                window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                    if phase == DispatchPhase::Capture {
                        let _ = drag_target.update(cx, |view, cx| view.drag_move(event, cx));
                    }
                });
                for (id, contours, fill) in surfaces {
                    let entry = &entries[&id];
                    window.with_content_mask(Some(mask(entry, bounds.origin, bounds)), |window| {
                        let mut builder = PathBuilder::fill().with_style(PathStyle::Fill(
                            FillOptions::default().with_fill_rule(FillRule::NonZero),
                        ));
                        for ring in contours {
                            for (i, p) in ring.iter().enumerate() {
                                let p = entry.transform.apply(*p);
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
                    });
                }
                for (id, frame, lines) in paragraphs {
                    let entry = &entries[&id];
                    window.with_content_mask(Some(mask(entry, bounds.origin, bounds)), |window| {
                        let p = entry
                            .transform
                            .apply(geometry::Point::new(frame.x, frame.y));
                        let mut origin = bounds.origin + point(px(p.x as f32), px(p.y as f32));
                        let line_height = typography(&id).1;
                        for line in lines {
                            if let Err(e) =
                                line.paint(origin, line_height, TextAlign::Left, None, window, cx)
                            {
                                eprintln!("MUI text: {e}");
                            }
                            origin.y += line.size(line_height).height;
                        }
                    });
                }
            },
        )
        .absolute()
        .size_full();
        let press_layout = layout.clone();
        let press_origin = origin.clone();
        let gain = div()
            .id("gain")
            .absolute()
            .left(px((gain.x - viewport.x) as f32))
            .top(px((gain.y - viewport.y) as f32))
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
                    if press_layout.gain_hit(p) {
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
                    if let Some((_, initial, _)) = this.drag {
                        this.gain.cancel();
                        this.value = initial;
                        cx.notify();
                    }
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
                    if !layout.gain_hit(p) {
                        return;
                    }
                }
                this.toggle(cx);
            }));
        Ok(div()
            .id("mui-panel")
            .tab_group()
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
            .overflow_hidden()
            .bg(rgb(0x181c24))
            .child(canvas)
            .child(
                div()
                    .id("mui-scroll")
                    .absolute()
                    .left(px(viewport.x as f32))
                    .top(px(viewport.y as f32))
                    .w(px(viewport.size.width as f32))
                    .h(px(viewport.size.height as f32))
                    .track_scroll(&self.scroll)
                    .overflow_y_scroll()
                    .child(
                        div()
                            .relative()
                            .w_full()
                            .h(px(outer_height as f32))
                            .children(semantics)
                            .child(gain)
                            .child(
                                div()
                                    .absolute()
                                    .left(px((name.x - viewport.x) as f32))
                                    .top(px((name.y - viewport.y) as f32))
                                    .w(px(name.size.width as f32))
                                    .h(px(name.size.height as f32))
                                    .overflow_hidden()
                                    .child(self.name.clone()),
                            )
                            .child(
                                div()
                                    .id("nested-scroll")
                                    .absolute()
                                    .left(px((nested.x - viewport.x) as f32))
                                    .top(px((nested.y - viewport.y) as f32))
                                    .w(px(nested.size.width as f32))
                                    .h(px(nested.size.height as f32))
                                    .track_scroll(&self.inner_scroll)
                                    .overflow_y_scroll()
                                    .child(
                                        div()
                                            .relative()
                                            .w_full()
                                            .h(px(inner_height as f32))
                                            .children(nested_semantics),
                                    ),
                            ),
                    ),
            )
            .into_any_element())
    }
}
impl Render for ProbeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        match self.panel(window, cx) {
            Ok(panel) => {
                self.render_error = None;
                panel
            }
            Err(error) => {
                self.render_error = Some(format!("{error:#}"));
                div()
                    .text_color(rgb(0xff8080))
                    .child(format!("MUI panel: {error:#}"))
                    .into_any_element()
            }
        }
    }
}
