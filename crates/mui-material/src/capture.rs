//! Paint-only subtree extraction for transparent product shots and motion layers.
use std::collections::{HashMap, HashSet};

use mui_scene::{El, Layer, ResolvedScene, Size};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CaptureError {
    MissingSurface(String),
    UnnamedSurface(String),
    InvalidHierarchy(String),
    SplitComposite(String),
    ExternalMaterial,
    BackdropBlend,
    InvalidStack,
    InvalidSize,
    DuplicateSurface(String),
}

impl std::fmt::Display for CaptureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cannot extract scene paint: {self:?}")
    }
}
impl std::error::Error for CaptureError {}

/// One contiguous painter-order fragment. Several fragments can belong to the
/// same named part (for example its deferred floating sockets). Animate them
/// with the same pose and scene-space pivot, but retain their returned order.
#[derive(Clone, Debug)]
pub struct CaptureLayer {
    /// None is stationary paint outside the selected parts.
    pub part: Option<String>,
    pub scene: ResolvedScene,
}

impl CaptureLayer {
    /// This part's paint pulled out of its ancestors: without the clips of
    /// the surfaces above its root (a panel's rounded clip, the window's),
    /// so it can move away from them uncut. Its own clips and its
    /// descendants' stay. `None` for stationary paint, and when no ancestor
    /// clip wraps it (the captured scene is already free).
    pub fn free(&self) -> Option<ResolvedScene> {
        let part = self.part.as_deref()?;
        let mut ancestors = HashSet::new();
        let mut id = self.scene.surface(part).and_then(|s| s.parent.as_deref());
        while let Some(k) = id {
            if !ancestors.insert(k) {
                break;
            }
            id = self.scene.surface(k).and_then(|s| s.parent.as_deref());
        }
        // Clip/Unclip pairs nest: an Unclip closes the latest open Clip.
        let mut open = Vec::new();
        let mut drop = vec![false; self.scene.paint.len()];
        for (i, p) in self.scene.paint.iter().enumerate() {
            match p.layer {
                Layer::Clip => {
                    let d = ancestors.contains(p.key.as_ref());
                    open.push(d);
                    drop[i] = d;
                }
                Layer::Unclip => drop[i] = open.pop().unwrap_or(false),
                _ => {}
            }
        }
        if !drop.contains(&true) {
            return None;
        }
        let mut scene = self.scene.clone();
        scene.forget_memos();
        let mut i = 0;
        scene.paint.retain(|_| {
            i += 1;
            !drop[i - 1]
        });
        Some(scene)
    }
}

/// Clone an authored tree and offer a named part a new size before resolution.
/// Text, strokes and corner radii retain their logical sizes; children relayout.
/// Normal intrinsic/min/max constraints still apply. This does not mutate the
/// plugin's live tree or state. Resolve the result before extracting its paint.
pub fn resize_capture(root: &El, key: &str, size: Size) -> Result<El, CaptureError> {
    fn visit(node: &mut El, key: &str, size: Size) -> usize {
        let own = usize::from(node.key() == Some(key));
        if own == 1 {
            *node = node.clone().size(size.width, size.height);
        }
        own + node
            .children_mut()
            .iter_mut()
            .map(|n| visit(n, key, size))
            .sum::<usize>()
    }
    if !size.width.is_finite() || !size.height.is_finite() || size.width <= 0. || size.height <= 0.
    {
        return Err(CaptureError::InvalidSize);
    }
    if !mui_scene::Id::is_named(key) {
        return Err(CaptureError::UnnamedSurface(key.into()));
    }
    let mut result = root.clone();
    match visit(&mut result, key, size) {
        0 => Err(CaptureError::MissingSurface(key.into())),
        1 => Ok(result),
        _ => Err(CaptureError::DuplicateSurface(key.into())),
    }
}

