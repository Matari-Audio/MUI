//! What is wrong with an editor, found by building it.
//!
//! [`audit`] opens the tree at every size it is given, lets it settle, and
//! reads the resolved scene: what stands outside its parent, what was
//! squeezed under its floor, what keeps repainting when nothing happens, what
//! a screen reader cannot name, and what the pointer cannot reach. A plugin
//! writes one test:
//!
//! ```
//! use mui::prelude::*;
//! let sizes = [Size::new(640., 400.), Size::new(1280., 800.)];
//! let findings = mui::audit::audit(&mut mui::Ui::default(), sizes, |_| {
//!     row([text("Gain")]).pad(8.)
//! });
//! assert!(findings.is_empty(), "{findings:#?}");
//! ```
//!
//! [`lint`] reads the declaration alone, for sizes the solver would have
//! given anyway.

use std::collections::HashMap;
use std::fmt;

use mui_geometry::Rect;
use mui_input::Input;
use mui_layout::Id;
use mui_scene::prelude::{El, Size};
use mui_scene::{Painted, ResolvedScene};

use crate::Ui;

/// Less than this is rounding, not a fault.
const SLACK: f64 = 0.5;
/// Frames a tree gets to finish its transitions before it counts as idle.
const SETTLE: usize = 240;
/// Idle frames compared for repaints.
const IDLE: usize = 8;
const DT: f64 = 1. / 60.;

/// One fault at one size.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub size: Size,
    pub fault: Fault,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Fault {
    /// `id` shows `by` units outside `parent`, its nearest named ancestor,
    /// where nothing clips it. Floats are placed against anchors and are not
    /// checked.
    Escapes { id: String, parent: String, by: f64 },
    /// `id` was arranged smaller than its measured floor: its content
    /// overflows it. `id` is empty for the whole tree, below whose floor the
    /// window itself was.
    FloorViolated { id: String, size: Size, floor: Size },
    /// `id` repainted on `count` of the idle frames, with no input and no
    /// transition running.
    IdleRebuild { id: String, count: usize },
    /// `id` takes keyboard focus but has no accessible name.
    Unnamed { id: String },
    /// The pointer at `id`'s centre reaches `by` instead, or nothing when a
    /// clip hides the centre.
    Occluded { id: String, by: Option<String> },
    /// The tree did not resolve at this size.
    Failed { error: String },
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Size { width, height } = self.size;
        write!(f, "at {width}x{height}: ")?;
        match &self.fault {
            Fault::Escapes { id, parent, by } => write!(f, "{id} escapes {parent} by {by:.1}"),
            Fault::FloorViolated { id, size, floor } => write!(
                f,
                "{} is {:.1}x{:.1}, below its floor {:.1}x{:.1}",
                if id.is_empty() { "the window" } else { id },
                size.width,
                size.height,
                floor.width,
                floor.height
            ),
            Fault::IdleRebuild { id, count } => {
                write!(f, "{id} repainted on {count} of {IDLE} idle frames")
            }
            Fault::Unnamed { id } => write!(f, "{id} is focusable but has no name"),
            Fault::Occluded { id, by: Some(by) } => write!(f, "{id} is covered by {by}"),
            Fault::Occluded { id, by: None } => write!(f, "{id} is clipped out of reach"),
            Fault::Failed { error } => write!(f, "did not resolve: {error}"),
        }
    }
}

pub use mui_layout::lint::Lint;

/// Sizes in `root` written by hand where intrinsic sizing already holds, or
/// that fight it. Static, so it costs nothing to run beside [`audit`]:
///
/// ```
/// use mui::prelude::*;
/// let tree = col![text("Gain").w(Len::Pct(100.)), text("-6 dB").w(48)];
/// assert_eq!(mui::audit::lint(&tree).len(), 2);
/// ```
pub fn lint(root: &El) -> Vec<Lint> {
    mui_layout::lint::lint(root, |e| matches!(e.content, mui_scene::Content::Text(_)))
}

