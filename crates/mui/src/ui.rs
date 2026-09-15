//! The per-frame runtime: gestures in, animated styles applied, scene out.
use std::collections::BTreeMap;
use std::sync::Arc;

use mui_core::prelude::{overlay, text, Role, Styled as _};
use mui_core::{
    Color, Cursor, El, Element, Fill, Paint, Palette, Radius, ResolvedScene, SceneError, SceneSpec,
    Size, Spacing, Spring, TextCache, Theme,
};
use mui_geometry::Point;
use mui_input::{Hit, Input, Interaction, Key, KeyPress, PointerInput, Response};
use mui_layout::SpacingToken::S;

/// How long the pointer must rest on a surface before its tip is due.
pub const TIP_DELAY: f64 = 0.5;
/// Two presses on one target within this are a double click.
pub const DOUBLE_CLICK: f64 = 0.4;

/// What one call to [`Ui::frame`] produced.
pub struct Frame<'a> {
    pub scene: &'a ResolvedScene,
    /// A spring is still moving: schedule another frame.
    pub animating: bool,
    /// A tip that came due this frame, and where to put it. It is already
    /// floated into the scene; this is for a host that would rather place its
    /// own (a native tooltip window, say).
    pub tip: Option<(String, Point)>,
    /// The cursor the hovered surface asks for.
    pub cursor: Cursor,
    /// Every gesture that began or ended this frame, for a host that brackets
    /// automation. [`Ui::edit`] asks about one id.
    pub edits: Vec<(String, Edit)>,
    /// A copy or cut asked for this: put it on the host's clipboard.
    pub clipboard: Option<String>,
}

/// A parameter gesture's two edges. A slider or knob drag is one `Begin`, a
/// run of value changes, and one `End`: exactly the bracket a plugin host
/// wants around touched automation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edit {
    Begin,
    End,
}

/// Retained state for an immediate tree. Build the tree every frame; the
/// runtime remembers what is hovered, held, and mid-animation.
pub struct Ui {
    pub theme: Theme,
    pub font: Option<Arc<Vec<u8>>>,
    interaction: Interaction,
    hit: Hit,
    scene: Option<ResolvedScene>,
    /// Per key: hover and press springs, 0..1.
    springs: BTreeMap<String, [Spring; 2]>,
    /// Transition springs per node id, one per paint channel, and tweens
    /// under a `~` prefix so a widget id cannot collide with one.
    // ponytail: never pruned -- bounded by the ids an app ever uses; retain
    // against the keys a frame touched if a generated-id list grows.
    motion: BTreeMap<String, Vec<Spring>>,
    text_cache: TextCache,
    /// Per scroll node: how far its children are slid.
    scrolls: BTreeMap<String, [f64; 2]>,
    /// Per text field: the selection's anchor and caret, in characters. They
    /// are equal when nothing is selected.
    sel: BTreeMap<String, (usize, usize)>,
    /// The host's clipboard, handed in with a paste key; and what a copy or
    /// cut asked to put back on it.
    pasted: Option<String>,
    copied: Option<String>,
    /// A press that landed on the same target within [`DOUBLE_CLICK`].
    double: Option<String>,
    last_press: Option<(String, f64)>,
    focus: Option<String>,
    edits: Vec<(String, Edit)>,
    /// An `End` owed because the gesture was cancelled, not released.
    cancelled: Option<String>,
    keys: Vec<KeyPress>,
    typed: String,
    pointer: PointerInput,
    /// The hovered key and how long it has been hovered.
    hover: Option<(String, f64)>,
    time: f64,
}
impl Ui {
    pub fn new(theme: Theme) -> Self {
        Self {
            theme,
            font: None,
            interaction: Interaction::new(),
            hit: Hit::default(),
            scene: None,
            springs: BTreeMap::new(),
            motion: BTreeMap::new(),
            text_cache: TextCache::default(),
            scrolls: BTreeMap::new(),
            sel: BTreeMap::new(),
            pasted: None,
            copied: None,
            double: None,
            last_press: None,
            focus: None,
            edits: Vec::new(),
            cancelled: None,
            keys: Vec::new(),
            typed: String::new(),
            pointer: PointerInput::default(),
            hover: None,
            time: 0.0,
        }
    }
    pub fn font(mut self, font: impl Into<Arc<Vec<u8>>>) -> Self {
        self.font = Some(font.into());
        self
    }
    /// What the last frame resolved to, for anything drawn on top of it.
    pub fn scene(&self) -> Option<&ResolvedScene> {
        self.scene.as_ref()
    }
    /// Drop the gesture in flight, for focus loss. The held target still
    /// gets its [`Edit::End`] on the next frame: a host that was told a
    /// gesture began must be told it ended.
    pub fn cancel(&mut self) {
        self.cancelled = self.interaction.held().map(str::to_owned);
        self.interaction.cancel();
    }

