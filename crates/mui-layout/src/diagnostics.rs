//! Opt-in bounded work tracing for measurement-cache requests.
//!
//! These are exact constraint bits and work counts, not timing samples. The
//! ordinary cache owns no recorder. An enabled trace adds history searches,
//! so its wall-clock cost must not be compared with an untraced baseline.

use crate::Id;
use rustc_hash::FxHashMap;

const NODES: usize = 128;
const REVISIONS: usize = 8;
const KEYS: usize = 64;
const DEPTHS: usize = 257;

/// Exact consumed inputs; optional extents retain their IEEE-754 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LayoutRequestInput {
    pub redo: bool,
    pub definite: [Option<u64>; 2],
    pub room: Option<u64>,
    pub container: [Option<u64>; 2],
}

#[derive(Clone, Debug)]
pub struct LayoutRequestVariant {
    pub input: LayoutRequestInput,
    pub requests: u64,
    pub hits: u64,
    pub misses: u64,
    /// Inclusive descendant misses. Ancestor rows overlap and must not be summed.
    pub child_misses: u64,
    pub rebound_nodes: u64,
}

#[derive(Clone, Debug)]
pub struct LayoutRequestRevision {
    pub revision: u64,
    /// Exact node and nearest named ancestor context for this stamp revision.
    pub id: Option<Id>,
    pub ancestor: Id,
    pub requests: u64,
    pub hits: u64,
    pub misses: u64,
    pub child_misses: u64,
    pub rebound_nodes: u64,
    pub key_overflow_requests: u64,
    pub lru_unknown_requests: u64,
    /// Requests for a previously captured key that missed again. A failed
    /// measurement can also cause this; consult the trace's failed_requests.
    pub repeated_misses: u64,
    /// Index 0 is the most recent key; 64 is a first captured use. No distance is
    /// recorded after key overflow, when untracked keys could change it.
    pub lru_reuse_distances: Vec<u64>,
    pub miss_reuse_distances: Vec<u64>,
    /// Adjacent request changes: redo, definite width/height, room, CQ width/height.
    pub changed_components: [u64; 6],
    pub keys: Vec<LayoutRequestVariant>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn input(width: u64) -> LayoutRequestInput {
        LayoutRequestInput {
            redo: false,
            definite: [Some(width), None],
            room: Some(width),
            container: [None; 2],
        }
    }

    #[test]
    fn bounded_history_reports_node_revision_and_key_coverage_overflow() {
        let mut recorder = Recorder::new();
        recorder.frame(false);
        for address in 0..=NODES as u64 {
            let ticket = recorder.begin(Some((address, 1)), None, "root", 3, input(0), false);
            recorder.finish(ticket, 2, 1, false);
        }
        for revision in 2..=REVISIONS as u64 + 1 {
            let ticket = recorder.begin(Some((0, revision)), None, "root", 3, input(0), false);
            recorder.finish(ticket, 0, 0, false);
        }
        for key in 1..=KEYS as u64 {
            let ticket = recorder.begin(Some((0, 1)), None, "root", 3, input(key), false);
            recorder.finish(ticket, 0, 0, false);
        }
        let ticket = recorder.begin(Some((0, 1)), None, "root", 3, input(0), true);
        recorder.finish(ticket, 0, 1, false);
        let trace = recorder.export();
        assert_eq!(trace.nodes.len(), NODES);
        assert_eq!(trace.node_overflow_requests, 1);
        assert_eq!(trace.revision_overflow_requests, 1);
        assert_eq!(trace.key_overflow_requests, 1);
        assert_eq!(trace.requests, trace.hits + trace.misses);
        assert_eq!(trace.depths[0].requests, trace.requests);
        assert_eq!(trace.depths[0].child_misses, (NODES as u64 + 1) * 2);
        let revision = &trace.nodes[0].revisions[0];
        assert_eq!(trace.nodes[0].revisions.len(), REVISIONS);
        assert_eq!(revision.keys.len(), KEYS);
        assert_eq!(revision.key_overflow_requests, 1);
        assert_eq!(revision.lru_unknown_requests, 2);
        assert_eq!(
            revision.lru_reuse_distances.iter().sum::<u64>(),
            KEYS as u64
        );
        assert_eq!(
            revision.keys[0].hits, 1,
            "known-key counts remain captured after overflow"
        );
        assert_eq!(trace.failed_requests, 0);
    }