/// Paint-only extraction of named subtrees from a resolved scene.
pub trait Capture: Sized {
    /// See the impl on [`ResolvedScene`].
    fn capture_layers(&self, roots: &[&str]) -> Result<Vec<CaptureLayer>, CaptureError> {
        self.capture_layers_adopting(roots, &[])
    }
    /// See the impl on [`ResolvedScene`].
    fn capture_layers_adopting(
        &self,
        roots: &[&str],
        adopt: &[(&str, &str)],
    ) -> Result<Vec<CaptureLayer>, CaptureError>;
    /// See the impl on [`ResolvedScene`].
    fn isolate(&self, roots: &[&str]) -> Result<Self, CaptureError>;
    /// Complement of [`Capture::isolate`].
    fn without(&self, roots: &[&str]) -> Result<Self, CaptureError>;
}

impl Capture for ResolvedScene {
    /// Partition all paint into ordered fragments for lossless reassembly.
    /// Unlike one image per subtree, this preserves interleaved floats, cables
    /// and late strokes. Roots may nest (a panel and its knobs): paint belongs
    /// to its nearest selected ancestor, so a panel's fragments are what is
    /// left of it with its selected children taken out, whole, never cropped.
    /// Each compositing group must belong entirely to one part.
    fn capture_layers(&self, roots: &[&str]) -> Result<Vec<CaptureLayer>, CaptureError> {
        self.capture_layers_adopting(roots, &[])
    }

    /// [`Capture::capture_layers`] with some surfaces adopted: `(surface,
    /// parent)` makes a surface belong to a named surface as if it were its
    /// child. For a widget whose name sits on one inner block (a knob's
    /// dial) while its pointer and caption are unnamed blocks beside it.
    fn capture_layers_adopting(
        &self,
        roots: &[&str],
        adopt: &[(&str, &str)],
    ) -> Result<Vec<CaptureLayer>, CaptureError> {
        let adopt: HashMap<&str, &str> = adopt.iter().copied().collect();
        // These also validate hierarchy, external paint, and atomic composites.
        for root in roots {
            selection(self, &[*root], false, &adopt)?;
        }
        selection(self, roots, true, &adopt)?;
        let structural = |p: &mui_scene::Painted| {
            matches!(
                p.layer,
                Layer::Clip | Layer::Unclip | Layer::Blend { .. } | Layer::Unblend
            )
        };
        // The first mention of a root names it; the hierarchy is acyclic
        // (checked above), so the walk up ends.
        let mut index = HashMap::new();
        for (i, root) in roots.iter().enumerate() {
            index.entry(*root).or_insert(i);
        }
        let owner_of = |key: &str| {
            let mut id = Some(key);
            while let Some(k) = id {
                if let Some(&i) = index.get(k) {
                    return Some(i);
                }
                id = parent_of(self, &adopt, k);
            }
            None
        };
        let mut owners = Vec::with_capacity(self.paint.len());
        let mut runs: Vec<Option<usize>> = Vec::new();
        for p in &self.paint {
            if structural(p) {
                owners.push(None);
                continue;
            }
            let owner = owner_of(&p.key);
            let run = match runs.last_mut() {
                Some(last) if *last == owner => runs.len() - 1,
                _ => {
                    runs.push(owner);
                    runs.len() - 1
                }
            };
            owners.push(Some(run));
        }
        Ok(runs
            .into_iter()
            .enumerate()
            .map(|(run, owner)| {
                let mut scene = self.clone();
                let mut index = 0;
                scene.paint.retain(|p| {
                    let keep = structural(p) || owners[index] == Some(run);
                    index += 1;
                    keep
                });
                CaptureLayer {
                    part: owner.map(|i| roots[i].to_owned()),
                    scene,
                }
            })
            .collect())
    }

