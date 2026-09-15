use std::{
    collections::{BTreeMap, BTreeSet},
    sync::{
        RwLock,
        atomic::{AtomicU64, Ordering},
    },
};
use truce_core::custom_state::{PersistField, State, StateCursor, StateField};

#[derive(Clone, Debug, Default, PartialEq, truce_derive::State)]
pub struct Module {
    pub id: u64,
    pub kind: String,
    pub enabled: bool,
    /// Stable Truce IDs; never derive these from visual order.
    pub parameters: Vec<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Target {
    Parameter(u32),
    Input(u64),
    Depth(u64),
}
impl Default for Target {
    fn default() -> Self {
        Self::Input(0)
    }
}
impl StateField for Target {
    fn write_field(&self, out: &mut Vec<u8>) {
        let (tag, id) = match *self {
            Self::Parameter(id) => (0u8, u64::from(id)),
            Self::Input(id) => (1, id),
            Self::Depth(id) => (2, id),
        };
        tag.write_field(out);
        id.write_field(out);
    }
    fn read_field(cursor: &mut StateCursor) -> Option<Self> {
        let tag = u8::read_field(cursor)?;
        let id = u64::read_field(cursor)?;
        match tag {
            0 => Some(Self::Parameter(id.try_into().ok()?)),
            1 => Some(Self::Input(id)),
            2 => Some(Self::Depth(id)),
            _ => None,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, truce_derive::State)]
pub struct Route {
    pub id: u64,
    pub source: u64,
    pub target: Target,
    pub depth: f64,
}

#[derive(Clone, Debug, PartialEq, truce_derive::State)]
pub struct EditorState {
    pub version: u32,
    pub name: String,
    /// OKLCH seed. Appearance is per instance; transient hover/focus is not saved.
    pub lightness: f64,
    pub chroma: f64,
    pub hue: f64,
    pub modules: Vec<Module>,
    pub routes: Vec<Route>,
    pub retired_parameters: Vec<u32>,
    pub next_module: u64,
    pub next_route: u64,
}
impl Default for EditorState {
    fn default() -> Self {
        Self {
            version: 1,
            name: String::new(),
            lightness: 0.65,
            chroma: 0.,
            hue: 240.,
            modules: Vec::new(),
            routes: Vec::new(),
            retired_parameters: Vec::new(),
            next_module: 1,
            next_route: 1,
        }
    }
}
impl EditorState {
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.next_module == 0 || self.next_route == 0 {
            return Err("invalid identity counters");
        }
        if self.version != 1 {
            return Err("unsupported document version");
        }
        if self.name.len() > 4096 || self.modules.len() > 256 || self.routes.len() > 4096 {
            return Err("document limit exceeded");
        }
        if !self.lightness.is_finite()
            || !(0. ..=1.).contains(&self.lightness)
            || !self.chroma.is_finite()
            || !(0. ..=0.5).contains(&self.chroma)
            || !self.hue.is_finite()
            || !(0. ..360.).contains(&self.hue)
        {
            return Err("invalid theme seed");
        }
        if self.retired_parameters.len() > 262_144 {
            return Err("retired parameter limit exceeded");
        }
        let retired: BTreeSet<_> = self.retired_parameters.iter().copied().collect();
        if retired.len() != self.retired_parameters.len() {
            return Err("duplicate retired parameter");
        }
        let mut nodes = BTreeSet::new();
        let mut parameters = BTreeMap::new();
        for node in &self.modules {
            if node.id == 0
                || node.id >= self.next_module
                || !nodes.insert(node.id)
                || node.kind.is_empty()
                || node.kind.len() > 64
                || node.parameters.len() > 1024
            {
                return Err("invalid module identity");
            }
            for id in &node.parameters {
                if retired.contains(id) || parameters.insert(*id, node.id).is_some() {
                    return Err("duplicate parameter identity");
                }
            }
        }
        let mut routes = BTreeMap::new();
        for route in &self.routes {
            if route.id == 0
                || route.id >= self.next_route
                || routes.insert(route.id, route).is_some()
                || !nodes.contains(&route.source)
                || !route.depth.is_finite()
                || !(-1. ..=1.).contains(&route.depth)
            {
                return Err("invalid route");
            }
        }
        let mut edges: BTreeMap<u64, BTreeSet<u64>> = BTreeMap::new();
        let mut pairs = BTreeSet::new();
        for route in &self.routes {
            let mut target = route.target;
            let mut parents = BTreeSet::new();
            let pair = match target {
                Target::Parameter(id) => (0, u64::from(id)),
                Target::Input(id) => (1, id),
                Target::Depth(id) => (2, id),
            };
            if !pairs.insert((route.source, pair)) {
                return Err("duplicate route");
            }
            let destination = loop {
                match target {
                    Target::Parameter(id) => {
                        break *parameters.get(&id).ok_or("missing parameter")?;
                    }
                    Target::Input(id) => {
                        if !nodes.contains(&id) {
                            return Err("missing input");
                        }
                        break id;
                    }
                    Target::Depth(id) => {
                        if !parents.insert(id) {
                            return Err("parent route cycle");
                        }
                        let parent = routes.get(&id).ok_or("missing parent route")?;
                        if parent.source == route.source {
                            return Err("source cannot parent its own route");
                        }
                        target = parent.target;
                    }
                }
            };
            edges.entry(route.source).or_default().insert(destination);
        }
        // Bound is 256 modules; explicit traversal avoids recursion on loaded data.
        for node in nodes {
            let mut pending = vec![node];
            let mut visited = BTreeSet::new();
            while let Some(current) = pending.pop() {
                if !visited.insert(current) {
                    continue;
                }
                for target in edges.get(&current).into_iter().flatten() {
                    if *target == node {
                        return Err("modulation feedback cycle");
                    }
                    pending.push(*target);
                }
            }
        }
        Ok(())
    }
    pub fn add_module(&mut self, kind: &str, parameters: Vec<u32>) -> Result<u64, &'static str> {
        let id = self.next_module;
        self.next_module = id.checked_add(1).ok_or("module IDs exhausted")?;
        self.modules.push(Module {
            id,
            kind: kind.into(),
            enabled: true,
            parameters,
        });
        Ok(id)
    }
    pub fn connect(&mut self, source: u64, target: Target) -> Result<u64, &'static str> {
        let id = self.next_route;
        self.next_route = id.checked_add(1).ok_or("route IDs exhausted")?;
        self.routes.push(Route {
            id,
            source,
            target,
            depth: 0.,
        });
        Ok(id)
    }
    pub fn remove_module(&mut self, id: u64) {
        let parameters: BTreeSet<_> = self
            .modules
            .iter()
            .filter(|m| m.id == id)
            .flat_map(|m| m.parameters.iter().copied())
            .collect();
        self.retired_parameters.extend(parameters.iter().copied());
        self.modules.retain(|m| m.id != id);
        self.routes.retain(|r| {
            r.source != id
                && match r.target {
                    Target::Input(n) => n != id,
                    Target::Parameter(p) => !parameters.contains(&p),
                    _ => true,
                }
        });
        loop {
            let ids: BTreeSet<_> = self.routes.iter().map(|r| r.id).collect();
            let before = self.routes.len();
            self.routes
                .retain(|r| !matches!(r.target, Target::Depth(id) if !ids.contains(&id)));
            if before == self.routes.len() {
                break;
            }
        }
    }
}