    #[test]
    fn recency_and_component_changes_use_exact_same_revision_inputs() {
        let mut recorder = Recorder::new();
        recorder.frame(false);
        for key in [0, 1, 2, 3, 4, 0, 4] {
            let ticket = recorder.begin(Some((7, 1)), None, "owner", 2, input(key), key == 4);
            recorder.finish(ticket, 0, 1, false);
        }
        recorder.context_changed();
        let ticket = recorder.begin(
            Some((7, 1)),
            Some(&Id::of("new")),
            "new-owner",
            2,
            input(0),
            false,
        );
        recorder.finish(ticket, 0, 0, false);
        let trace = recorder.export();
        assert_eq!(
            trace.nodes.len(),
            2,
            "a reset cache epoch cannot alias an old stamp"
        );
        assert_ne!(trace.nodes[0].epoch, trace.nodes[1].epoch);
        let revision = &trace.nodes[0].revisions[0];
        assert_eq!(revision.keys.len(), 5);
        assert_eq!(revision.lru_reuse_distances[KEYS], 5);
        assert_eq!(revision.miss_reuse_distances[4], 1);
        assert_eq!(revision.lru_reuse_distances[1], 1);
        assert_eq!(revision.repeated_misses, 1);
        assert_eq!(revision.changed_components, [0, 6, 0, 6, 0, 0]);
        assert_eq!(revision.ancestor, Id::of("owner"));
        assert_eq!(trace.nodes[1].revisions[0].ancestor, Id::of("new-owner"));
    }
}

#[derive(Clone, Debug)]
pub struct LayoutRequestNode {
    /// Cache context epoch plus resolved stamp address identifies the slot.
    /// This is never a pointer into a rebuilt declaration tree.
    pub epoch: u64,
    pub address: u64,
    /// Last observed exact id; revisions retain their own id/ancestor context.
    pub id: Option<Id>,
    /// First observed depth. The depth aggregates cover every actual request.
    pub depth: usize,
    pub requests: u64,
    pub hits: u64,
    pub misses: u64,
    pub child_misses: u64,
    pub rebound_nodes: u64,
    pub revisions: Vec<LayoutRequestRevision>,
}

#[derive(Clone, Debug, Default)]
pub struct LayoutRequestDepth {
    pub depth: usize,
    pub requests: u64,
    pub hits: u64,
    pub misses: u64,
    pub child_misses: u64,
    pub rebound_nodes: u64,
}

/// Owned records exported after collection. Overflow counts describe requests
/// whose detailed history was not captured, not estimated distinct cardinality.
#[derive(Clone, Debug, Default)]
pub struct LayoutRequestTrace {
    pub node_limit: usize,
    pub revision_limit: usize,
    pub key_limit: usize,
    pub contexts: u64,
    pub frames: u64,
    pub bare_resolves: u64,
    pub whole_layout_reuses: u64,
    pub requests: u64,
    pub hits: u64,
    pub misses: u64,
    pub failed_requests: u64,
    pub unscanned_requests: u64,
    pub depth_overflow_requests: u64,
    pub node_overflow_requests: u64,
    pub revision_overflow_requests: u64,
    pub key_overflow_requests: u64,
    pub depths: Vec<LayoutRequestDepth>,
    pub nodes: Vec<LayoutRequestNode>,
}

#[derive(Debug, Default)]
struct RevisionState {
    previous: Option<LayoutRequestInput>,
    recent: Vec<usize>,
    overflowed: bool,
}