    /// Keep paint belonging to these explicitly named subtrees, on transparency.
    /// Uses authored ancestry, never rectangular cropping or ID-prefix matching.
    /// Ancestor clips remain in effect; shadows can extend outside surface frames.
    ///
    /// This is a paint-only snapshot: layout, surfaces and hit metadata are retained
    /// unchanged. Do not use it as an interactive scene. Rebuild the original tree
    /// at a new offered size for layout resizing; scaling this snapshot stretches it.
    /// Fused material plates are atomic: select their owning welded surface.
    /// Splitting a blend/mask group is rejected, as are backdrop-dependent blend modes and external GPU materials.
    fn isolate(&self, roots: &[&str]) -> Result<Self, CaptureError> {
        selection(self, roots, false, &HashMap::new())
    }

    /// Complement of [`Capture::isolate`], useful for the stationary background.
    /// The same clipping and compositing restrictions apply.
    fn without(&self, roots: &[&str]) -> Result<Self, CaptureError> {
        selection(self, roots, true, &HashMap::new())
    }
}

/// A surface's parent: its adopter, or its nearest named ancestor.
fn parent_of<'s>(
    scene: &'s ResolvedScene,
    adopt: &HashMap<&str, &'s str>,
    id: &str,
) -> Option<&'s str> {
    adopt
        .get(id)
        .copied()
        .or_else(|| scene.surface(id).and_then(|s| s.parent.as_deref()))
}