/// Place this in `#[derive(Params)]` as `#[persist] pub editor: Document`.
/// UI/host threads only: DSP must use Truce parameter atomics and prepared snapshots.
#[derive(Default)]
pub struct Document {
    state: RwLock<EditorState>,
    revision: AtomicU64,
}
impl Document {
    pub fn snapshot(&self) -> EditorState {
        self.state.read().expect("document lock poisoned").clone()
    }
    pub fn revision(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    pub fn edit<R>(
        &self,
        edit: impl FnOnce(&mut EditorState) -> Result<R, &'static str>,
    ) -> Result<R, &'static str> {
        let mut state = self.state.write().map_err(|_| "document lock poisoned")?;
        let mut next = state.clone();
        let result = edit(&mut next)?;
        // Retire removed host slots even when a caller edits the module list directly.
        for module in &state.modules {
            for id in &module.parameters {
                match next.modules.iter().find(|m| m.parameters.contains(id)) {
                    Some(owner) if owner.id != module.id => {
                        return Err("parameter ownership cannot change");
                    }
                    None if !next.retired_parameters.contains(id) => {
                        next.retired_parameters.push(*id)
                    }
                    _ => {}
                }
            }
        }
        next.validate()?;
        if next.next_module < state.next_module
            || next.next_route < state.next_route
            || state
                .retired_parameters
                .iter()
                .any(|id| !next.retired_parameters.contains(id))
            || next.modules.iter().any(|m| {
                m.id < state.next_module && !state.modules.iter().any(|old| old.id == m.id)
            })
            || next
                .routes
                .iter()
                .any(|r| r.id < state.next_route && !state.routes.iter().any(|old| old.id == r.id))
        {
            return Err("persistent IDs cannot be reused by an edit");
        }
        if *state != next {
            *state = next;
            self.revision.fetch_add(1, Ordering::Release);
        }
        Ok(result)
    }
    pub fn restore(&self, data: &[u8]) -> Result<(), &'static str> {
        if data.len() > 4 * 1024 * 1024 {
            return Err("state too large");
        }
        let next = EditorState::deserialize(data).ok_or("malformed document")?;
        next.validate()?;
        let mut state = self.state.write().map_err(|_| "document lock poisoned")?;
        if *state != next {
            *state = next;
            self.revision.fetch_add(1, Ordering::Release);
        }
        Ok(())
    }
}
impl PersistField for Document {
    fn persist_write(&self, out: &mut Vec<u8>) {
        self.snapshot().serialize().write_field(out);
    }
    fn persist_read(&self, cursor: &mut StateCursor) {
        if let Some(len) = u32::read_field(cursor)
            && len <= 4 * 1024 * 1024
            && let Some(data) = cursor.read_bytes(len as usize)
        {
            let _ = self.restore(data);
        }
    }
}