#[derive(Debug)]
pub(crate) struct Recorder {
    trace: LayoutRequestTrace,
    slots: FxHashMap<(u64, u64), usize>,
    states: Vec<Vec<RevisionState>>,
    epoch: u64,
}

#[derive(Clone, Copy)]
pub(crate) struct Ticket {
    depth: usize,
    node: Option<usize>,
    revision: Option<usize>,
    key: Option<usize>,
}

fn request(requests: &mut u64, hits: &mut u64, misses: &mut u64, hit: bool) {
    *requests = requests.saturating_add(1);
    if hit {
        *hits = hits.saturating_add(1);
    } else {
        *misses = misses.saturating_add(1);
    }
}

impl Recorder {
    pub(crate) fn new() -> Self {
        Self {
            trace: LayoutRequestTrace {
                node_limit: NODES,
                revision_limit: REVISIONS,
                key_limit: KEYS,
                depths: (0..DEPTHS)
                    .map(|depth| LayoutRequestDepth {
                        depth,
                        ..Default::default()
                    })
                    .collect(),
                nodes: Vec::with_capacity(NODES),
                ..Default::default()
            },
            slots: FxHashMap::with_capacity_and_hasher(NODES, Default::default()),
            states: Vec::with_capacity(NODES),
            epoch: 0,
        }
    }

    pub(crate) fn context_changed(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
        self.trace.contexts = self.trace.contexts.saturating_add(1);
    }

    pub(crate) fn frame(&mut self, bare: bool) {
        if self.epoch == 0 {
            self.context_changed();
        }
        self.trace.frames = self.trace.frames.saturating_add(1);
        self.trace.bare_resolves += u64::from(bare);
    }

    pub(crate) fn whole_layout_reused(&mut self) {
        self.trace.whole_layout_reuses = self.trace.whole_layout_reuses.saturating_add(1);
    }

