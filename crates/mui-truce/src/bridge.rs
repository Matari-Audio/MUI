//! Widget ids bound to truce parameters: a `Ui` gesture becomes the host's
//! begin/perform/end, and whatever the host did to a parameter is the value
//! the next tree reads.
use std::sync::Arc;

use mui::layout::Id;
use mui::scene::{El, IntoEl};
use mui::widgets::{self, Control};
use mui::{Edit, Ui};
use truce_core::editor::PluginContext;
use truce_params::{ParamFlags, ParamInfo, ParamRange, Params};

/// The widget id [`Bridge::bind`] gives parameter `param`: `param/<id>`.
/// Derived, never typed, so the gesture a widget reports is always the one
/// its parameter listens for.
pub fn widget_id(param: impl Into<u32>) -> Id {
    Id::of("param").entity(u64::from(param.into()))
}

/// The editor's side of the plugin: its parameter store, the host's gesture
/// channel while the window is open, and the gestures that channel owes an
/// end to.
pub struct Bridge<P: ?Sized = dyn Params> {
    params: Arc<P>,
    context: Option<PluginContext<P>>,
    infos: Box<[ParamInfo]>,
    /// Formatted domain samples, shaped by MUI in the active font. Shared
    /// across builds so a parameter view does not allocate its reserve again.
    readouts: Box<[Arc<[String]>]>,
    meters: Box<[u32]>,
    /// Open gestures: the id, the value the control holds, and the value
    /// last sent. The control reads its own value back, not the store, so a
    /// host echo cannot jitter a drag, and an incremental drag across a
    /// discrete parameter accumulates between the steps it sends.
    open: Vec<(u32, f64, f64)>,
    /// The ids `bind` saw this build: an open gesture not among them lost
    /// its widget, and nothing else would ever end it.
    bound: Vec<u32>,
    /// Every parameter and meter as last seen, bit for bit: `changed`
    /// compares against it without hashing or allocating.
    seen: Box<[u64]>,
}

impl<P: Params + ?Sized> Bridge<P> {
    /// Reads the parameter table once; no host until [`Bridge::attach`].
    pub fn new(params: Arc<P>) -> Self {
        let infos = params.param_infos().into_boxed_slice();
        let readouts = infos
            .iter()
            .map(|info| readout_samples(&*params, info))
            .collect();
        let meters = params.meter_ids().into_boxed_slice();
        let seen = vec![u64::MAX; infos.len() + meters.len()].into_boxed_slice();
        Self {
            params,
            context: None,
            infos,
            readouts,
            meters,
            open: Vec::new(),
            bound: Vec::new(),
            seen,
        }
    }

    /// The typed store, for everything the bridge does not wrap.
    pub fn params(&self) -> &Arc<P> {
        &self.params
    }

    /// The host channel, while the editor is open.
    pub fn context(&self) -> Option<&PluginContext<P>> {
        self.context.as_ref()
    }

    /// Start talking to a host. Replacing a context ends the gestures the
    /// old one was owed.
    pub fn attach(&mut self, context: PluginContext<P>) {
        self.close();
        self.context = Some(context);
        self.seen.fill(u64::MAX);
    }

    /// End every open gesture and let the host go. Safe to call twice.
    pub fn close(&mut self) {
        if let Some(context) = self.context.take() {
            for (id, ..) in self.open.drain(..) {
                context.end_edit(id);
            }
        }
        self.open.clear();
    }

    /// Drop the host without calling it: for `Drop`, when the host may have
    /// torn its side down already.
    pub fn detach(&mut self) {
        self.open.clear();
        self.context = None;
    }

    /// End every open gesture but stay attached: a state load replaced the
    /// values they were editing.
    pub fn end_all(&mut self) {
        if let Some(context) = &self.context {
            for (id, ..) in self.open.drain(..) {
                context.end_edit(id);
            }
        }
    }

    fn info(&self, id: u32) -> Option<&ParamInfo> {
        self.infos.iter().find(|info| info.id == id)
    }

    /// The normalized value the tree should show: the gesture's own while
    /// one is open, the store's otherwise. `0` for an unknown id.
    pub fn value(&self, id: impl Into<u32>) -> f64 {
        let id = id.into();
        self.open
            .iter()
            .find(|(open, ..)| *open == id)
            .map(|&(_, shown, _)| shown)
            .or_else(|| self.params.get_normalized(id))
            .unwrap_or(0.0)
    }