/// Every fault `build` has at each of `sizes`, in size order.
pub fn audit(
    ui: &mut Ui,
    sizes: impl IntoIterator<Item = Size>,
    mut build: impl FnMut(&mut Ui) -> El,
) -> Vec<Finding> {
    let mut out = Vec::new();
    for size in sizes {
        let faults = at_size(ui, size, &mut build).unwrap_or_else(|error| {
            vec![Fault::Failed {
                error: error.to_string(),
            }]
        });
        out.extend(faults.into_iter().map(|fault| Finding { size, fault }));
    }
    out
}

fn at_size(
    ui: &mut Ui,
    size: Size,
    build: &mut impl FnMut(&mut Ui) -> El,
) -> Result<Vec<Fault>, mui_scene::SceneError> {
    let mut frame = |ui: &mut Ui| -> Result<bool, mui_scene::SceneError> {
        let root = build(ui);
        let f = ui.frame(root, Some(size), Input::default(), DT)?;
        Ok(f.animating || f.repaint_after.is_some())
    };
    for _ in 0..SETTLE {
        if !frame(ui)? {
            break;
        }
    }
    // Repaints across idle frames, by surface.
    let mut last = painted(ui.scene().expect("a resolved frame"));
    let mut repaints: HashMap<String, usize> = HashMap::new();
    for _ in 0..IDLE {
        frame(ui)?;
        let now = painted(ui.scene().expect("a resolved frame"));
        for (key, paint) in &now {
            if last.get(key) != Some(paint) {
                *repaints.entry(key.clone()).or_default() += 1;
            }
        }
        last = now;
    }
    let scene = ui.scene().expect("a resolved frame");
    let mut faults = layout_faults(scene);
    let mut idle: Vec<_> = repaints.into_iter().collect();
    idle.sort();
    faults.extend(
        idle.into_iter()
            .map(|(id, count)| Fault::IdleRebuild { id, count }),
    );
    faults.extend(reach_faults(ui, scene));
    Ok(faults)
}

/// Each surface's paint, so two frames can be compared surface by surface.
fn painted(scene: &ResolvedScene) -> HashMap<String, Vec<Painted>> {
    let mut by_key: HashMap<String, Vec<Painted>> = HashMap::new();
    for p in &scene.paint {
        by_key.entry(p.key.to_string()).or_default().push(p.clone());
    }
    by_key
}

fn layout_faults(scene: &ResolvedScene) -> Vec<Fault> {
    let layout = &scene.layout;
    let mut faults = Vec::new();
    let floor = layout.min_size();
    if layout.size.width + SLACK < floor.width || layout.size.height + SLACK < floor.height {
        faults.push(Fault::FloorViolated {
            id: String::new(),
            size: layout.size,
            floor,
        });
    }
    for s in scene.surfaces() {
        let id = s.key.as_str();
        if Id::is_named(id)
            && let Some(floor) = layout.min_size_of(id)
            && (s.frame.size.width + SLACK < floor.width
                || s.frame.size.height + SLACK < floor.height)
        {
            faults.push(Fault::FloorViolated {
                id: id.to_owned(),
                size: s.frame.size,
                floor,
            });
        }
        let Some(parent) = s.parent.as_ref().filter(|_| !s.floating) else {
            continue;
        };
        let Some(outer) = layout.frame(parent) else {
            continue;
        };
        // What shows of it: a clip between it and the parent hides the rest.
        let mut shown = rect(s.frame);
        if let Some(clip) = s.clip {
            shown = shown.intersect(clip);
        }
        if shown.is_zero_area() {
            continue;
        }
        let outer = rect(outer);
        let by = [
            outer.x0 - shown.x0,
            outer.y0 - shown.y0,
            shown.x1 - outer.x1,
            shown.y1 - outer.y1,
        ]
        .into_iter()
        .fold(0., f64::max);
        if by > SLACK {
            faults.push(Fault::Escapes {
                id: id.to_owned(),
                parent: parent.to_string(),
                by,
            });
        }
    }
    faults
}