    pub(crate) fn begin(
        &mut self,
        stamp: Option<(u64, u64)>,
        id: Option<&Id>,
        ancestor: &str,
        depth: usize,
        input: LayoutRequestInput,
        hit: bool,
    ) -> Ticket {
        request(
            &mut self.trace.requests,
            &mut self.trace.hits,
            &mut self.trace.misses,
            hit,
        );
        if let Some(row) = self.trace.depths.get_mut(depth) {
            request(&mut row.requests, &mut row.hits, &mut row.misses, hit);
        } else {
            self.trace.depth_overflow_requests += 1;
        }
        let mut ticket = Ticket {
            depth,
            node: None,
            revision: None,
            key: None,
        };
        let Some((address, revision)) = stamp else {
            self.trace.unscanned_requests += 1;
            return ticket;
        };
        let slot = (self.epoch, address);
        let node = if let Some(&node) = self.slots.get(&slot) {
            node
        } else {
            if self.trace.nodes.len() == NODES {
                self.trace.node_overflow_requests += 1;
                return ticket;
            }
            let node = self.trace.nodes.len();
            self.slots.insert(slot, node);
            self.trace.nodes.push(LayoutRequestNode {
                epoch: self.epoch,
                address,
                id: id.cloned(),
                depth,
                requests: 0,
                hits: 0,
                misses: 0,
                child_misses: 0,
                rebound_nodes: 0,
                revisions: Vec::with_capacity(REVISIONS),
            });
            self.states.push(Vec::with_capacity(REVISIONS));
            node
        };
        ticket.node = Some(node);
        let row = &mut self.trace.nodes[node];
        row.id = id.cloned();
        request(&mut row.requests, &mut row.hits, &mut row.misses, hit);
        let r = if let Some(r) = row.revisions.iter().position(|r| r.revision == revision) {
            r
        } else {
            if row.revisions.len() == REVISIONS {
                self.trace.revision_overflow_requests += 1;
                return ticket;
            }
            let r = row.revisions.len();
            row.revisions.push(LayoutRequestRevision {
                revision,
                id: id.cloned(),
                ancestor: ancestor.into(),
                requests: 0,
                hits: 0,
                misses: 0,
                child_misses: 0,
                rebound_nodes: 0,
                key_overflow_requests: 0,
                lru_unknown_requests: 0,
                repeated_misses: 0,
                lru_reuse_distances: vec![0; KEYS + 1],
                miss_reuse_distances: vec![0; KEYS + 1],
                changed_components: [0; 6],
                keys: Vec::with_capacity(KEYS),
            });
            self.states[node].push(RevisionState {
                recent: Vec::with_capacity(KEYS),
                ..Default::default()
            });
            r
        };
        ticket.revision = Some(r);
        let row = &mut row.revisions[r];
        let state = &mut self.states[node][r];
        request(&mut row.requests, &mut row.hits, &mut row.misses, hit);
        if let Some(previous) = state.previous {
            let differences = [
                previous.redo != input.redo,
                previous.definite[0] != input.definite[0],
                previous.definite[1] != input.definite[1],
                previous.room != input.room,
                previous.container[0] != input.container[0],
                previous.container[1] != input.container[1],
            ];
            for (count, differs) in row.changed_components.iter_mut().zip(differences) {
                *count += u64::from(differs);
            }
        }
        state.previous = Some(input);
        let found = row.keys.iter().position(|v| v.input == input);
        let key = if let Some(key) = found {
            if !hit {
                row.repeated_misses += 1;
            }
            key
        } else {
            if row.keys.len() == KEYS {
                row.key_overflow_requests += 1;
                self.trace.key_overflow_requests += 1;
                state.overflowed = true;
                row.lru_unknown_requests += 1;
                return ticket;
            }
            let key = row.keys.len();
            row.keys.push(LayoutRequestVariant {
                input,
                requests: 0,
                hits: 0,
                misses: 0,
                child_misses: 0,
                rebound_nodes: 0,
            });
            key
        };
        ticket.key = Some(key);
        if state.overflowed {
            row.lru_unknown_requests += 1;
        } else {
            let position = state.recent.iter().position(|&k| k == key);
            let distance = position.unwrap_or(KEYS);
            row.lru_reuse_distances[distance] += 1;
            row.miss_reuse_distances[distance] += u64::from(!hit);
            if let Some(position) = position {
                state.recent.remove(position);
            }
            state.recent.insert(0, key);
        }
        let variant = &mut row.keys[key];
        request(
            &mut variant.requests,
            &mut variant.hits,
            &mut variant.misses,
            hit,
        );
        ticket
    }

    pub(crate) fn finish(
        &mut self,
        ticket: Ticket,
        child_misses: usize,
        rebound: usize,
        failed: bool,
    ) {
        self.trace.failed_requests += u64::from(failed);
        let add = |misses: &mut u64, bound: &mut u64| {
            *misses = misses.saturating_add(child_misses as u64);
            *bound = bound.saturating_add(rebound as u64);
        };
        if let Some(row) = self.trace.depths.get_mut(ticket.depth) {
            add(&mut row.child_misses, &mut row.rebound_nodes);
        }
        let Some(node) = ticket.node else {
            return;
        };
        let row = &mut self.trace.nodes[node];
        add(&mut row.child_misses, &mut row.rebound_nodes);
        let Some(revision) = ticket.revision else {
            return;
        };
        let row = &mut row.revisions[revision];
        add(&mut row.child_misses, &mut row.rebound_nodes);
        let Some(key) = ticket.key else {
            return;
        };
        let row = &mut row.keys[key];
        add(&mut row.child_misses, &mut row.rebound_nodes);
    }

    pub(crate) fn export(mut self) -> LayoutRequestTrace {
        self.trace.depths.retain(|d| d.requests != 0);
        self.trace
    }
}