    /// Whether a parameter gesture on `id` began or ended, on the same frame
    /// boundary as [`Ui::get`]. Bracket automation with it:
    ///
    /// ```
    /// # use mui::{Edit, Ui}; use mui::prelude::*;
    /// # let mut ui = Ui::new(Theme::DEFAULT);
    /// # let mut cutoff = 0.5;
    /// match ui.edit("cutoff") {
    ///     Some(Edit::Begin) => { /* host.begin_gesture(CUTOFF) */ }
    ///     Some(Edit::End) => { /* host.end_gesture(CUTOFF) */ }
    ///     None => {}
    /// }
    /// let el = slider(&mut ui, "cutoff", "Cutoff", &mut cutoff, 0.0..=1.0);
    /// ```
    pub fn edit(&self, id: &str) -> Option<Edit> {
        self.edits.iter().find(|(k, _)| k == id).map(|(_, e)| *e)
    }

    /// A keyed spring anyone can read while building the tree: pass the value
    /// you want, get the value to draw. A knob's sweep drawn from
    /// `ui.tween("cutoff", v)` glides when a preset changes it and still
    /// tracks a drag, because the spring is retargeted, never restarted.
    /// First call returns `target`, so nothing flies in from zero.
    pub fn tween(&mut self, id: &str, target: f64) -> f64 {
        self.tween_with(id, target, Spring::DEFAULT)
    }
    /// [`Ui::tween`] with your own spring. The spring's shape is taken on
    /// the first call for `id`.
    pub fn tween_with(&mut self, id: &str, target: f64, spring: Spring) -> f64 {
        let s = self
            .motion
            .entry(format!("~{id}"))
            .or_insert_with(|| vec![seed(spring, target)]);
        let s = &mut s[0];
        s.to(target);
        s.value
    }
    /// Last frame's gesture on `id`. Widgets read this while building the
    /// next tree, so a drag lands one frame late and nobody notices.
    pub fn get(&self, id: &str) -> Response {
        self.interaction.get(id)
    }
    /// Hover and press amounts for `id`, 0..1 and spring-smoothed.
    pub fn state(&self, id: &str) -> (f64, f64) {
        self.springs
            .get(id)
            .map_or((0.0, 0.0), |[h, p]| (h.value, p.value))
    }
    /// Whether `id` holds the keyboard focus.
    pub fn focused(&self, id: &str) -> bool {
        self.focus.as_deref() == Some(id)
    }
    /// Focus `id` from code. No check that it exists: it may not have been
    /// built yet.
    pub fn focus(&mut self, id: impl Into<String>) {
        self.focus = Some(id.into());
    }
    /// The keys this frame, if `id` is focused. Empty otherwise, so a widget
    /// may loop over it unconditionally.
    pub fn keys(&self, id: &str) -> &[KeyPress] {
        if self.focused(id) {
            &self.keys
        } else {
            &[]
        }
    }
    /// The text typed this frame, if `id` is focused.
    pub fn text(&self, id: &str) -> &str {
        if self.focused(id) {
            &self.typed
        } else {
            ""
        }
    }
    /// The pointer relative to `id`'s frame origin, if both exist.
    pub fn local(&self, id: &str) -> Option<Point> {
        let p = self.pointer.pos?;
        let s = self.scene.as_ref()?.surface(id)?;
        Some(Point::new(p.x - s.frame.x, p.y - s.frame.y))
    }
    /// Source and target of a drag released this frame.
    pub fn dropped(&self) -> Option<(&str, &str)> {
        self.interaction.dropped()
    }
    /// How far `id`'s children are scrolled.
    pub fn scroll(&self, id: &str) -> [f64; 2] {
        self.scrolls.get(id).copied().unwrap_or([0.0, 0.0])
    }