    /// The value as the plugin formats it, unit included.
    pub fn text(&self, id: impl Into<u32>) -> String {
        let id = id.into();
        // ponytail: one String per call; cache per id when a label-heavy
        // editor shows it in a profile.
        self.info(id)
            .and_then(|info| {
                self.params
                    .format_value(id, info.range.denormalize(self.value(id)))
            })
            .unwrap_or_default()
    }

    /// Build a parameter knob with its derived id, metadata name, normalized
    /// mapping, discrete keyboard step, formatted value and measured readout.
    /// Styling stays available: `bridge.knob(ui, P::Gain).size(S)`.
    /// Host gestures follow the same protocol as [`Bridge::bind`].
    /// Unknown/read-only parameters draw disabled. Numeric accessibility
    /// actions use normalized 0..1, with the formatted plain value as text.
    pub fn knob(&mut self, ui: &mut Ui, param: impl Into<u32>) -> Control {
        self.number(ui, param.into(), true)
    }

    /// The slider counterpart of [`Bridge::knob`].
    pub fn slider(&mut self, ui: &mut Ui, param: impl Into<u32>) -> Control {
        self.number(ui, param.into(), false)
    }

    /// A switch with its name and id taken from parameter metadata. Intended
    /// for boolean parameters; like [`Bridge::bind_bool`], it writes 0 or 1.
    pub fn toggle(&mut self, ui: &mut Ui, param: impl Into<u32>) -> Control {
        let id = param.into();
        let name = self.info(id).map_or("Unknown parameter", |info| info.name);
        let disabled = self
            .info(id)
            .is_none_or(|info| info.flags.contains(ParamFlags::READONLY));
        self.edit(ui, id, widget_id(id), |ui, widget, value| {
            let mut on = *value >= 0.5;
            let control = widgets::toggle(ui, widget, name, &mut on).el;
            *value = f64::from(u8::from(on));
            if disabled {
                control.disabled()
            } else {
                control
            }
        })
    }

    /// Replace a parameter's reserve samples once, when the editor is built.
    /// Returns false for an unknown parameter. Samples are measured using the
    /// actual font/axes at layout time and never clip an unexpectedly wide value.
    ///
    /// By default every discrete value is sampled up to 256 intervals;
    /// larger or continuous ranges sample their endpoints and default. An
    /// arbitrary formatter may have wider interior values or switch units:
    /// supply those here for a stable readout across its complete domain.
    pub fn reserve_readout(
        &mut self,
        param: impl Into<u32>,
        samples: impl Into<Arc<[String]>>,
    ) -> bool {
        let id = param.into();
        let Some(index) = self.infos.iter().position(|info| info.id == id) else {
            return false;
        };
        self.readouts[index] = samples.into();
        true
    }

    fn number(&mut self, ui: &mut Ui, id: u32, knob: bool) -> Control {
        let info = self.info(id);
        let name = info.map_or("Unknown parameter", |info| info.name);
        let range = info.map(|info| info.range);
        let disabled = info.is_none_or(|info| info.flags.contains(ParamFlags::READONLY));
        let samples = self
            .infos
            .iter()
            .position(|info| info.id == id)
            .map(|index| self.readouts[index].clone());
        let params = self.params.clone();
        self.edit(ui, id, widget_id(id), |ui, widget, value| {
            let mut control = if knob {
                widgets::knob(ui, widget, name, value, 0.0..=1.0).el
            } else {
                widgets::slider(ui, widget, name, value, 0.0..=1.0).el
            };
            if let Some(range) = range {
                if let Some(steps) = range.step_count() {
                    control = control.step(1.0 / f64::from(steps.get()));
                }
                if let Some(text) = params.format_value(id, range.denormalize(*value)) {
                    control = control.value_text(text);
                }
            }
            if let Some(samples) = samples {
                control = control.value_reserve_all(samples);
            }
            if disabled {
                control.disabled()
            } else {
                control
            }
        })
    }

    /// A meter the audio thread published, `0` while closed.
    pub fn meter(&self, id: impl Into<u32>) -> f32 {
        self.context.as_ref().map_or(0.0, |c| c.get_meter(id))
    }

