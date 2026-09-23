//! Widget ids bound to truce parameters: a `Ui` gesture becomes the host's
//! begin/perform/end, and whatever the host did to a parameter is the value
//! the next tree reads.
use std::sync::Arc;

use mui::scene::El;
use mui::{Edit, Ui};
use truce_core::editor::PluginContext;
use truce_params::{ParamFlags, ParamInfo, ParamRange, Params};

/// The editor's side of the plugin: its parameter store, the host's gesture
/// channel while the window is open, and the gestures that channel owes an
/// end to.
pub struct Bridge<P: ?Sized = dyn Params> {
    params: Arc<P>,
    context: Option<PluginContext<P>>,
    infos: Box<[ParamInfo]>,
    meters: Box<[u32]>,
    /// Open gestures: the id, the value the control holds, and the value
    /// last sent. The control reads its own value back, not the store, so a
    /// host echo cannot jitter a drag, and an incremental drag across a
    /// discrete parameter accumulates between the steps it sends.
    open: Vec<(u32, f64, f64)>,
    /// Every parameter and meter as last seen, bit for bit: `changed`
    /// compares against it without hashing or allocating.
    seen: Box<[u64]>,
}

impl<P: Params + ?Sized> Bridge<P> {
    /// Reads the parameter table once; no host until [`Bridge::attach`].
    pub fn new(params: Arc<P>) -> Self {
        let infos = params.param_infos().into_boxed_slice();
        let meters = params.meter_ids().into_boxed_slice();
        let seen = vec![u64::MAX; infos.len() + meters.len()].into_boxed_slice();
        Self {
            params,
            context: None,
            infos,
            meters,
            open: Vec::new(),
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

    /// Build `widget` bound to parameter `param`: `control` gets the
    /// normalized value to draw and edit, and whatever it leaves there goes
    /// to the host inside the gesture `Ui` reported for `widget`.
    ///
    /// ```ignore
    /// let gain = bridge.bind(ui, "gain", P::Gain, |ui, v| knob(ui, "gain", "Gain", v, 0.0..=1.0).0.into());
    /// ```
    ///
    /// A drag is `Begin` .. values .. `End`. A change with no gesture open --
    /// a key step, an accessibility action -- is wrapped in its own
    /// begin/set/end, and the atomic `Begin`/`End` pair `Ui` reports for it
    /// a frame later is dropped. Read-only and unknown parameters draw but
    /// never reach the host.
    pub fn bind(
        &mut self,
        ui: &mut Ui,
        widget: &str,
        param: impl Into<u32>,
        control: impl FnOnce(&mut Ui, &mut f64) -> El,
    ) -> El {
        let id = param.into();
        let before = self.value(id);
        let mut value = before;
        let Some(range) = self
            .info(id)
            .filter(|info| !info.flags.contains(ParamFlags::READONLY))
            .map(|info| info.range)
        else {
            return control(ui, &mut value);
        };
        // At most a cancel's End, an End and a Begin reach one id per frame.
        let mut edges = [None; 4];
        for (slot, edit) in edges.iter_mut().zip(ui.edits_for(widget)) {
            *slot = Some(edit);
        }
        let atomic = edges == [Some(Edit::Begin), Some(Edit::End), None, None]
            && !self.is_open(id);
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
        let el = control(ui, &mut value);
        if value.to_bits() != before.to_bits() && value.is_finite() {
            if self.is_open(id) {
                self.set(id, range, value);
            } else if quantize(range, value).to_bits() != quantize(range, before).to_bits() {
                // ponytail: a key step smaller than a discrete parameter's
                // step is dropped here; widgets step by a hundredth of the
                // range, so stepping a 1..8 switch needs its own step size.
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