/// Focusable surfaces a screen reader cannot name or a pointer cannot reach.
fn reach_faults<'a>(ui: &'a Ui, scene: &'a ResolvedScene) -> Vec<Fault> {
    let parents: HashMap<&str, &str> = scene
        .surfaces()
        .filter_map(|s| Some((s.key.as_str(), s.parent.as_deref()?)))
        .collect();
    let inside = |mut key: &'a str, of: &str| {
        while let Some(&p) = parents.get(key) {
            if p == of {
                return true;
            }
            key = p;
        }
        false
    };
    let mut faults = Vec::new();
    for s in scene.surfaces().filter(|s| s.focusable && !s.disabled) {
        let id = s.key.as_str();
        let named = s
            .semantics
            .as_ref()
            .and_then(|m| m.label.as_deref())
            .or(s.text_value.as_deref())
            .is_some_and(|n| !n.trim().is_empty());
        if !named {
            faults.push(Fault::Unnamed { id: id.to_owned() });
        }
        let centre = s.bounds.unwrap_or_else(|| rect(s.frame)).center();
        match ui.target_at(centre) {
            Some(hit) if hit == id || inside(hit, id) => {}
            by => faults.push(Fault::Occluded {
                id: id.to_owned(),
                by: by.map(str::to_owned),
            }),
        }
    }
    faults
}

fn rect(f: mui_layout::Frame) -> Rect {
    Rect::new(f.x, f.y, f.right(), f.bottom())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;

    fn audit_one(build: impl FnMut(&mut Ui) -> El) -> Vec<Fault> {
        audit(&mut Ui::default(), [Size::new(300., 200.)], build)
            .into_iter()
            .map(|f| f.fault)
            .collect()
    }

    #[test]
    fn a_tidy_tree_has_nothing_to_report() {
        let faults = audit_one(|_| {
            row([block(40., 20.)
                .id("ok")
                .focusable()
                .a11y(A11y::Button)
                .named("Ok")])
            .id("bar")
            .pad(8.)
        });
        assert!(faults.is_empty(), "{faults:?}");
    }

    #[test]
    fn a_child_wider_than_its_parent_escapes_it() {
        let faults = audit_one(|_| {
            // The root takes the window; the parent under test is inside it.
            row([row([block(120., 20.).id("wide").shrink(0.)])
                .w(60.)
                .id("narrow")])
        });
        assert!(
            faults.iter().any(|f| matches!(f,
                Fault::Escapes { id, parent, by } if id == "wide" && parent == "narrow" && *by > 50.)),
            "{faults:?}"
        );
    }

    #[test]
    fn a_focusable_without_a_name_is_reported() {
        let faults = audit_one(|_| row([block(40., 20.).id("mute").focusable()]));
        assert!(
            faults.contains(&Fault::Unnamed { id: "mute".into() }),
            "{faults:?}"
        );
    }

    #[test]
    fn a_float_over_a_button_occludes_it() {
        let faults = audit_one(|_| {
            stack([
                block(40., 20.)
                    .id("under")
                    .focusable()
                    .a11y(A11y::Button)
                    .named("Under"),
                block(300., 200.)
                    .id("veil")
                    .focusable()
                    .named("Veil")
                    .float(),
            ])
        });
        assert!(
            faults.contains(&Fault::Occluded {
                id: "under".into(),
                by: Some("veil".into())
            }),
            "{faults:?}"
        );
    }

    #[test]
    fn a_tree_that_never_settles_is_reported() {
        let mut n = 0.;
        let faults = audit_one(move |_| {
            n += 0.01;
            block(40., 20.)
                .fill(Color::oklcha(0.5, 0.1, 30., n % 1.))
                .id("clock")
        });
        assert!(
            faults.iter().any(
                |f| matches!(f, Fault::IdleRebuild { id, count } if id == "clock" && *count == IDLE)
            ),
            "{faults:?}"
        );
    }
}