    /// Whether any parameter or meter moved since the last call: host
    /// automation, a preset, the audio thread. The window polls this to
    /// decide whether an idle tick has anything to draw.
    pub fn changed(&mut self) -> bool {
        let mut changed = false;
        let values = self.infos.iter().map(|info| {
            self.params
                .get_normalized(info.id)
                .map_or(u64::MAX - 1, f64::to_bits)
        });
        let meters = self.meters.iter().map(|&id| {
            self.context
                .as_ref()
                .map_or(0, |c| u64::from(c.get_meter(id).to_bits()))
        });
        for (seen, now) in self.seen.iter_mut().zip(values.chain(meters)) {
            changed |= *seen != now;
            *seen = now;
        }
        changed
    }

    /// Build the widget bound to parameter `param`: `control` gets the
    /// widget id ([`widget_id`]) and the normalized value to draw and edit,
    /// and whatever it leaves there goes to the host inside the gesture
    /// `Ui` reported for that id. It returns anything that becomes an `El`:
    /// a widget's whole `Response`, or a tree of its own.
    ///
    /// ```ignore
    /// let gain = bridge.bind(ui, P::Gain, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0));
    /// ```
    ///
    /// A drag is `Begin` .. values .. `End`. A change with no gesture open --
    /// a key step, an accessibility action -- is wrapped in its own
    /// begin/set/end, and the atomic `Begin`/`End` pair `Ui` reports for it
    /// a frame later is dropped. Read-only and unknown parameters draw but
    /// never reach the host. A gesture whose parameter no `bind` of a build
    /// names -- its control dropped out of the tree -- ends with that build.
    ///
    /// One parameter bound twice (a knob and a value field) needs two widget
    /// ids: give the second one its own with [`Bridge::bind_as`].
    pub fn bind<R: IntoEl>(
        &mut self,
        ui: &mut Ui,
        param: impl Into<u32>,
        control: impl FnOnce(&mut Ui, Id, &mut f64) -> R,
    ) -> El {
        let id = param.into();
        self.bind_as(ui, id, widget_id(id), control)
    }

    /// [`Bridge::bind`] under the widget id `widget` instead of
    /// [`widget_id`]`(param)`, for a second control on the same parameter:
    /// two widgets under one id are a duplicate key, and the tree stops
    /// resolving. Gestures still bracket by parameter, whichever control
    /// made them.
    ///
    /// ```ignore
    /// let knob = bridge.bind(ui, P::Gain, |ui, id, v| knob(ui, id, "Gain", v, 0.0..=1.0));
    /// let field = widget_id(P::Gain).field("value");
    /// let entry = bridge.bind_as(ui, P::Gain, field, |ui, id, v| number(ui, id, v));
    /// ```
    pub fn bind_as<R: IntoEl>(
        &mut self,
        ui: &mut Ui,
        param: impl Into<u32>,
        widget: Id,
        control: impl FnOnce(&mut Ui, Id, &mut f64) -> R,
    ) -> El {
        self.edit(ui, param.into(), widget, |ui, widget, value| {
            control(ui, widget, value).into_el()
        })
    }

    // Preserve the control's concrete builder while sharing bind_as's edit
    // protocol. Existing bind/bind_as still return El, unchanged.
    fn edit<R>(
        &mut self,
        ui: &mut Ui,
        id: u32,
        widget: Id,
        control: impl FnOnce(&mut Ui, Id, &mut f64) -> R,
    ) -> R {
        self.bound.push(id);
        let before = self.value(id);
        let mut value = before;
        let Some(range) = self
            .info(id)
            .filter(|info| !info.flags.contains(ParamFlags::READONLY))
            .map(|info| info.range)
        else {
            return control(ui, widget, &mut value);
        };
        // At most a cancel's End, an End and a Begin reach one id per frame.
        let mut edges = [None; 4];
        for (slot, edit) in edges.iter_mut().zip(ui.edits_for(&widget)) {
            *slot = Some(edit);
        }
        let atomic = edges == [Some(Edit::Begin), Some(Edit::End), None, None] && !self.is_open(id);
        let last = |e| edges.iter().rposition(|x| *x == Some(e));
        // An End after the last Begin closes the gesture after this frame's
        // value, so the value lands inside it.
        let trailing = match (last(Edit::End), last(Edit::Begin)) {
            (Some(end), Some(begin)) if end > begin => Some(end),
            (Some(end), None) => Some(end),
            _ => None,
        };
        if !atomic {
            for (i, edge) in edges.iter().enumerate() {
                match edge {
                    Some(Edit::Begin) => self.begin(id),
                    Some(Edit::End) if Some(i) != trailing => self.end(id),
                    _ => {}
                }
            }
        }
        let el = control(ui, widget, &mut value);
        if value.to_bits() != before.to_bits() && value.is_finite() {
            if self.is_open(id) {
                self.set(id, range, value);
            } else if quantize(range, value).to_bits() != quantize(range, before).to_bits() {
                // A custom bind may still choose a sub-quantum key step.
                // Metadata controls declare the discrete quantum themselves.
                self.begin(id);
                self.set(id, range, value);
                self.end(id);
            }
        }
        if !atomic && trailing.is_some() {
            self.end(id);
        }
        el
    }

