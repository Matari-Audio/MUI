use std::{
    collections::BTreeSet,
    sync::{mpsc::Sender, Arc},
};
use truce_core::editor::PluginContext;
use truce_params::{ParamFlags, ParamInfo, Params};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Edit {
    Begin(u32),
    Value(u32, f64),
    End(u32),
}

/// Host-thread gesture dispatch. Close it before destroying the editor worker.
#[derive(Default)]
pub struct Automation {
    active: BTreeSet<u32>,
}
impl Automation {
    pub fn dispatch(&mut self, context: &PluginContext, edit: Edit) {
        let id = match edit {
            Edit::Begin(id) | Edit::Value(id, _) | Edit::End(id) => id,
        };
        if context.params().get_normalized(id).is_none() {
            return;
        }
        match edit {
            Edit::Begin(_) if self.active.insert(id) => context.begin_edit(id),
            Edit::Value(_, value) if value.is_finite() && self.active.contains(&id) => {
                context.set_param(id, value.clamp(0., 1.))
            }
            Edit::End(_) if self.active.remove(&id) => context.end_edit(id),
            _ => {}
        }
    }
    pub fn close(&mut self, context: &PluginContext) {
        for id in std::mem::take(&mut self.active) {
            context.end_edit(id);
        }
    }
}

/// Shared pointer/keyboard/reset/cancel semantics, independent of GPUI rendering.
/// Values and descriptors come from the plugin's existing Truce Params store.
pub struct Parameter {
    info: ParamInfo,
    pub modulatable: bool,
    params: Arc<dyn Params>,
    edits: Sender<Edit>,
    initial: Option<f64>,
    preview: f64,
    enabled: bool,
}
impl Parameter {
    pub fn info(&self) -> &ParamInfo {
        &self.info
    }
    pub fn new(
        params: Arc<dyn Params>,
        id: u32,
        modulatable: bool,
        edits: Sender<Edit>,
    ) -> Option<Self> {
        Self::new_many(params, &[(id, modulatable)], edits)?.pop()
    }
    /// Bind a card's controls with one read of Truce's parameter metadata.
    pub fn new_many(
        params: Arc<dyn Params>,
        ids: &[(u32, bool)],
        edits: Sender<Edit>,
    ) -> Option<Vec<Self>> {
        let infos = params.param_infos();
        ids.iter()
            .map(|&(id, modulatable)| {
                let info = *infos.iter().find(|info| info.id == id)?;
                let preview = params.get_normalized(id)?;
                Some(Self {
                    info,
                    modulatable,
                    params: params.clone(),
                    edits: edits.clone(),
                    initial: None,
                    preview,
                    enabled: true,
                })
            })
            .collect()
    }
    pub fn value(&self) -> f64 {
        if self.initial.is_some() {
            self.preview
        } else {
            self.params
                .get_normalized(self.info.id)
                .unwrap_or(self.preview)
        }
    }
    pub fn text(&self) -> String {
        self.params
            .format_value(self.info.id, self.info.range.denormalize(self.value()))
            .unwrap_or_default()
    }
    pub fn begin(&mut self) -> bool {
        if !self.enabled || self.info.flags.contains(ParamFlags::READONLY) || self.initial.is_some()
        {
            return false;
        }
        self.preview = self.value();
        if self.edits.send(Edit::Begin(self.info.id)).is_err() {
            return false;
        }
        self.initial = Some(self.preview);
        true
    }
    pub fn set(&mut self, normalized: f64) {
        if self.initial.is_none() || !normalized.is_finite() {
            return;
        }
        // Let Truce perform discrete/enum quantization through its declared range.
        let next = self
            .info
            .range
            .normalize(self.info.range.denormalize(normalized.clamp(0., 1.)));
        if next != self.preview {
            self.preview = next;
            let _ = self.edits.send(Edit::Value(self.info.id, next));
        }
    }
    pub fn drag(&mut self, delta: f64, fine: bool) {
        if let Some(initial) = self.initial {
            self.set(initial + delta * if fine { 0.1 } else { 1. });
        }
    }
    pub fn end(&mut self) {
        if self.initial.take().is_some() {
            let _ = self.edits.send(Edit::End(self.info.id));
        }
    }
    pub fn cancel(&mut self) {
        if let Some(initial) = self.initial {
            self.set(initial);
            self.end();
        }
    }
    pub fn set_enabled(&mut self, enabled: bool) {
        if !enabled {
            self.cancel();
        }
        self.enabled = enabled;
    }
    pub fn step(&mut self, delta: f64) {
        if self.begin() {
            self.set(self.preview + delta);
            self.end();
        }
    }
    pub fn reset(&mut self) {
        if self.begin() {
            self.set(self.info.range.normalize(self.info.default_plain));
            self.end();
        }
    }
    /// Uses the plugin's Truce `#[param(parse = "...")]` hook; no second unit parser.
    pub fn parse(&mut self, text: &str) -> bool {
        let Some(plain) = self.params.parse_value(self.info.id, text) else {
            return false;
        };
        if !plain.is_finite() || !self.begin() {
            return false;
        }
        self.set(self.info.range.normalize(plain));
        self.end();
        true
    }
}
impl Drop for Parameter {
    fn drop(&mut self) {
        self.end();
    }
}
