//! `--trace`: one JSONL record per stepped frame of what changed.
//!
//! Rune's `--trace` records keyed elements' motion per frame; this records the
//! same question for a MUI scene: which ids moved, repainted or changed state
//! this frame, which springs are still in flight, whether `Ui::frame` says it
//! is animating. Nothing here reads a wall clock unless `cost` is on, and that
//! one field (`cost_us`) is the only thing that differs between two runs.
//!
//! Record: `{"frame", "t", "animating", "springs": [ids], "changed": [{"id",
//! "what": ["added"|"removed"|"frame"|"paint"|"state"|"semantics"]}],
//! "edits": [{"id","edit"}], "surfaces", "paint", "pointer", "buttons"}`.
//! `springs` are hover/press springs strictly between rest and target.

use std::collections::{BTreeMap, HashMap};
use std::hash::{Hash, Hasher};

use mui::prelude::*;
use mui::scene::{Layer, Paint};
use serde_json::{Value, json};

use crate::Error;
use crate::session::{Session, Step};
use crate::tree::{listed, r2, r4};

#[derive(PartialEq)]
struct Sig {
    frame: [i64; 4],
    paint: u64,
    state: [i64; 2],
    semantics: u64,
}

fn q(v: f64) -> i64 {
    (v * 100.0).round() as i64
}

/// Tracks the previous frame's signatures and reports the difference.
#[derive(Default)]
pub struct Tracer {
    prev: Option<BTreeMap<String, Sig>>,
    /// The last frame that was still animating or had a spring in flight.
    pub last_moving: Option<u64>,
    pub frames: u64,
}

impl Tracer {
    /// Every listed surface's signature, and the ids with a spring in flight.
    fn sigs(s: &Session) -> Result<(BTreeMap<String, Sig>, Vec<String>), Error> {
        let scene = s.scene()?;
        let ui = s.ui();
        // Paint per id, hashed in paint order with a fixed-key hasher.
        let mut paint: HashMap<&str, std::hash::DefaultHasher> = HashMap::new();
        for p in &scene.paint {
            let h = paint.entry(&p.key).or_default();
            format!("{:?}", p.layer).hash(h);
            [q(p.offset.x), q(p.offset.y), q(p.width), q(p.blur)].hash(h);
            p.transform.as_coeffs().map(q).hash(h);
            // The shape itself: a new contour in the same frame repaints.
            format!("{:?}", p.path).hash(h);
            match &p.paint {
                Paint::Solid(c) => {
                    (
                        1u8,
                        c.to_srgb().components.map(|v| (v * 1000.0).round() as i32),
                    )
                        .hash(h);
                }
                _ => 0u8.hash(h),
            }
            if let Some(t) = &p.text {
                (q(t.origin.x), q(t.origin.y)).hash(h);
                // Which glyphs, not just how many: equal-length text changes.
                format!("{:?}", t.glyphs).hash(h);
            }
            if p.layer == Layer::Text {
                1u8.hash(h);
            }
        }
        let mut now = BTreeMap::new();
        let mut springs = Vec::new();
        for sf in scene.surfaces().filter(|sf| listed(sf)) {
            let id = sf.key.to_string();
            let st = ui.state(id.as_str());
            if [st.hover, st.press]
                .iter()
                .any(|v| *v > 0.001 && *v < 0.999)
            {
                springs.push(id.clone());
            }
            let mut sem = std::hash::DefaultHasher::new();
            format!("{:?}", sf.semantics).hash(&mut sem);
            (
                sf.disabled,
                ui.focused(id.as_str()),
                sf.text_value.as_deref(),
            )
                .hash(&mut sem);
            now.insert(
                id.clone(),
                Sig {
                    frame: [
                        q(sf.frame.x),
                        q(sf.frame.y),
                        q(sf.frame.size.width),
                        q(sf.frame.size.height),
                    ],
                    paint: paint.get(id.as_str()).map_or(0, Hasher::finish),
                    state: [
                        (st.hover * 100.0).round() as i64,
                        (st.press * 100.0).round() as i64,
                    ],
                    semantics: sem.finish(),
                },
            );
        }
        Ok((now, springs))
    }

    /// Take the warm-up scene as the baseline, without tracing it as a
    /// frame: the first step's changes then report against it.
    pub fn seed(&mut self, s: &Session) -> Result<(), Error> {
        self.prev = Some(Self::sigs(s)?.0);
        Ok(())
    }

    pub fn record(
        &mut self,
        s: &Session,
        step: &Step,
        cost_us: Option<u128>,
    ) -> Result<Value, Error> {
        let scene = s.scene()?;
        let (now, springs) = Self::sigs(s)?;
        let mut changed = Vec::new();
        if let Some(prev) = &self.prev {
            for (id, sig) in &now {
                let what: Vec<&str> = match prev.get(id) {
                    None => vec!["added"],
                    Some(p) => [
                        (p.frame != sig.frame, "frame"),
                        (p.paint != sig.paint, "paint"),
                        (p.state != sig.state, "state"),
                        (p.semantics != sig.semantics, "semantics"),
                    ]
                    .into_iter()
                    .filter_map(|(d, n)| d.then_some(n))
                    .collect(),
                };
                if !what.is_empty() {
                    changed.push(json!({"id": id, "what": what}));
                }
            }
            for id in prev.keys().filter(|id| !now.contains_key(*id)) {
                changed.push(json!({"id": id, "what": ["removed"]}));
            }
        }
        self.prev = Some(now);
        self.frames += 1;
        if step.animating || !springs.is_empty() {
            self.last_moving = Some(step.frame);
        }
        let (pos, buttons) = s.pointer();
        let held = [Button::Primary, Button::Secondary, Button::Middle]
            .iter()
            .filter(|b| buttons.contains(**b))
            .count();
        let mut rec = json!({
            "frame": step.frame,
            "t": r4(s.time()),
            "animating": step.animating,
            "springs": springs,
            "changed": changed,
            "edits": step.edits.iter().map(|(id, e)| json!({
                "id": id,
                "edit": if *e == Edit::Begin { "begin" } else { "end" },
            })).collect::<Vec<_>>(),
            "surfaces": scene.surfaces().count(),
            "paint": scene.paint.len(),
            "pointer": pos.map(|p| json!([r2(p.x), r2(p.y)])),
            "buttons": held,
        });
        if let (Some(us), Some(o)) = (cost_us, rec.as_object_mut()) {
            o.insert("cost_us".into(), json!(us));
        }
        Ok(rec)
    }
}
