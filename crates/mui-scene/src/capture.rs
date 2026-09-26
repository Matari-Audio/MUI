//! Paint-only subtree extraction for transparent product shots and motion layers.
use std::collections::{HashMap, HashSet};

use crate::{El, Layer, ResolvedScene, Size};

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
    if !crate::Id::is_named(key) {
        return Err(CaptureError::UnnamedSurface(key.into()));
    }
    let mut result = root.clone();
    match visit(&mut result, key, size) {
        0 => Err(CaptureError::MissingSurface(key.into())),
        1 => Ok(result),
        _ => Err(CaptureError::DuplicateSurface(key.into())),
    }
}

impl ResolvedScene {
    /// Partition all paint into ordered fragments for lossless reassembly.
    /// Unlike one image per subtree, this preserves interleaved floats, cables
    /// and late strokes. Selected roots must not overlap in authored ancestry.
    /// Each compositing group must belong entirely to one part.
    pub fn capture_layers(&self, roots: &[&str]) -> Result<Vec<CaptureLayer>, CaptureError> {
        // These also validate hierarchy, external paint, and atomic composites.
        let selections = roots
            .iter()
            .map(|root| self.isolate(&[*root]))
            .collect::<Result<Vec<_>, _>>()?;
        self.without(roots)?;
        let structural = |p: &crate::Painted| {
            matches!(
                p.layer,
                Layer::Clip | Layer::Unclip | Layer::Blend { .. } | Layer::Unblend
            )
        };
        let keys: Vec<HashSet<&str>> = selections
            .iter()
            .map(|s| {
                s.paint
                    .iter()
                    .filter(|p| !structural(p))
                    .map(|p| p.key.as_ref())
                    .collect()
            })
            .collect();
        let mut owners = Vec::with_capacity(self.paint.len());
        let mut runs: Vec<Option<usize>> = Vec::new();
        for p in &self.paint {
            if structural(p) {
                owners.push(None);
                continue;
            }
            let mut matching = keys
                .iter()
                .enumerate()
                .filter(|(_, keys)| keys.contains(p.key.as_ref()));
            let owner = matching.next().map(|(i, _)| i);
            if matching.next().is_some() {
                return Err(CaptureError::DuplicateSurface(p.key.to_string()));
            }
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
    pub fn isolate(&self, roots: &[&str]) -> Result<Self, CaptureError> {
        self.capture_selection(roots, false)
    }

    /// Complement of [`Self::isolate`], useful for the stationary background.
    /// The same clipping and compositing restrictions apply.
    pub fn without(&self, roots: &[&str]) -> Result<Self, CaptureError> {
        self.capture_selection(roots, true)
    }

    fn capture_selection(&self, roots: &[&str], invert: bool) -> Result<Self, CaptureError> {
        let roots: HashSet<&str> = roots.iter().copied().collect();
        for root in &roots {
            if self.surface(root).is_none() {
                return Err(CaptureError::MissingSurface((*root).into()));
            }
            if !crate::Id::is_named(root) {
                return Err(CaptureError::UnnamedSurface((*root).into()));
            }
        }
        let mut selected = HashMap::new();
        for surface in self.surfaces() {
            let mut key = Some(surface.key.as_ref());
            let mut seen = HashSet::new();
            let mut found = false;
            while let Some(id) = key {
                if !seen.insert(id) {
                    return Err(CaptureError::InvalidHierarchy(id.into()));
                }
                found |= roots.contains(id);
                key = self
                    .surface(id)
                    .ok_or_else(|| CaptureError::InvalidHierarchy(id.into()))?
                    .parent
                    .as_deref();
            }
            selected.insert(surface.key.as_ref(), found != invert);
        }
        // Validate before cloning: a partially extracted opacity or mask group
        // cannot in general be recomposited into the original image.
        let mut stack: Vec<(bool, &str, u8)> = Vec::new();
        for p in &self.paint {
            match p.layer {
                Layer::External => return Err(CaptureError::ExternalMaterial),
                Layer::Clip => stack.push((false, &p.key, 0)),
                Layer::Blend { mix, .. } => {
                    if mix != crate::Mix::Normal {
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
        let mut result = self.clone();
        result.memos.clear();
        result.paint.retain(|p| {
            matches!(
                p.layer,
                Layer::Clip | Layer::Unclip | Layer::Blend { .. } | Layer::Unblend
            ) || selected[p.key.as_ref()]
        });
        Ok(result)
    }
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
                *mix = crate::Mix::Multiply;
            }
        }
        assert!(matches!(
            backdrop.capture_layers(&["group"]),
            Err(CaptureError::BackdropBlend)
        ));
    }
}