    pub(crate) fn sel(&self, id: &str) -> (usize, usize) {
        self.sel.get(id).copied().unwrap_or((0, 0))
    }
    pub(crate) fn set_sel(&mut self, id: &str, anchor: usize, caret: usize) {
        self.sel.insert(id.to_owned(), (anchor, caret));
    }
    /// The clipboard the host handed in because a paste key arrived.
    pub(crate) fn pasted(&self) -> Option<&str> {
        self.pasted.as_deref()
    }
    /// Whether the last press on `id` was the second of a double click.
    pub(crate) fn double_click(&self, id: &str) -> bool {
        self.double.as_deref() == Some(id)
    }
    /// Ask the host to put `s` on the clipboard: it comes back on the next
    /// frame's [`Frame::clipboard`].
    pub fn set_clipboard(&mut self, s: impl Into<String>) {
        self.copied = Some(s.into());
    }
    /// The character index in `s` nearest `x`, measured in the scene's font.
    pub(crate) fn hit(&self, s: &str, size: f64, x: f64) -> usize {
        match self.font.as_deref() {
            Some(f) => mui_text::hit_index(f, s, size, x)
                .map_or(0, |b| s[..b.min(s.len())].chars().count()),
            // ponytail: the same 0.6em guess `advance` falls back to.
            None => ((x / (size * 0.6)).round().max(0.0) as usize).min(s.chars().count()),
        }
    }
    /// Where the caret sits when it is `byte` bytes into `s`: the inverse of
    /// [`Ui::hit`], and the advance of the whole string when `byte == s.len()`.
    /// Measured, not shaped -- `mui_text::caret_x` reads advances only, where
    /// `text_run` would build every outline to throw them away.
    pub(crate) fn caret_x(&self, s: &str, size: f64, byte: usize) -> f64 {
        match self.font.as_deref() {
            Some(f) => mui_text::caret_x(f, s, size, byte).unwrap_or(0.0),
            // ponytail: the 0.6em guess the scene itself falls back to
            // without a font; set a font and both agree.
            None => s[..byte.min(s.len())].chars().count() as f64 * size * 0.6,
        }
    }
    /// A caret is on for 0.625 s of every 1.25 s.
    pub(crate) fn blink(&self) -> bool {
        (self.time * 1.6) as i64 % 2 == 0
    }

    /// Move the focus to the next (or previous) focusable surface in z-order,
    /// wrapping. Nothing focused yet starts at either end.
    fn cycle_focus(&mut self, back: bool) {
        let Some(scene) = self.scene.as_ref() else {
            return;
        };
        let stops: Vec<String> = scene
            .keys
            .iter()
            .filter(|k| scene.surface(k).is_some_and(|s| s.focusable))
            .cloned()
            .collect();
        if stops.is_empty() {
            return;
        }
        let at = self
            .focus
            .as_ref()
            .and_then(|c| stops.iter().position(|k| k == c));
        let next = match (at, back) {
            (Some(i), false) => (i + 1) % stops.len(),
            (Some(i), true) => (i + stops.len() - 1) % stops.len(),
            (None, false) => 0,
            (None, true) => stops.len() - 1,
        };
        self.focus = Some(stops[next].clone());
    }

    /// Apply a horizontal or vertical drag on `id` to `value` across `range`,
    /// `px` pixels for the full span. Returns whether it changed.
    pub fn drag(
        &self,
        id: &str,
        value: &mut f64,
        range: std::ops::RangeInclusive<f64>,
        px: f64,
        vertical: bool,
    ) -> bool {
        let r = self.get(id);
        if !r.dragged || px <= 0.0 {
            return false;
        }
        let d = if vertical {
            -r.drag_delta.y
        } else {
            r.drag_delta.x
        };
        let next =
            (*value + d / px * (range.end() - range.start())).clamp(*range.start(), *range.end());
        let changed = next != *value;
        *value = next;
        changed
    }

