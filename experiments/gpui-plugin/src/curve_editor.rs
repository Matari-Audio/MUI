//! Plugin-neutral Bézier editor. The shared MUI model supplies both render segments and evaluation.
use gpui::{prelude::*, *};

pub use mui::curve::{Curve, CurvePoint, Handle};

#[derive(Clone, Debug)]
pub enum CurveEdit {
    Begin,
    Changed(Curve),
    End,
}
struct Drag {
    start: Point<Pixels>,
    initial: Curve,
}
/// No Truce types, host IDs, timers, or DSP work belong in this component.
pub struct CurveEditor {
    curve: Curve,
    history: Option<mui::curve::CurveHistory>,
    selected: usize,
    selected_handle: Option<(usize, Handle)>,
    compact_height: Option<f32>,
    transparent: bool,
    samples: usize,
    axis_labels: [SharedString; 2],
    drag: Option<Drag>,
    bounds: Bounds<Pixels>,
    focus: FocusHandle,
    enabled: bool,
    activation: Option<Subscription>,
}
impl EventEmitter<CurveEdit> for CurveEditor {}
impl CurveEditor {
    pub fn new(curve: Curve, cx: &mut Context<Self>) -> Self {
        crate::controls::init(cx);
        Self {
            curve,
            history: Some(Default::default()),
            selected: 1,
            selected_handle: None,
            compact_height: None,
            transparent: false,
            samples: 0,
            axis_labels: ["PHASE".into(), "LEVEL".into()],
            drag: None,
            bounds: Bounds::default(),
            focus: cx.focus_handle(),
            enabled: true,
            activation: None,
        }
    }
    /// Delegate undo/redo to an enclosing document instead of keeping standalone curve history.
    pub fn without_local_history(mut self) -> Self {
        self.history = None;
        self
    }
    fn remember(&mut self, before: &Curve) {
        if let Some(history) = &mut self.history {
            history.commit(before, &self.curve);
        }
    }
    pub fn undo(&mut self, cx: &mut Context<Self>) {
        self.restore_history(false, cx);
    }
    pub fn redo(&mut self, cx: &mut Context<Self>) {
        self.restore_history(true, cx);
    }
    fn restore_history(&mut self, redo: bool, cx: &mut Context<Self>) {
        if !self.enabled || self.drag.is_some() {
            return;
        }
        let Some(history) = &mut self.history else {
            return;
        };
        let restored = if redo {
            history.redo(&self.curve)
        } else {
            history.undo(&self.curve)
        };
        if let Some(curve) = restored {
            self.curve = curve;
            self.selected_handle = None;
            self.selected = self.selected.min(self.curve.points().len() - 1);
            cx.emit(CurveEdit::Begin);
            cx.emit(CurveEdit::Changed(self.curve.clone()));
            cx.emit(CurveEdit::End);
            cx.notify();
        }
    }
    /// Hide the readout rail when embedding the same graph inside another component.
    /// Layer handles over an owner-supplied response visualization.
    pub fn transparent(mut self) -> Self { self.transparent = true; self }
    pub fn compact(mut self, height: f32) -> Self {
        self.compact_height = Some(if height.is_finite() {
            height.clamp(48., 1024.)
        } else {
            140.
        });
        self
    }
    pub fn axis_labels(
        mut self,
        input: impl Into<SharedString>,
        output: impl Into<SharedString>,
    ) -> Self {
        self.axis_labels = [input.into(), output.into()];
        self
    }
    /// Optional response samples, e.g. one point for each unison voice. Zero hides them.
    pub fn set_samples(&mut self, count: usize, cx: &mut Context<Self>) {
        let count = count.min(64);
        if self.samples != count {
            self.samples = count;
            cx.notify();
        }
    }
    fn selected_point(&self, curve: &Curve) -> CurvePoint {
        match self.selected_handle {
            Some((i, Handle::Outgoing)) => curve.handles()[i].outgoing,
            Some((i, Handle::Incoming)) => curve.handles()[i].incoming,
            None => curve.points()[self.selected],
        }
    }
    fn move_selected(&mut self, phase: f32, value: f32) {
        if let Some((i, h)) = self.selected_handle {
            self.curve.move_handle(i, h, CurvePoint { phase, value });
        } else {
            self.curve.move_point(self.selected, phase, value);
        }
    }
    pub fn curve(&self) -> &Curve {
        &self.curve
    }
    /// External recall supersedes an active edit and never emits a Changed event back.
    pub fn replace(&mut self, curve: Curve, cx: &mut Context<Self>) {
        self.finish(cx);
        if let Some(history) = &mut self.history {
            history.clear();
        }
        self.curve = curve;
        self.selected_handle = None;
        self.selected = self.selected.min(self.curve.points().len() - 1);
        cx.notify();
    }
    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if enabled == self.enabled {
            return;
        }
        if !enabled {
            self.cancel(cx);
        }
        self.enabled = enabled;
        cx.notify();
    }
    fn finish(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.drag.take() {
            self.remember(&drag.initial);
            cx.emit(CurveEdit::End);
        }
    }
    fn cancel(&mut self, cx: &mut Context<Self>) {
        if let Some(drag) = self.drag.take() {
            if self.curve != drag.initial {
                self.curve = drag.initial;
                cx.emit(CurveEdit::Changed(self.curve.clone()));
            }
            cx.emit(CurveEdit::End);
            cx.notify();
        }
    }
    fn change(&mut self, before: Curve, cx: &mut Context<Self>) {
        if before != self.curve {
            cx.emit(CurveEdit::Changed(self.curve.clone()));
            cx.notify();
        }
    }
    fn screen(&self, p: CurvePoint) -> Point<Pixels> {
        point(
            self.bounds.left() + self.bounds.size.width * p.phase,
            self.bounds.top() + self.bounds.size.height * (1. - p.value),
        )
    }
    fn nearest(&self, position: Point<Pixels>) -> Option<usize> {
        self.curve
            .points()
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                let d = self.screen(*p) - position;
                let d = f32::from(d.x).hypot(f32::from(d.y));
                (d <= 10.).then_some((i, d))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|p| p.0)
    }
    fn nearest_handle(&self, position: Point<Pixels>) -> Option<(usize, Handle)> {
        self.curve
            .handles()
            .iter()
            .enumerate()
            .flat_map(|(i, h)| {
                [
                    (i, Handle::Outgoing, h.outgoing, i == self.selected),
                    (i, Handle::Incoming, h.incoming, i + 1 == self.selected),
                ]
            })
            .filter_map(|(i, h, p, visible)| {
                let d = self.screen(p) - position;
                let distance = f32::from(d.x).hypot(f32::from(d.y));
                (visible && distance <= 8.).then_some((i, h, distance))
            })
            .min_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(i, h, _)| (i, h))
    }
    fn down(&mut self, e: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.enabled || self.bounds.size.width <= px(0.) || self.bounds.size.height <= px(0.) {
            return;
        }
        window.focus(&self.focus, cx);
        self.finish(cx);
        let mut hit = self.nearest(e.position);
        let mut handle = self.nearest_handle(e.position);
        // In compact graphs the hit radii overlap: choose the closest control, not always the knot.
        if let (Some(node), Some((segment, side))) = (hit, handle) {
            let h = self.curve.handles()[segment];
            let h = match side {
                Handle::Outgoing => h.outgoing,
                Handle::Incoming => h.incoming,
            };
            let distance = |p| {
                let d = self.screen(p) - e.position;
                f32::from(d.x).hypot(f32::from(d.y))
            };
            if distance(h) < distance(self.curve.points()[node]) {
                hit = None;
            } else {
                handle = None;
            }
        }
        if e.click_count == 2 {
            let before = self.curve.clone();
            if let Some((i, _)) = handle {
                self.curve.reset_segment(i);
            } else if let Some(i) = hit {
                self.selected_handle = None;
                self.curve.remove(i);
                self.selected = self.selected.min(self.curve.points().len() - 1);
            } else {
                let p = CurvePoint {
                    phase: f32::from(e.position.x - self.bounds.left())
                        / f32::from(self.bounds.size.width),
                    value: 1.
                        - f32::from(e.position.y - self.bounds.top())
                            / f32::from(self.bounds.size.height),
                };
                let inserted = if e.modifiers.alt {
                    self.curve.split(p.phase)
                } else {
                    self.curve.insert(p)
                };
                if let Some(i) = inserted {
                    self.selected = i;
                    self.selected_handle = None;
                }
            }
            if before != self.curve {
                self.remember(&before);
                cx.emit(CurveEdit::Begin);
                self.change(before, cx);
                cx.emit(CurveEdit::End);
            }
        } else if hit.is_some() || handle.is_some() {
            if let Some(index) = hit {
                self.selected = index;
            }
            self.selected_handle = handle;
            self.drag = Some(Drag {
                start: e.position,
                initial: self.curve.clone(),
            });
            cx.emit(CurveEdit::Begin);
            cx.notify();
        }
    }
    fn movement(&mut self, e: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(drag) = &self.drag else {
            return;
        };
        if !e.dragging() {
            self.finish(cx);
            return;
        }
        let origin = self.selected_point(&drag.initial);
        let fine = if e.modifiers.shift { 0.1 } else { 1. };
        let mut phase = origin.phase
            + f32::from(e.position.x - drag.start.x) / f32::from(self.bounds.size.width) * fine;
        let mut value = origin.value
            - f32::from(e.position.y - drag.start.y) / f32::from(self.bounds.size.height) * fine;
        if !e.modifiers.alt && !e.modifiers.shift {
            for v in [&mut phase, &mut value] {
                let snap = (*v * 8.).round() / 8.;
                if (*v - snap).abs() < 0.015 {
                    *v = snap;
                }
            }
        }
        let before = self.curve.clone();
        self.curve = drag.initial.clone();
        self.move_selected(phase, value);
        self.change(before, cx);
    }
}
impl Render for CurveEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.activation.is_none() {
            self.activation = Some(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.cancel(cx);
                }
            }));
        }
        let entity = cx.entity();
        let points = self.curve.points().to_vec();
        let selected = self.selected;
        let selected_handle = self.selected_handle;
        let curve = self.curve.clone();
        let samples = self.samples;
        let enabled = self.enabled;
        let plot = canvas(
            move |bounds, _, cx| {
                entity.update(cx, |this, _| this.bounds = bounds);
            },
            move |bounds, _, window, _| {
                let color = crate::live_theme::color(if enabled { 0xbadc91 } else { 0x9ba697 });
                let pos = |p: CurvePoint| {
                    point(
                        bounds.left() + bounds.size.width * p.phase,
                        bounds.top() + bounds.size.height * (1. - p.value),
                    )
                };
                let mut grid = PathBuilder::stroke(px(1.));
                for n in 0..=4 {
                    let t = n as f32 / 4.;
                    grid.move_to(pos(CurvePoint {
                        phase: t,
                        value: 0.,
                    }));
                    grid.line_to(pos(CurvePoint {
                        phase: t,
                        value: 1.,
                    }));
                    grid.move_to(pos(CurvePoint {
                        phase: 0.,
                        value: t,
                    }));
                    grid.line_to(pos(CurvePoint {
                        phase: 1.,
                        value: t,
                    }));
                }
                if let Ok(path) = grid.build() {
                    window.paint_path(path, crate::live_theme::color(0x343f32));
                }
                let mut line = PathBuilder::stroke(px(1.5));
                line.move_to(pos(points[0]));
                for i in 0..curve.handles().len() {
                    let segment = curve.segment(i).unwrap();
                    line.cubic_bezier_to(
                        pos(segment.end),
                        pos(segment.handles.outgoing),
                        pos(segment.handles.incoming),
                    );
                }
                if let Ok(path) = line.build() {
                    window.paint_path(path, color);
                }
                for (i, h) in curve.handles().iter().enumerate() {
                    for (handle, p, anchor, visible) in [
                        (Handle::Outgoing, h.outgoing, points[i], selected == i),
                        (
                            Handle::Incoming,
                            h.incoming,
                            points[i + 1],
                            selected == i + 1,
                        ),
                    ] {
                        if !visible {
                            continue;
                        }
                        let mut stem = PathBuilder::stroke(px(1.));
                        stem.move_to(pos(anchor));
                        stem.line_to(pos(p));
                        if let Ok(path) = stem.build() {
                            window.paint_path(path, crate::live_theme::color(0x718164));
                        }
                        let r = if selected_handle == Some((i, handle)) {
                            4.
                        } else {
                            3.
                        };
                        window.paint_quad(
                            outline(
                                Bounds::new(
                                    pos(p) - point(px(r), px(r)),
                                    size(px(r * 2.), px(r * 2.)),
                                ),
                                color,
                                BorderStyle::Solid,
                            )
                            .corner_radii(px(1.)),
                        );
                    }
                }
                let mut values = [0.; 64];
                curve.sample_into(&mut values[..samples]);
                for (i, value) in values[..samples].iter().enumerate() {
                    let phase = if samples == 1 {
                        0.5
                    } else {
                        i as f32 / (samples - 1) as f32
                    };
                    let p = pos(CurvePoint {
                        phase,
                        value: *value,
                    });
                    let mut stem = PathBuilder::stroke(px(1.));
                    stem.move_to(pos(CurvePoint { phase, value: 0.5 }));
                    stem.line_to(p);
                    if let Ok(path) = stem.build() {
                        window.paint_path(path, crate::live_theme::color(0x718164));
                    }
                    window.paint_quad(
                        fill(
                            Bounds::new(p - point(px(2.), px(2.)), size(px(4.), px(4.))),
                            color,
                        )
                        .corner_radii(px(2.)),
                    );
                }
                for (i, p) in points.iter().enumerate() {
                    let r = if i == selected { 4.5 } else { 3. };
                    window.paint_quad(
                        fill(
                            Bounds::new(
                                pos(*p) - point(px(r), px(r)),
                                size(px(r * 2.), px(r * 2.)),
                            ),
                            color,
                        )
                        .corner_radii(px(r)),
                    );
                }
            },
        )
        .size_full();
        let point = self.selected_point(&self.curve);
        div().id("curve-editor").key_context("MUI").track_focus(&self.focus).tab_index(0)
            .role(Role::Group).aria_label("Bézier editor. Brackets select a point, H selects its handles. Arrows move selection, Shift moves finely, Delete removes a point or resets handles, Escape cancels a drag. Control-Z undoes; Control-Shift-Z redoes.")
            .flex().w_full().h(px(self.compact_height.unwrap_or(164.))).mt(px(if self.compact_height.is_some() { 0. } else { 12. })).gap(px(10.))
            .child(div().id("curve-plot").relative().flex_1().min_w(px(0.)).h_full().p(px(10.))
                .when(!self.transparent, |el| el.bg(crate::live_theme::color(0x1b211a))).rounded(px(6.)).cursor(CursorStyle::Crosshair).child(plot)
                .on_mouse_down(MouseButton::Left, cx.listener(Self::down)))
            .when(self.compact_height.is_none(), |el| el.child(div().flex().flex_col().w(px(62.)).gap(px(8.))
                .child(crate::kurv::ink(if self.selected_handle.is_some() { "HANDLE" } else { "POINT" }, 9., 0x9ba697))
                .child(crate::kurv::ink(&format!("{}/{}", self.selected+1, self.curve.points().len()), 14., 0xe0e5da))
                .child(crate::kurv::ink(&self.axis_labels[0], 9., 0x9ba697))
                .child(crate::kurv::ink(&format!("{:.1}%", point.phase*100.), 13., 0xe0e5da))
                .child(crate::kurv::ink(&self.axis_labels[1], 9., 0x9ba697))
                .child(crate::kurv::ink(&format!("{:.1}%", point.value*100.), 13., 0xe0e5da))))
            .on_mouse_move(cx.listener(Self::movement))
            .on_mouse_up(MouseButton::Left, cx.listener(|this, _, _, cx| this.finish(cx)))
            .on_mouse_up_out(MouseButton::Left, cx.listener(|this, _, _, cx| this.finish(cx)))
            .on_action(cx.listener(|this, _: &crate::controls::Cancel, _, cx| { this.cancel(cx); }))
            .on_key_down(cx.listener(|this, e: &KeyDownEvent, _, cx| {
                if !this.enabled || this.drag.is_some() { return; }
                let modifiers = &e.keystroke.modifiers;
                if this.history.is_some() && (modifiers.control || modifiers.platform) && !modifiers.alt {
                    match e.keystroke.key.as_str() {
                        "z" if modifiers.shift => this.redo(cx),
                        "z" => this.undo(cx),
                        "y" => this.redo(cx),
                        _ => return,
                    }
                    cx.stop_propagation();
                    return;
                }
                let before = this.curve.clone();
                let p = this.selected_point(&this.curve);
                let step = if e.keystroke.modifiers.shift { 0.001 } else { 0.01 };
                match e.keystroke.key.as_str() {
                    "left" => this.move_selected(p.phase-step, p.value),
                    "right" => this.move_selected(p.phase+step, p.value),
                    "up" => this.move_selected(p.phase, p.value+step),
                    "down" => this.move_selected(p.phase, p.value-step),
                    "backspace" | "delete" => { if let Some((i,_)) = this.selected_handle { this.curve.reset_segment(i); } else { this.curve.remove(this.selected); } this.selected_handle = None; this.selected = this.selected.min(this.curve.points().len()-1); },
                    "[" => { this.selected_handle = None; this.selected = this.selected.saturating_sub(1); cx.notify(); },
                    "]" => { this.selected_handle = None; this.selected = (this.selected+1).min(this.curve.points().len()-1); cx.notify(); },
                    "h" => { this.selected_handle = match this.selected_handle {
                        None if this.selected < this.curve.handles().len() => Some((this.selected, Handle::Outgoing)),
                        Some((_, Handle::Outgoing)) | None if this.selected > 0 => Some((this.selected-1, Handle::Incoming)),
                        _ => None,
                    }; cx.notify(); },
                    _ => return,
                }
                if before != this.curve { this.remember(&before); cx.emit(CurveEdit::Begin); this.change(before, cx); cx.emit(CurveEdit::End); }
                cx.stop_propagation();
            }))
    }
}
/// Runs against the laid-out GPUI component in the existing scale/input matrix.
pub(crate) fn check_interactions(
    editor: &Entity<CurveEditor>,
    window: &mut Window,
    cx: &mut App,
    done: impl FnOnce(&mut Window, &mut App) + 'static,
) {
    use std::{cell::RefCell, rc::Rc};
    let events = Rc::new(RefCell::new(Vec::new()));
    let output = events.clone();
    let _subscription = cx.subscribe(editor, move |_, event: &CurveEdit, _| {
        output.borrow_mut().push(event.clone())
    });
    let original = editor.read(cx).curve.clone();
    let from = editor.read(cx).screen(original.points()[1]);
    assert!(
        editor.read(cx).bounds.size.width > px(20.),
        "curve has usable layout"
    );
    let to = from + point(px(-12.), px(24.));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: from,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: from,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: to,
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        }),
        cx,
    );
    assert_ne!(editor.read(cx).curve, original, "native drag updates curve");
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: to,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    let changed = editor.read(cx).curve.clone();
    for (key, expected) in [("ctrl-z", &original), ("ctrl-shift-z", &changed)] {
        let keystroke = Keystroke::parse(key).unwrap();
        window.dispatch_event(
            PlatformInput::KeyDown(KeyDownEvent {
                keystroke: keystroke.clone(),
                is_held: false,
                prefer_character_input: false,
            }),
            cx,
        );
        window.dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke }), cx);
        assert_eq!(
            &editor.read(cx).curve,
            expected,
            "native {key} restores the whole gesture"
        );
    }
    // Recalling the original closes a gesture without publishing the recalled value as a user edit.
    editor.update(cx, |editor, cx| editor.replace(original.clone(), cx));
    editor.update(cx, |editor, cx| {
        editor.down(
            &MouseDownEvent {
                position: from,
                button: MouseButton::Left,
                click_count: 1,
                ..Default::default()
            },
            window,
            cx,
        );
        editor.movement(
            &MouseMoveEvent {
                position: to,
                pressed_button: Some(MouseButton::Left),
                ..Default::default()
            },
            window,
            cx,
        );
        editor.cancel(cx);
        editor.undo(cx); // A cancelled drag must not leave an undo entry.
        assert_eq!(editor.curve, original, "cancel restores the entire curve");
        editor.set_enabled(false, cx);
        editor.down(
            &MouseDownEvent {
                position: from,
                button: MouseButton::Left,
                click_count: 1,
                ..Default::default()
            },
            window,
            cx,
        );
        assert!(editor.drag.is_none(), "disabled graphs cannot edit");
        editor.set_enabled(true, cx);
    });
    assert_ne!(original, changed);
    let handle_point = editor.read(cx).screen(original.handles()[0].incoming);
    let handle_to = handle_point + point(px(0.), px(22.));
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: handle_point,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position: handle_point,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position: handle_to,
            pressed_button: Some(MouseButton::Left),
            ..Default::default()
        }),
        cx,
    );
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position: handle_to,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
    assert_ne!(
        editor.read(cx).curve.handles(),
        original.handles(),
        "native handle drag bends the cubic"
    );
    assert_eq!(
        editor.read(cx).curve.points(),
        original.points(),
        "handle editing preserves anchors"
    );
    editor.update(cx, |editor, cx| editor.replace(original.clone(), cx));
    editor.update(cx, |editor, cx| editor.undo(cx));
    assert_eq!(
        editor.read(cx).curve,
        original,
        "recall clears obsolete history"
    );
    window.on_next_frame(move |window, cx| {
        let _subscription = _subscription;
        let events = events.borrow();
        assert!(matches!(events.first(), Some(CurveEdit::Begin)));
        assert!(matches!(events.last(), Some(CurveEdit::End)));
        assert_eq!(events.iter().filter(|e| matches!(e, CurveEdit::Begin)).count(), 5);
        assert_eq!(events.iter().filter(|e| matches!(e, CurveEdit::End)).count(), 5);
        assert_eq!(events.iter().filter(|e| matches!(e, CurveEdit::Changed(_))).count(), 6,
            "point drag, second drag, cancel and handle drag; external recall must not echo");
        eprintln!("PASS: Bézier anchor/handle drag, native undo/redo, balanced owner events, recall without feedback, cancellation, disabled editing");
        done(window, cx);
    });
    window.refresh();
}