    /// [`Bridge::bind`] for a switch: `control` edits a `bool`, sent as
    /// normalized 0 or 1.
    ///
    /// ```ignore
    /// let bypass = bridge.bind_bool(ui, P::Bypass, |ui, id, on| toggle(ui, id, "Bypass", on));
    /// ```
    pub fn bind_bool<R: IntoEl>(
        &mut self,
        ui: &mut Ui,
        param: impl Into<u32>,
        control: impl FnOnce(&mut Ui, Id, &mut bool) -> R,
    ) -> El {
        self.bind(ui, param, |ui, id, v| {
            let mut on = *v >= 0.5;
            let el = control(ui, id, &mut on).into_el();
            *v = f64::from(u8::from(on));
            el
        })
    }

    /// After a build: end the gestures no `bind` in it named. `MuiEditor`
    /// calls it; a host that drives the bridge itself calls it after every
    /// build, or a gesture whose control left the tree never ends.
    pub fn end_unbound(&mut self) {
        while let Some(&(id, ..)) = self.open.iter().find(|(id, ..)| !self.bound.contains(id)) {
            self.end(id);
        }
        self.bound.clear();
    }

    fn is_open(&self, id: u32) -> bool {
        self.open.iter().any(|(open, ..)| *open == id)
    }

    fn begin(&mut self, id: u32) {
        let Some(context) = &self.context else {
            return;
        };
        if !self.is_open(id) {
            let value = self.params.get_normalized(id).unwrap_or(0.0);
            context.begin_edit(id);
            self.open.push((id, value, value));
        }
    }

    fn set(&mut self, id: u32, range: ParamRange, value: f64) {
        let (Some(context), Some((_, shown, sent))) = (
            &self.context,
            self.open.iter_mut().find(|(open, ..)| *open == id),
        ) else {
            return;
        };
        *shown = value.clamp(0.0, 1.0);
        let value = quantize(range, *shown);
        if sent.to_bits() != value.to_bits() {
            *sent = value;
            context.set_param(id, value);
        }
    }

    fn end(&mut self, id: u32) {
        if let (Some(context), Some(i)) = (
            &self.context,
            self.open.iter().position(|(open, ..)| *open == id),
        ) {
            self.open.swap_remove(i);
            context.end_edit(id);
        }
    }
}

/// Truce's declared range quantizes: a discrete parameter snaps.
fn quantize(range: ParamRange, normalized: f64) -> f64 {
    range.normalize(range.denormalize(normalized.clamp(0.0, 1.0)))
}

/// Bounded setup work, never an unbounded walk of an integer parameter range.
fn readout_samples<P: Params + ?Sized>(params: &P, info: &ParamInfo) -> Arc<[String]> {
    let mut samples = Vec::new();
    let mut add = |plain| {
        if let Some(text) = params.format_value(info.id, plain)
            && !samples.contains(&text)
        {
            samples.push(text);
        }
    };
    if let Some(steps) = info.range.step_count().filter(|n| n.get() <= 256) {
        for i in 0..=steps.get() {
            add(info
                .range
                .denormalize(f64::from(i) / f64::from(steps.get())));
        }
    } else {
        add(info.range.denormalize(0.0));
        add(info.range.denormalize(1.0));
        add(info.default_plain);
    }
    samples.into()
}

#[cfg(test)]
mod tests;