    /// Advance gestures and springs, style the tree by state, resolve it.
    pub fn frame(
        &mut self,
        root: El,
        offered: Option<Size>,
        input: impl Into<Input>,
        dt: f64,
    ) -> Result<Frame<'_>, SceneError> {
        let input = input.into();
        self.pointer = input.pointer;
        self.pasted = input.clipboard;
        self.time += dt;
        let prev_held = self.interaction.held().map(str::to_owned);
        self.interaction.update(&self.hit, input.pointer);
        let (hovered, held) = (
            self.interaction.hovered().map(str::to_owned),
            self.interaction.held().map(str::to_owned),
        );
        // A gesture is exactly the span a target is captured for, so the two
        // edges are the two ends of that capture -- plus the one a `cancel`
        // stole before this frame could see it.
        self.edits = self
            .cancelled
            .take()
            .map(|k| (k, Edit::End))
            .into_iter()
            .collect();
        if prev_held != held {
            self.edits.extend(prev_held.clone().map(|k| (k, Edit::End)));
            self.edits.extend(held.clone().map(|k| (k, Edit::Begin)));
        }
        for (k, [h, p]) in &mut self.springs {
            h.to(f64::from(
                hovered.as_deref() == Some(k) || held.as_deref() == Some(k),
            ));
            p.to(f64::from(held.as_deref() == Some(k)));
        }
        for k in [hovered, held].into_iter().flatten() {
            self.springs
                .entry(k)
                .or_insert_with(|| [Spring::at(0.0), Spring::at(0.0)])
                .iter_mut()
                .for_each(|s| s.to(1.0));
        }
        // A release is read by the *next* tree, so that frame must come even
        // when nothing is moving.
        let mut animating = prev_held.is_some();
        for s in self.springs.values_mut().flatten() {
            animating |= s.step(dt);
        }
        self.springs
            .retain(|_, [h, p]| h.value > 0.0 || p.value > 0.0 || !h.settled() || !p.settled());

        // Focus follows a press on a focusable surface, and a press on
        // anything else drops it.
        self.double = None;
        if let Some(id) = self.interaction.pressed().map(str::to_owned) {
            if let Some((prev, t)) = self.last_press.take() {
                if prev == id && self.time - t < DOUBLE_CLICK {
                    self.double = Some(id.clone());
                }
            }
            self.last_press = Some((id.clone(), self.time));
            let keeps = self
                .scene
                .as_ref()
                .and_then(|s| s.surface(&id))
                .is_some_and(|s| s.focusable);
            self.focus = keeps.then_some(id);
        }
        for k in &input.keys {
            match k.key {
                Key::Escape => self.focus = None,
                Key::Tab => self.cycle_focus(k.mods.shift),
                _ => {}
            }
        }
        self.keys = input.keys;
        self.typed = input.text;

        // A tip is due after the pointer has rested. Both the hover and the
        // surface come from last frame's scene, which is the one the pointer
        // was actually over.
        let hovered = self.interaction.hovered().map(str::to_owned);
        match (&mut self.hover, &hovered) {
            (Some((id, t)), Some(h)) if id == h => *t += dt,
            (_, Some(h)) => self.hover = Some((h.clone(), 0.0)),
            (_, None) => self.hover = None,
        }
        let tip = self
            .hover
            .as_ref()
            .filter(|(_, t)| *t >= TIP_DELAY)
            .and_then(|(id, _)| {
                let s = self.scene.as_ref()?.surface(id)?;
                Some((
                    s.tip.clone()?,
                    Point::new(s.frame.x, s.frame.bottom() + 4.0),
                ))
            });
        let mut root = match &tip {
            Some((t, at)) => {
                let float = text(t.clone())
                    .pad(S)
                    .fill(Role::Raised)
                    .radius(6.0)
                    .float()
                    .offset(at.x, at.y);
                // A leaf root has nowhere to push, so that one case still
                // rides a wrapper.
                if root.is_container() {
                    root.push(float)
                } else {
                    overlay([root, float])
                }
            }
            None => root,
        };