fn selection(
    scene: &ResolvedScene,
    roots: &[&str],
    invert: bool,
    adopt: &HashMap<&str, &str>,
) -> Result<ResolvedScene, CaptureError> {
    let roots: HashSet<&str> = roots.iter().copied().collect();
    for root in &roots {
        if scene.surface(root).is_none() {
            return Err(CaptureError::MissingSurface((*root).into()));
        }
        if !mui_scene::Id::is_named(root) {
            return Err(CaptureError::UnnamedSurface((*root).into()));
        }
    }
    let mut selected = HashMap::new();
    for surface in scene.surfaces() {
        let mut key = Some(surface.key.as_ref());
        let mut seen = HashSet::new();
        let mut found = false;
        while let Some(id) = key {
            if !seen.insert(id) {
                return Err(CaptureError::InvalidHierarchy(id.into()));
            }
            found |= roots.contains(id);
            if scene.surface(id).is_none() {
                return Err(CaptureError::InvalidHierarchy(id.into()));
            }
            key = parent_of(scene, adopt, id);
        }
        selected.insert(surface.key.as_ref(), found != invert);
    }
    // Validate before cloning: a partially extracted opacity or mask group
    // cannot in general be recomposited into the original image.
    let mut stack: Vec<(bool, &str, u8)> = Vec::new();
    for p in &scene.paint {
        match p.layer {
            Layer::External => return Err(CaptureError::ExternalMaterial),
            Layer::Clip => stack.push((false, &p.key, 0)),
            Layer::Blend { mix, .. } => {
                if mix != mui_scene::Mix::Normal {
                    return Err(CaptureError::BackdropBlend);
                }
                stack.push((true, &p.key, 0));
            }
            Layer::Unclip | Layer::Unblend => {
                let (blend, key, bits) = stack.pop().ok_or(CaptureError::InvalidStack)?;
                if blend != matches!(p.layer, Layer::Unblend) {
                    return Err(CaptureError::InvalidStack);
                }
                if blend && bits == 3 {
                    return Err(CaptureError::SplitComposite(key.into()));
                }
            }
            _ => {
                let keep = *selected
                    .get(p.key.as_ref())
                    .ok_or_else(|| CaptureError::MissingSurface(p.key.to_string()))?;
                for (_, _, bits) in &mut stack {
                    *bits |= if keep { 1 } else { 2 };
                }
            }
        }
    }
    if !stack.is_empty() {
        return Err(CaptureError::InvalidStack);
    }
    let mut result = scene.clone();
    result.forget_memos();
    result.paint.retain(|p| {
        matches!(
            p.layer,
            Layer::Clip | Layer::Unclip | Layer::Blend { .. } | Layer::Unblend
        ) || selected[p.key.as_ref()]
    });
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prelude::*;

    #[test]
    fn resizing_reflows_children_without_stretching_their_geometry() {
        let tree = row![
            block(20., 20.).id("fixed"),
            block(20., 20.).grow(1.).id("fluid")
        ]
        .size(100., 40.)
        .id("panel");
        let resized = resize_capture(&tree, "panel", Size::new(200., 40.)).unwrap();
        let before = resolve(&SceneSpec::new(tree)).unwrap();
        let after = resolve(&SceneSpec::new(resized)).unwrap();
        assert_eq!(before.surface("panel").unwrap().frame.size.width, 100.);
        assert_eq!(after.surface("panel").unwrap().frame.size.width, 200.);
        assert_eq!(after.surface("fixed").unwrap().frame.size.width, 20.);
        assert_eq!(after.surface("fluid").unwrap().frame.size.width, 180.);
        assert!(matches!(
            resize_capture(&block(1., 1.), "x", Size::new(f64::NAN, 1.)),
            Err(CaptureError::InvalidSize)
        ));
    }

    #[test]
    fn extraction_follows_ancestry_and_keeps_clip_pairs() {
        let tree = row![
            col([
                block(20., 20.).fill(Role::Primary).id("unrelated-name"),
                text("label")
            ])
            .id("card")
            .size(50., 50.)
            .clip(),
            block(20., 20.).fill(Role::Danger).id("card/sibling")
        ]
        .id("root")
        .size(100., 60.)
        .clip();
        let s = resolve(&SceneSpec::new(tree)).unwrap();
        let isolated = s.isolate(&["card"]).unwrap();
        assert!(isolated.paint.iter().any(|p| &*p.key == "unrelated-name"));
        assert!(!isolated.paint.iter().any(|p| &*p.key == "card/sibling"));
        assert_eq!(
            isolated
                .paint
                .iter()
                .filter(|p| p.layer == Layer::Clip)
                .count(),
            2
        );
        assert_eq!(
            isolated
                .paint
                .iter()
                .filter(|p| p.layer == Layer::Unclip)
                .count(),
            2
        );
        let rest = s.without(&["card"]).unwrap();
        assert!(!rest.paint.iter().any(|p| &*p.key == "unrelated-name"));
        assert!(rest.paint.iter().any(|p| &*p.key == "card/sibling"));
        assert!(matches!(
            s.isolate(&["missing"]),
            Err(CaptureError::MissingSurface(_))
        ));
        assert_eq!(s.isolate(&["card", "card"]).unwrap(), isolated);
    }

    #[test]
    fn nested_roots_leave_the_parent_whole_and_free_the_child_of_its_clips() {
        // A clipped panel whose knob hangs half out of it.
        let tree = stack([stack([
            block(60., 40.).fill(Role::Surface).id("panel-bg"),
            block(20., 20.)
                .fill(Role::Primary)
                .offset(50., 10.)
                .float()
                .id("knob"),
        ])
        .size(60., 40.)
        .clip()
        .id("panel")])
        .size(120., 80.)
        .clip()
        .id("root");
        let s = resolve(&SceneSpec::new(tree)).unwrap();
        let layers = s.capture_layers(&["panel", "knob"]).unwrap();
        let owners = |key: &str| -> Vec<Option<&str>> {
            layers
                .iter()
                .filter(|l| {
                    l.scene
                        .paint
                        .iter()
                        .any(|p| &*p.key == key && p.layer == Layer::Fill)
                })
                .map(|l| l.part.as_deref())
                .collect()
        };
        // Paint goes to its nearest selected ancestor, once.
        assert_eq!(owners("knob"), [Some("knob")]);
        assert_eq!(owners("panel-bg"), [Some("panel")]);
        // Taking the knob out leaves the panel's own paint as it was.
        let panel: Vec<_> = s
            .without(&["knob"])
            .unwrap()
            .paint
            .into_iter()
            .filter(|p| p.layer == Layer::Fill && &*p.key != "root")
            .collect();
        let left: Vec<_> = layers
            .iter()
            .filter(|l| l.part.as_deref() == Some("panel"))
            .flat_map(|l| l.scene.paint.iter().filter(|p| p.layer == Layer::Fill))
            .cloned()
            .collect();
        assert_eq!(left, panel);
        // Pulled out, the knob loses the panel's and the root's clips.
        let knob = layers
            .iter()
            .find(|l| l.part.as_deref() == Some("knob"))
            .unwrap();
        let clips = |s: &ResolvedScene| -> Vec<String> {
            s.paint
                .iter()
                .filter(|p| p.layer == Layer::Clip)
                .map(|p| p.key.to_string())
                .collect()
        };
        assert!(clips(&knob.scene).contains(&"panel".to_owned()));
        let free = knob.free().unwrap();
        assert!(clips(&free).is_empty(), "{:?}", clips(&free));
        let pairs = |s: &ResolvedScene, l| s.paint.iter().filter(|p| p.layer == l).count();
        assert_eq!(pairs(&free, Layer::Clip), pairs(&free, Layer::Unclip));
        // The panel keeps its own clip (it clips its own paint), losing the root's.
        let panel = layers
            .iter()
            .find(|l| l.part.as_deref() == Some("panel"))
            .unwrap();
        assert_eq!(clips(&panel.free().unwrap()), ["panel"]);
        // Stationary paint has nothing to be freed from.
        assert!(
            layers
                .iter()
                .filter(|l| l.part.is_none())
                .all(|l| l.free().is_none())
        );
    }

    #[test]
    fn adopted_surfaces_go_with_their_adopter() {
        // A knob's name on its dial; its caption an unnamed block beside it.
        let tree = row![
            col([
                block(20., 20.).fill(Role::Primary).id("dial"),
                block(20., 4.).fill(Role::Danger)
            ]),
            block(20., 20.).fill(Role::Primary).id("other")
        ]
        .id("panel");
        let s = resolve(&SceneSpec::new(tree)).unwrap();
        let caption = s
            .paint
            .iter()
            .find(|p| !mui_scene::Id::is_named(&p.key))
            .map(|p| p.key.to_string())
            .unwrap();
        let owner = |layers: &[CaptureLayer]| {
            layers
                .iter()
                .find(|l| l.scene.paint.iter().any(|p| p.key.as_ref() == caption))
                .and_then(|l| l.part.clone())
        };
        let roots = ["panel", "dial"];
        assert_eq!(
            owner(&s.capture_layers(&roots).unwrap()).as_deref(),
            Some("panel")
        );
        let adopted = s
            .capture_layers_adopting(&roots, &[(caption.as_str(), "dial")])
            .unwrap();
        assert_eq!(owner(&adopted).as_deref(), Some("dial"));
    }

    #[test]
    fn composites_must_be_extracted_atomically() {
        let s = resolve(&SceneSpec::new(
            row![
                block(20., 20.).fill(Role::Primary).id("a"),
                block(20., 20.).fill(Role::Danger).id("b")
            ]
            .id("group")
            .opacity(0.5),
        ))
        .unwrap();
        assert!(matches!(
            s.isolate(&["a"]),
            Err(CaptureError::SplitComposite(_))
        ));
        assert!(matches!(
            s.without(&["a"]),
            Err(CaptureError::SplitComposite(_))
        ));
        assert!(s.isolate(&["group"]).is_ok());
        assert!(s.isolate(&["a", "b"]).is_ok());
        let mut backdrop = s;
        for paint in &mut backdrop.paint {
            if let Layer::Blend { mix, .. } = &mut paint.layer {
                *mix = mui_scene::Mix::Multiply;
            }
        }
        assert!(matches!(
            backdrop.capture_layers(&["group"]),
            Err(CaptureError::BackdropBlend)
        ));
    }
}