        let pal = self.theme.palette;
        animating |= transitions(&mut root, &pal, &mut self.motion, dt);
        for (_, s) in self.motion.iter_mut().filter(|(k, _)| k.starts_with('~')) {
            animating |= s[0].step(dt);
        }
        let springs = &self.springs;
        let scrolls = &self.scrolls;
        state(
            &mut root,
            &pal,
            &|k| springs.get(k).map(|[h, p]| (h.value, p.value)),
            scrolls,
        );

        let mut spec = SceneSpec::new(root).theme(self.theme);
        spec.offered = offered;
        spec.font = self.font.clone();
        let scene = mui_core::resolve_scene_with(&spec, &mut self.text_cache)?;
        // Named nodes are the gesture targets, in z-order. Unnamed ones are
        // decoration. A target clipped away does not respond.
        let mut hit = Hit::default();
        for k in scene.keys.iter().filter(|k| !k.starts_with('/')) {
            if let Some(s) = scene.surface(k) {
                hit.push_clipped(k.clone(), &s.path, s.clip)?;
            }
        }
        self.hit = hit;
        self.wheel(&scene, input.wheel);

        let held = self.interaction.held().map(str::to_owned);
        let cursor = held
            .clone()
            .or(hovered)
            .and_then(|k| scene.surface(&k).and_then(|s| s.cursor))
            .unwrap_or(Cursor::Arrow);
        let cursor = match cursor {
            Cursor::Grab if held.is_some() => Cursor::Grabbing,
            c => c,
        };

        self.scene = Some(scene);
        Ok(Frame {
            scene: self.scene.as_ref().expect("just set"),
            animating,
            tip,
            cursor,
            edits: self.edits.clone(),
            clipboard: self.copied.take(),
        })
    }

    /// Send the wheel to the innermost scrollable surface under the pointer.
    /// It lands on the next frame's tree, the same frame late a release is.
    fn wheel(&mut self, scene: &ResolvedScene, wheel: Point) {
        if wheel.x == 0.0 && wheel.y == 0.0 {
            return;
        }
        let Some(p) = self.pointer.pos else { return };
        for k in scene.keys.iter().rev() {
            let Some(s) = scene.surface(k) else { continue };
            let f = s.frame;
            if p.x < f.x || p.x > f.right() || p.y < f.y || p.y > f.bottom() {
                continue;
            }
            // `content` is the frame size for everything but a scroll node,
            // so an overflow here *is* the "is this scrollable" test.
            let max = [
                (s.content.width - f.size.width).max(0.0),
                (s.content.height - f.size.height).max(0.0),
            ];
            if max[0] <= 0.0 && max[1] <= 0.0 {
                continue;
            }
            let at = self.scrolls.entry(k.clone()).or_insert([0.0, 0.0]);
            at[0] = (at[0] + wheel.x).clamp(0.0, max[0]);
            at[1] = (at[1] + wheel.y).clamp(0.0, max[1]);
            return;
        }
    }
}

/// A spring shaped like `s`, resting at `value`.
fn seed(s: Spring, value: f64) -> Spring {
    Spring {
        value,
        velocity: 0.0,
        target: value,
        ..s
    }
}

/// Every numeric paint channel of `e`, in a fixed order, replaced by
/// `ch(index, declared)`. Sizes and layout are deliberately absent.
fn channels(e: &mut Element, pal: &Palette, ch: &mut impl FnMut(usize, f64) -> f64) {
    if let Some(Paint::Solid(c)) = e.style.fill.paint(pal, pal.background()) {
        let v = [c.lightness(), c.chroma(), c.hue(), c.alpha()];
        let o = std::array::from_fn::<f32, 4, _>(|i| ch(i, f64::from(v[i])) as f32);
        e.style.fill = Fill::Color(Color::oklcha(o[0], o[1], o[2], o[3]));
    }
    if let Some(w) = e.style.stroke.as_mut().and_then(|s| s.width.as_mut()) {
        *w = ch(4, *w);
    }
    if let Radius::Px(r) = &mut e.style.radius {
        *r = ch(5, *r);
    }
    if let Some(t) = e.text_size.as_mut() {
        *t = ch(6, *t);
    }
    if let Some(s) = e.style.shadow.as_mut() {
        s.blur = ch(7, s.blur);
    }
    for (i, (d, _)) in e.style.shells.iter_mut().enumerate() {
        if let Spacing::Px(v) = d {
            *v = ch(8 + i, *v);
        }
    }
}

/// Spring every transitioning node's paint toward what it declared this
/// frame. The springs hold last frame's declaration as their target, so a new
/// target mid-flight retargets the live spring instead of restarting it.
fn transitions(
    n: &mut El,
    pal: &Palette,
    motion: &mut BTreeMap<String, Vec<Spring>>,
    dt: f64,
) -> bool {
    let mut animating = false;
    if let (Some(k), Some(spring)) = (n.key().map(str::to_owned), n.payload().transition) {
        let list = motion.entry(k).or_default();
        channels(n.payload_mut(), pal, &mut |i, declared| {
            if list.len() <= i {
                list.resize(i + 1, seed(spring, declared));
            }
            let s = &mut list[i];
            // Hue is an angle: take the short way round rather than
            // sweeping 350 degrees back to 10.
            if i == 2 {
                s.value += ((declared - s.value) / 360.0).round() * 360.0;
            }
            s.to(declared);
            animating |= s.step(dt);
            s.value
        });
    }
    for c in n.children_mut() {
        animating |= transitions(c, pal, motion, dt);
    }
    animating
}

/// Push hover and press into every named node's fill, proportionally, and
/// slide the ones the wheel has scrolled.
fn state(
    n: &mut El,
    pal: &Palette,
    of: &dyn Fn(&str) -> Option<(f64, f64)>,
    scrolls: &BTreeMap<String, [f64; 2]>,
) {
    if let Some([x, y]) = n.key().and_then(|k| scrolls.get(k)).copied() {
        // `scrolled` is a builder and a built node cannot be reopened.
        let node = std::mem::replace(n, mui_core::leaf(0.0, 0.0));
        *n = node.scrolled(x, y);
    }
    if let Some((h, p)) = n.key().and_then(of) {
        let bg = pal.background();
        let e = n.payload_mut();
        if !e.style.fill.is_none() && (h > 0.0 || p > 0.0) {
            e.style.fill = e.style.fill.map(pal, bg, |c| {
                c.mix(pal.hover(c), h as f32).mix(pal.pressed(c), p as f32)
            });
        }
    }
    for c in n.children_mut() {
        state(c, pal, of, scrolls);
    }
}
impl std::fmt::Debug for Ui {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ui")
            .field("springs", &self.springs.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_core::prelude::*;
    use mui_geometry::Point;
    use mui_input::Mods;

    fn at(x: f64, y: f64, down: bool) -> PointerInput {
        PointerInput {
            pos: Some(Point::new(x, y)),
            primary_down: down,
        }
    }
    fn key(k: Key) -> Input {
        Input {
            keys: vec![KeyPress {
                key: k,
                mods: Mods::default(),
            }],
            ..Input::default()
        }
    }

    #[test]
    fn tab_walks_the_focusable_surfaces_in_scene_order() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            column([
                leaf(20., 20.).focusable().id("a"),
                leaf(20., 20.).id("plain"),
                leaf(20., 20.).focusable().id("b"),
            ])
        };
        ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        for want in ["a", "b", "a"] {
            ui.frame(tree(), None, key(Key::Tab), 0.016).unwrap();
            assert!(ui.focused(want), "expected {want}");
        }
        ui.frame(tree(), None, key(Key::Escape), 0.016).unwrap();
        assert!(!ui.focused("a") && !ui.focused("b"));
    }

    #[test]
    fn the_wheel_scrolls_a_column_and_stops_at_its_end() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || {
            column([leaf(20., 100.), leaf(20., 100.)])
                .height(50.)
                .scroll()
                .id("list")
        };
        let wheel = |y: f64| Input {
            pointer: at(10., 10., false),
            wheel: Point::new(0., y),
            ..Input::default()
        };
        ui.frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        ui.frame(tree(), None, wheel(30.), 0.016).unwrap();
        assert_eq!(ui.scroll("list"), [0., 30.]);
        ui.frame(tree(), None, wheel(1000.), 0.016).unwrap();
        let [_, y] = ui.scroll("list");
        assert!(y > 30. && y <= 200., "clamped to the overflow, got {y}");
        ui.frame(tree(), None, wheel(-1e6), 0.016).unwrap();
        assert_eq!(ui.scroll("list"), [0., 0.], "and not past the top");
    }

    #[test]
    fn typing_reaches_the_focused_field() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut value = String::new();
        let tree = |ui: &mut Ui, v: &mut String| crate::widgets::text_input(ui, "f", v);
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, PointerInput::default(), 0.016)
            .unwrap();
        ui.focus("f");
        let typed = Input {
            text: "hi".into(),
            ..Input::default()
        };
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, typed, 0.016).unwrap();
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, key(Key::Backspace), 0.016).unwrap();
        let root = tree(&mut ui, &mut value);
        ui.frame(root, None, PointerInput::default(), 0.016)
            .unwrap();
        assert_eq!(value, "h");
    }

    #[test]
    fn a_selection_is_extended_by_shift_and_deleted_as_one() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut value = String::from("hello");
        let run = |ui: &mut Ui, v: &mut String, input: Input| {
            let root = crate::widgets::text_input(ui, "f", v);
            ui.frame(root, None, input, 0.016).unwrap();
        };
        run(&mut ui, &mut value, Input::default());
        ui.focus("f");
        let shift = |k| Input {
            keys: vec![KeyPress {
                key: k,
                mods: Mods {
                    shift: true,
                    ..Mods::default()
                },
            }],
            ..Input::default()
        };
        run(&mut ui, &mut value, shift(Key::Right));
        run(&mut ui, &mut value, shift(Key::Right));
        run(&mut ui, &mut value, key(Key::Backspace));
        run(&mut ui, &mut value, Input::default());
        assert_eq!(value, "llo", "two characters selected, one Backspace");
    }

    #[test]
    fn copy_asks_the_host_for_the_clipboard_and_paste_takes_it_back() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let mut value = String::from("hi");
        let run = |ui: &mut Ui, v: &mut String, input: Input| {
            let root = crate::widgets::text_input(ui, "f", v);
            ui.frame(root, None, input, 0.016)
                .unwrap()
                .clipboard
                .clone()
        };
        run(&mut ui, &mut value, Input::default());
        ui.focus("f");
        let ctrl = |c: char, clipboard: Option<String>| Input {
            keys: vec![KeyPress {
                key: Key::Char(c),
                mods: Mods {
                    ctrl: true,
                    ..Mods::default()
                },
            }],
            clipboard,
            ..Input::default()
        };
        run(&mut ui, &mut value, ctrl('a', None));
        run(&mut ui, &mut value, ctrl('c', None));
        let out = run(&mut ui, &mut value, Input::default());
        assert_eq!(out.as_deref(), Some("hi"), "the copy reached the frame");
        run(&mut ui, &mut value, ctrl('v', Some("yo".into())));
        run(&mut ui, &mut value, Input::default());
        assert_eq!(value, "yo", "and a paste replaced the selection");
    }

    #[test]
    fn a_tip_comes_due_after_half_a_second_of_hover() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).tip("why").id("b");
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        let f = ui.frame(tree(), None, at(10., 10., false), 0.4).unwrap();
        assert!(f.tip.is_none(), "the pointer has not rested long enough");
        let f = ui.frame(tree(), None, at(10., 10., false), 0.6).unwrap();
        let (t, at) = f.tip.clone().expect("due");
        assert_eq!(t, "why");
        assert!(at.y > 40., "below the surface");
        assert!(
            f.scene.surfaces().any(|s| s.frame.y > 40.),
            "and floated into the scene"
        );
        let f = ui
            .frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap();
        assert!(f.tip.is_none(), "gone when the pointer leaves");
    }

    fn solid(f: &Frame) -> mui_core::Paint {
        f.scene.paint[0].paint.clone()
    }

    #[test]
    fn a_transition_lands_between_the_two_fills_and_settles() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = |on: bool| {
            leaf(40., 40.)
                .fill(if on { Role::Primary } else { Role::Field })
                .animate()
                .id("b")
        };
        let from = solid(
            &ui.frame(tree(false), None, Input::default(), 0.016)
                .unwrap(),
        );
        let mid = solid(&ui.frame(tree(true), None, Input::default(), 0.016).unwrap());
        assert_ne!(mid, from, "it left the old fill");
        let mut t = 0.0;
        let to = loop {
            let f = ui.frame(tree(true), None, Input::default(), 0.016).unwrap();
            t += 0.016;
            let paint = solid(&f);
            assert!(t < 2.0, "never settled");
            if !f.animating {
                break paint;
            }
        };
        assert_ne!(mid, to, "and the middle was not the end");
        let mut fresh = Ui::new(Theme::DEFAULT);
        let want = solid(
            &fresh
                .frame(tree(true), None, Input::default(), 0.016)
                .unwrap(),
        );
        assert_eq!(to, want, "it settles on the declared fill");
    }

    #[test]
    fn retargeting_mid_flight_does_not_jump() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = |on: bool| {
            leaf(40., 40.)
                .fill(if on { Role::Primary } else { Role::Field })
                .animate()
                .id("b")
        };
        let home = solid(
            &ui.frame(tree(false), None, Input::default(), 0.016)
                .unwrap(),
        );
        for _ in 0..3 {
            ui.frame(tree(true), None, Input::default(), 0.016).unwrap();
        }
        let before = solid(&ui.frame(tree(true), None, Input::default(), 0.016).unwrap());
        let after = solid(
            &ui.frame(tree(false), None, Input::default(), 0.016)
                .unwrap(),
        );
        assert_ne!(after, home, "a retarget carries velocity, it does not snap");
        assert_ne!(after, before, "and it keeps moving");
    }

    #[test]
    fn a_tween_walks_to_its_target_and_never_back() {
        let mut ui = Ui::new(Theme::DEFAULT);
        assert_eq!(ui.tween("cutoff", 0.0), 0.0, "it starts where it is told");
        let mut prev = 0.0;
        for _ in 0..180 {
            ui.frame(leaf(1., 1.), None, Input::default(), 0.016)
                .unwrap();
            let v = ui.tween("cutoff", 1.0);
            assert!(v >= prev, "went backwards: {v} after {prev}");
            assert!(v <= 1.0 + 1e-9, "overshot to {v}");
            prev = v;
        }
        assert!(prev > 0.99, "arrived: {prev}");
    }

    #[test]
    fn a_press_and_its_release_bracket_the_gesture() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).id("b");
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        let f = ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
        assert_eq!(f.edits, vec![("b".to_owned(), Edit::Begin)]);
        assert_eq!(ui.edit("b"), Some(Edit::Begin));
        let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        assert_eq!(f.edits, vec![("b".to_owned(), Edit::End)]);
        assert_eq!(ui.edit("b"), Some(Edit::End));
        let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        assert!(f.edits.is_empty());
    }

    #[test]
    fn a_cancelled_gesture_still_ends() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).id("b");
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
        ui.cancel();
        let f = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        assert_eq!(f.edits, vec![("b".to_owned(), Edit::End)]);
    }

    #[test]
    fn hover_warms_the_fill_and_a_press_is_reported_next_frame() {
        let mut ui = Ui::new(Theme::DEFAULT);
        let tree = || leaf(40., 40.).fill(Role::Raised).id("b");
        let base = ui
            .frame(tree(), None, PointerInput::default(), 0.016)
            .unwrap()
            .scene
            .paint[0]
            .paint
            .clone();
        ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        let f = ui.frame(tree(), None, at(10., 10., true), 0.016).unwrap();
        assert!(f.animating);
        assert_ne!(f.scene.paint[0].paint, base);
        let _ = ui.frame(tree(), None, at(10., 10., false), 0.016).unwrap();
        assert!(ui.get("b").released);
    }
}
