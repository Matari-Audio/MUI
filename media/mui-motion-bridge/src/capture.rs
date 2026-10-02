use base64::Engine;
use mui::material::Capture;
type Raster = Option<([usize; 4], String)>;
/// A fragment as made (a PNG, or its paint as a scene) and its rect
/// `[x, y, w, h]` in the UI's points.
type Made<T> = Option<([f64; 4], T)>;
struct CachedFragment<T> {
    key: (u16, u16, f64),
    paint: Vec<mui_scene::Painted>,
    made: Made<T>,
    /// The part pulled out of its ancestors' clips, when that differs.
    free: Made<T>,
}
pub fn capture_frame(
    scene: &mui_scene::ResolvedScene,
    width: u16,
    height: u16,
    scale: f64,
    roots: &[String],
) -> Result<serde_json::Value, String> {
    capture_cached(scene, width, height, scale, roots, &mut Vec::new())
}
fn raster(
    scene: &mui_scene::ResolvedScene,
    width: u16,
    height: u16,
    scale: f64,
) -> Result<Raster, String> {
    let mut ctx = RenderContext::new(width, height);
    let mut res = Resources::default();
    mui_vello::paint(
        &mut mui_vello::Cpu {
            ctx: &mut ctx,
            resources: &mut res,
            cache: &mut mui_vello::Cache::default(),
        },
        scene,
        mui_vello::kurbo::Affine::scale(scale),
    )
    .map_err(|e| e.to_string())?;
    ctx.flush();
    let mut pix = Pixmap::new(width, height);
    ctx.render(&mut pix, &mut res);
    let pixels = pix.take_unpremultiplied();
    let (mut x0, mut y0, mut x1, mut y1) = (usize::from(width), usize::from(height), 0, 0);
    for (i, _) in pixels.iter().enumerate().filter(|(_, p)| p.a != 0) {
        let (x, y) = (i % usize::from(width), i / usize::from(width));
        x0 = x0.min(x);
        y0 = y0.min(y);
        x1 = x1.max(x + 1);
        y1 = y1.max(y + 1);
    }
    if x1 == 0 {
        return Ok(None);
    }
    let rgba: Vec<u8> = (y0..y1)
        .flat_map(|y| {
            pixels[y * usize::from(width) + x0..y * usize::from(width) + x1]
                .iter()
                .flat_map(|p| [p.r, p.g, p.b, p.a])
        })
        .collect();
    let mut bytes = Vec::new();
    let mut encoder = png::Encoder::new(&mut bytes, (x1 - x0) as u32, (y1 - y0) as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&rgba))
        .map_err(|e| e.to_string())?;
    Ok(Some((
        [x0, y0, x1, y1],
        base64::engine::general_purpose::STANDARD.encode(bytes),
    )))
}
use mui_vello::vello_cpu::{Pixmap, RenderContext, Resources};
fn capture_cached(
    scene: &mui_scene::ResolvedScene,
    width: u16,
    height: u16,
    scale: f64,
    roots: &[String],
    cache: &mut Vec<CachedFragment<String>>,
) -> Result<serde_json::Value, String> {
    if width == 0 || height == 0 || !scale.is_finite() || scale <= 0. {
        return Err("capture dimensions and scale must be positive and finite".into());
    }
    let mut images = serde_json::Map::new();
    let size = [f64::from(width) / scale, f64::from(height) / scale];
    let manifest = capture_parts(
        scene,
        size,
        scale,
        (width, height, scale),
        roots,
        cache,
        |s| {
            Ok(raster(s, width, height, scale)?.map(|([x0, y0, x1, y1], data)| {
                let px = |v: usize| v as f64 / scale;
                ([px(x0), px(y0), px(x1 - x0), px(y1 - y0)], data)
            }))
        },
        |index, free, data| {
            let name = if free {
                format!("layer-{index:02}-free.png")
            } else {
                format!("layer-{index:02}.png")
            };
            images.insert(name.clone(), serde_json::Value::String(data.clone()));
            name
        },
    )?;
    Ok(serde_json::json!({"scene":manifest,"images":images}))
}

/// The capture manifest of `scene` split at `roots`: every fragment made
/// by `make` (unless `cache` has its paint already) and named by `name`
/// (its index, whether it is the free copy, what `make` made).
#[expect(clippy::too_many_arguments, reason = "one body for both kinds of capture")]
fn capture_parts<T: Clone + PartialEq>(
    scene: &mui_scene::ResolvedScene,
    size: [f64; 2],
    scale: f64,
    key: (u16, u16, f64),
    roots: &[String],
    cache: &mut Vec<CachedFragment<T>>,
    mut make: impl FnMut(&mui_scene::ResolvedScene) -> Result<Made<T>, String>,
    mut name: impl FnMut(usize, bool, &T) -> String,
) -> Result<serde_json::Value, String> {
    let refs: Vec<&str> = roots.iter().map(String::as_str).collect();
    let surfaces: Vec<_> = scene.surfaces().map(|s| serde_json::json!({"id":s.key.as_ref(),"parent":s.parent.as_deref(),"frame":[s.frame.x,s.frame.y,s.frame.size.width,s.frame.size.height]})).collect();
    let tree = part_tree(scene, roots);
    let (adopt, widget) = widgets(scene, roots);
    let adopt: Vec<(&str, &str)> = adopt
        .iter()
        .map(|(a, b)| (a.as_str(), b.as_str()))
        .collect();
    let path_of = |id: &str| tree.iter().find(|p| p.id == id).map(|p| p.path.clone());
    let mut layers = Vec::new();
    let mut fragment_count = 0;
    for (index, fragment) in scene
        .capture_layers_adopting(&refs, &adopt)
        .map_err(|e| e.to_string())?
        .into_iter()
        .enumerate()
    {
        fragment_count = index + 1;
        let free = fragment.free();
        let id = fragment.part.as_ref();
        let isolated = fragment.scene;
        let cached = cache
            .get(index)
            .filter(|c| c.key == key && c.paint == isolated.paint);
        let (data, loose) = if let Some(c) = cached {
            (c.made.clone(), c.free.clone())
        } else {
            let data = make(&isolated)?;
            let loose = match free {
                Some(f) => make(&f)?.filter(|f| Some(f) != data.as_ref()),
                None => None,
            };
            (data, loose)
        };
        let entry = CachedFragment {
            key,
            paint: isolated.paint,
            made: data.clone(),
            free: loose.clone(),
        };
        if index < cache.len() {
            cache[index] = entry;
        } else {
            cache.push(entry);
        }
        let Some((rect, data)) = data else {
            continue;
        };
        let src = name(index, false, &data);
        let origin = id.and_then(|id| scene.surface(id)).map_or(
            [size[0] / 2., size[1] / 2.],
            |s| {
                [
                    s.frame.x + s.frame.size.width / 2.,
                    s.frame.y + s.frame.size.height / 2.,
                ]
            },
        );
        let node = id.and_then(|id| tree.iter().find(|p| &p.id == id));
        let mut layer = serde_json::json!({"id":format!("fragment-{index}"),"group":node.map_or("background",|n| n.path.as_str()),"origin":origin,"src":src,"rect":rect});
        if let Some(n) = node {
            layer["part"] = n.id.clone().into();
            layer["parent"] = n.parent.as_deref().and_then(path_of).into();
        }
        if let Some((r, data)) = loose {
            layer["free"] = serde_json::json!({"src": name(index, true, &data), "rect": r});
        }
        layers.push(layer);
    }
    cache.truncate(fragment_count);
    let parts: Vec<_> = tree
        .iter()
        .map(|p| {
            let f = widget.get(&p.id).copied().or_else(|| scene.surface(&p.id).map(|s| s.frame));
            serde_json::json!({"path": p.path, "id": p.id, "parent": p.parent.as_deref().and_then(path_of),
                "frame": f.map(|f| [f.x, f.y, f.size.width, f.size.height])})
        })
        .collect();
    Ok(serde_json::json!({"version":1,"width":size[0],"height":size[1],"scale":scale,"layers":layers,"groups":roots,"parts":parts,"surfaces":surfaces}))
}

/// Offline capture uses exactly the same pixels as the live in-memory stream.
pub fn capture(
    scene: &mui_scene::ResolvedScene,
    width: u16,
    height: u16,
    scale: f64,
    directory: &std::path::Path,
    roots: &[String],
) -> Result<(), String> {
    use base64::Engine;
    let frame = capture_frame(scene, width, height, scale, roots)?;
    std::fs::create_dir_all(directory).map_err(|e| e.to_string())?;
    for (name, data) in frame["images"].as_object().unwrap() {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data.as_str().unwrap())
            .map_err(|e| e.to_string())?;
        std::fs::write(directory.join(name), bytes).map_err(|e| e.to_string())?;
    }
    std::fs::write(
        directory.join("scene.json"),
        serde_json::to_vec_pretty(&frame["scene"]).unwrap(),
    )
    .map_err(|e| e.to_string())
}

/// Content-addressed, changed-texture transport. Only the current frame's
/// textures are retained; a static surface is not sent again on every tick.
#[derive(Default)]
pub struct CaptureStream {
    previous: std::collections::HashSet<String>,
    cache: Vec<CachedFragment<String>>,
}
impl CaptureStream {
    pub fn frame(
        &mut self,
        scene: &mui_scene::ResolvedScene,
        width: u16,
        height: u16,
        scale: f64,
        roots: &[String],
    ) -> Result<serde_json::Value, String> {
        use std::hash::{DefaultHasher, Hash, Hasher};
        let mut frame = capture_cached(scene, width, height, scale, roots, &mut self.cache)?;
        let source = frame["images"].take();
        let mut images = serde_json::Map::new();
        let mut current = std::collections::HashSet::new();
        let mut rename = |src: &mut serde_json::Value| {
            let data = source[src.as_str().unwrap()].as_str().unwrap();
            // ponytail: 64-bit std hash, not a digest; a collision in one
            // session would reuse a stale texture. Names never leave the session.
            let mut hash = DefaultHasher::new();
            data.hash(&mut hash);
            let name = format!("{:016x}-{}.png", hash.finish(), data.len());
            if !self.previous.contains(&name) {
                images.insert(name.clone(), serde_json::Value::String(data.into()));
            }
            current.insert(name.clone());
            *src = serde_json::Value::String(name);
        };
        for layer in frame["scene"]["layers"].as_array_mut().unwrap() {
            rename(&mut layer["src"]);
            if layer["free"].is_object() {
                rename(&mut layer["free"]["src"]);
            }
        }
        self.previous = current;
        frame["images"] = serde_json::Value::Object(images);
        Ok(frame)
    }
}

/// A fragment's paint as a scene drawn at its rect's origin, and the name
/// it was given when made. Equal by paint alone.
#[derive(Clone, Debug)]
pub struct Vector {
    pub scene: std::sync::Arc<mui_scene::ResolvedScene>,
    id: u64,
}
impl PartialEq for Vector {
    fn eq(&self, o: &Self) -> bool {
        self.scene == o.scene
    }
}
impl Vector {
    /// `v<id>.mui`: made once, so a part whose paint did not change keeps
    /// its name (and a renderer its texture).
    pub fn name(&self) -> String {
        format!("v{}.mui", self.id)
    }
}

/// [`CaptureStream`] without pixels: each fragment is its paint, moved to
/// its rect's origin, for a renderer in the same process to draw at
/// whatever size it needs.
#[derive(Default)]
pub struct VectorStream {
    cache: Vec<CachedFragment<Vector>>,
    made: u64,
}
impl VectorStream {
    /// The manifest of `scene`, `size` points, split at `roots` (as
    /// [`capture_frame`]'s `scene`, `src` naming a [`Vector`]), and every
    /// fragment it names.
    pub fn frame(
        &mut self,
        scene: &mui_scene::ResolvedScene,
        size: mui_scene::Size,
        roots: &[String],
    ) -> Result<(serde_json::Value, Vec<Vector>), String> {
        let made = &mut self.made;
        let mut used = Vec::new();
        let manifest = capture_parts(
            scene,
            [size.width, size.height],
            1.,
            (0, 0, 1.),
            roots,
            &mut self.cache,
            |s| {
                Ok(bounds(s).map(|r| {
                    *made += 1;
                    let scene = std::sync::Arc::new(shifted(s.clone(), -r.origin().to_vec2()));
                    ([r.x0, r.y0, r.width(), r.height()], Vector { scene, id: *made })
                }))
            },
            |_, _, v| {
                used.push(v.clone());
                v.name()
            },
        )?;
        Ok((manifest, used))
    }
}

/// Where `scene` paints, clipped as it clips: every outline (strokes and
/// blurs grown by their reach, glyph runs by their size), each cut to the
/// clips open around it. `None` paints nothing.
pub fn bounds(scene: &mui_scene::ResolvedScene) -> Option<mui_geometry::Rect> {
    use mui_geometry::PathCommand as C;
    use mui_geometry::Rect;
    use mui_scene::Layer;
    let outline = |p: &mui_scene::Painted| {
        let o = p.offset.to_vec2();
        let pts = p.path.commands.iter().flat_map(|c| match *c {
            C::MoveTo(a) | C::LineTo(a) => vec![a],
            C::CubicTo(a, b, c) => vec![a, b, c],
            C::ArcTo(a) => {
                let r = mui_geometry::Vec2::new(a.radius, a.radius);
                vec![a.center - r, a.center + r]
            }
            C::Close => vec![],
        });
        mui_geometry::bounds(pts.map(|q| q + o))
    };
    let mut clips: Vec<Option<Rect>> = Vec::new();
    let mut out: Option<Rect> = None;
    for p in &scene.paint {
        let open = clips.last().copied().flatten();
        match p.layer {
            Layer::Clip => {
                let b = outline(p).unwrap_or(Rect::ZERO);
                clips.push(Some(open.map_or(b, |c| c.intersect(b))));
                continue;
            }
            Layer::Unclip => {
                clips.pop();
                continue;
            }
            Layer::Blend { .. } | Layer::Unblend => continue,
            _ => {}
        }
        let b = match &p.text {
            Some(t) => {
                let size = f64::from(t.size);
                mui_geometry::bounds(t.glyphs.iter().flat_map(|g| {
                    let at = t.origin + mui_geometry::Vec2::new(f64::from(g.x), f64::from(g.y));
                    [at + mui_geometry::Vec2::new(-size * 0.2, -size * 1.2), at + mui_geometry::Vec2::new(size * 1.2, size * 0.4)]
                }))
            }
            None => outline(p),
        };
        let Some(b) = b else { continue };
        // A stroke reaches half its width out, a blur about three radii;
        // a pixel more for anti-aliasing.
        let reach = p.width / 2. + 3. * p.blur + 1.;
        let mut b = b.inflate(reach, reach);
        if let Some(c) = open {
            b = b.intersect(c);
        }
        if b.width() <= 0. || b.height() <= 0. {
            continue;
        }
        out = Some(out.map_or(b, |o| o.union(b)));
    }
    out
}

/// `scene` moved by `d`: every entry's offset, and a glyph run's origin
/// (which stands in scene space with the offset at zero).
pub fn shifted(mut scene: mui_scene::ResolvedScene, d: mui_geometry::Vec2) -> mui_scene::ResolvedScene {
    for p in &mut scene.paint {
        match &mut p.text {
            Some(t) => t.origin += d,
            None => p.offset += d,
        }
    }
    scene
}

/// Discover a useful non-overlapping partition without plugin-specific names.
/// Every native surface remains in the manifest for deeper selection. Large
/// container surfaces are traversed until a panel-sized named surface is found.
pub fn discover_parts(scene: &mui_scene::ResolvedScene, width: f64, height: f64) -> Vec<String> {
    let surfaces: Vec<_> = scene
        .surfaces()
        .filter(|s| mui_scene::Id::is_named(&s.key))
        .collect();
    let candidates: Vec<_> = surfaces
        .iter()
        .filter(|s| {
            // No named ancestor is fine (a plugin whose root is anonymous):
            // the size cap keeps the whole UI out.
            s.frame.size.width >= 50.
                && s.frame.size.height >= 35.
                && s.frame.size.width * s.frame.size.height <= width * height * 0.3
        })
        .collect();
    candidates
        .iter()
        .filter(|s| {
            let mut parent = s.parent.as_deref();
            while let Some(id) = parent {
                if candidates.iter().any(|p| p.key.as_ref() == id) {
                    return false;
                }
                parent = surfaces
                    .iter()
                    .find(|p| p.key.as_ref() == id)
                    .and_then(|p| p.parent.as_deref());
            }
            true
        })
        .map(|s| s.key.to_string())
        .collect()
}

/// One selected root: its surface id, its selected parent's id, and its
/// path, the ids from the outermost selected ancestor down joined by `/`.
struct PartNode {
    id: String,
    parent: Option<String>,
    path: String,
}

/// The selected roots as a tree, parents before children (then in the
/// order given).
fn part_tree(scene: &mui_scene::ResolvedScene, roots: &[String]) -> Vec<PartNode> {
    let mut nodes: Vec<(usize, PartNode)> = Vec::new();
    for (i, root) in roots.iter().enumerate() {
        if nodes.iter().any(|(_, n)| &n.id == root) {
            continue;
        }
        let mut chain = vec![root.clone()];
        let mut up = scene.surface(root).and_then(|s| s.parent.as_deref());
        while let Some(id) = up {
            if roots.iter().any(|r| r == id) {
                chain.push(id.to_owned());
            }
            up = scene.surface(id).and_then(|s| s.parent.as_deref());
            if chain.len() > roots.len() {
                break; // a cycle; capture_layers reports it
            }
        }
        chain.reverse();
        let depth = chain.len();
        let parent = (depth > 1).then(|| chain[depth - 2].clone());
        nodes.push((
            depth * roots.len() + i,
            PartNode {
                id: root.clone(),
                parent,
                path: chain.join("/"),
            },
        ));
    }
    nodes.sort_by_key(|(k, _)| *k);
    nodes.into_iter().map(|(_, n)| n).collect()
}

/// Each part's widget: the unnamed containers around it that hold no other
/// named surface (a knob's column: its dial, the dial's pointer, its
/// caption), when its name sits on an inner block. Unnamed surfaces are
/// keyed by their tree path (`/1/0/2`), so a container holds what its path
/// prefixes; where a named part sits among them comes from the surfaces'
/// order (a node's surface before its children's). Each unnamed member with
/// the part that adopts it, and each such part's widget frame.
// ponytail: where a named part sits is inferred (surface order, and frames
// when it ends its container); a scene that recorded named surfaces' tree
// paths would not need the guess.
fn widgets(
    scene: &mui_scene::ResolvedScene,
    roots: &[String],
) -> (
    Vec<(String, String)>,
    std::collections::HashMap<String, mui_scene::Frame>,
) {
    let list: Vec<_> = scene.surfaces().collect();
    let named = |s: &str| mui_scene::Id::is_named(s);
    let under = |id: &str, top: &str| {
        let mut up = Some(id);
        while let Some(p) = up {
            if p == top {
                return true;
            }
            up = scene.surface(p).and_then(|s| s.parent.as_deref());
        }
        false
    };
    let mut adopt = Vec::new();
    let mut frames = std::collections::HashMap::new();
    for root in roots {
        let Some(i) = list.iter().position(|s| &*s.key == root) else {
            continue;
        };
        let scope = list[i].parent.as_deref();
        // Where it is: inside the unnamed containers whose paths prefix the
        // first surface after it (and its own), or, when that one is named
        // or outside (the part ends its container), those prefixing the
        // surface before it that also frame it.
        let next = list[i + 1..]
            .iter()
            .find(|s| !under(&s.key, root))
            .filter(|s| !named(&s.key));
        let prev = list[..i].last().filter(|s| !named(&s.key));
        let own = list[i].frame;
        let holds = |u: &mui_scene::ResolvedSurface| {
            let inside = |k: &str| k.starts_with(&format!("{}/", u.key));
            let f = u.frame;
            next.is_some_and(|n| inside(&n.key))
                || prev.is_some_and(|p| {
                    (p.key == u.key || inside(&p.key))
                        && f.x <= own.x
                        && f.y <= own.y
                        && own.x + own.size.width <= f.x + f.size.width
                        && own.y + own.size.height <= f.y + f.size.height
                })
        };
        let mut widget = None;
        for u in list[..i].iter().rev() {
            if named(&u.key) || !holds(u) {
                continue;
            }
            if u.parent.as_deref() != scope {
                break;
            }
            let inside = |s: &str| s.starts_with(&format!("{}/", u.key));
            let start = list.iter().position(|s| s.key == u.key).unwrap_or(0);
            let end = list
                .iter()
                .rposition(|s| inside(&s.key))
                .unwrap_or(start)
                .max(i);
            let crowded = list[start..=end]
                .iter()
                .any(|s| named(&s.key) && !under(&s.key, root));
            if crowded {
                break;
            }
            widget = Some(u);
        }
        let Some(w) = widget else {
            continue;
        };
        frames.insert(root.clone(), w.frame);
        for s in &list {
            let member = s.key == w.key || s.key.starts_with(&format!("{}/", w.key));
            if member && !named(&s.key) && s.parent.as_deref() == scope {
                adopt.push((s.key.to_string(), root.clone()));
            }
        }
    }
    (adopt, frames)
}

/// [`discover_parts`], then `depth - 1` more levels inside each part: its
/// topmost named descendants that are control-sized (at least 12 px each
/// way, at most 60% of it) and can be taken out whole. Parents come first.
/// A UI with no panel-sized part (a compact strip) starts at its controls.
pub fn discover_tree(
    scene: &mui_scene::ResolvedScene,
    width: f64,
    height: f64,
    depth: usize,
) -> Vec<String> {
    let mut all = discover_parts(scene, width, height);
    if all.is_empty() {
        all = controls(scene, None, width * height);
    }
    let mut level = all.clone();
    for _ in 1..depth {
        let mut next = Vec::new();
        for part in &level {
            let Some(area) = scene
                .surface(part)
                .map(|s| s.frame.size.width * s.frame.size.height)
            else {
                continue;
            };
            next.extend(controls(scene, Some(part), area));
        }
        if next.is_empty() {
            break;
        }
        all.extend(next.iter().cloned());
        level = next;
    }
    all
}

/// The topmost named, control-sized surfaces under `part` (anywhere for
/// `None`) that can be taken out whole; `area` is the part's.
fn controls(scene: &mui_scene::ResolvedScene, part: Option<&str>, area: f64) -> Vec<String> {
    let under = |id: &str, top: &str| {
        let mut up = scene.surface(id).and_then(|s| s.parent.as_deref());
        while let Some(p) = up {
            if p == top {
                return true;
            }
            up = scene.surface(p).and_then(|s| s.parent.as_deref());
        }
        false
    };
    let candidates: Vec<&str> = scene
        .surfaces()
        .filter(|s| {
            mui_scene::Id::is_named(&s.key)
                && s.frame.size.width >= 12.
                && s.frame.size.height >= 12.
                && s.frame.size.width * s.frame.size.height <= area * 0.6
                && part.is_none_or(|p| under(&s.key, p))
                && scene.isolate(&[&s.key]).is_ok()
        })
        .map(|s| s.key.as_ref())
        .collect();
    candidates
        .iter()
        .filter(|c| !candidates.iter().any(|o| o != *c && under(c, o)))
        .map(|c| (*c).to_owned())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use mui_scene::prelude::*;
    #[test]
    fn automatic_parts_and_pixel_cache_follow_real_scene_changes() {
        let make = |width| {
            resolve(&SceneSpec::new(
                col([block(width, 60.).fill(Role::Ink).id("panel")])
                    .size(300., 200.)
                    .id("root"),
            ))
            .unwrap()
        };
        let a = make(100.);
        let roots = discover_parts(&a, 300., 200.);
        assert_eq!(roots, vec!["panel"]);
        let mut stream = CaptureStream::default();
        let first = stream.frame(&a, 300, 200, 1., &roots).unwrap();
        assert!(!first["images"].as_object().unwrap().is_empty());
        let second = stream.frame(&a, 300, 200, 1., &roots).unwrap();
        assert!(second["images"].as_object().unwrap().is_empty());
        assert_eq!(first["scene"], second["scene"]);
        let changed = stream.frame(&make(140.), 300, 200, 1., &roots).unwrap();
        assert!(!changed["images"].as_object().unwrap().is_empty());
        assert_ne!(first["scene"]["layers"], changed["scene"]["layers"]);
    }

    /// A UI whose root and wrappers are anonymous (as plugins' often are)
    /// still splits into its named panels.
    #[test]
    fn parts_need_no_named_root() {
        let scene = resolve(&SceneSpec::new(
            col([col([
                block(100., 60.).fill(Role::Ink).id("about"),
                block(120., 60.).fill(Role::Ink).id("settings"),
            ])])
            .size(300., 200.),
        ))
        .unwrap();
        assert_eq!(discover_parts(&scene, 300., 200.), ["about", "settings"]);
    }

    /// A compact strip with no panel-sized part splits into its controls.
    #[test]
    fn a_strip_without_panels_starts_at_its_controls() {
        let scene = resolve(&SceneSpec::new(
            row([
                block(40., 18.).fill(Role::Ink).id("share"),
                block(24., 24.).fill(Role::Ink).id("roll"),
                block(8., 8.).fill(Role::Ink).id("dot"),
            ])
            .size(300., 40.),
        ))
        .unwrap();
        assert!(discover_parts(&scene, 300., 40.).is_empty());
        assert_eq!(discover_tree(&scene, 300., 40., 1), ["share", "roll"]);
    }

    /// Two levels: panels, then the controls in them, addressed by path;
    /// a control that overflows its clipped panel also comes free of it.
    #[test]
    fn a_two_level_tree_names_controls_by_path_and_frees_them() {
        // As mui's knob: the name on the dial; its pointer and caption
        // unnamed blocks beside it, in an unnamed column.
        let knob = |id: &str| {
            col([
                stack([
                    block(30., 30.).fill(Role::Primary).id(id),
                    block(4., 4.).fill(Role::Ink),
                ]),
                block(30., 8.).fill(Role::Dim),
            ])
        };
        let scene = resolve(&SceneSpec::new(
            row![
                row![knob("a-1"), knob("a-2"), text("A")]
                    .size(120., 60.)
                    .fill(Role::Ink)
                    .id("a"),
                // `b`'s bar is wider than `b`, which clips it.
                stack([block(160., 20.).fill(Role::Danger).id("b-bar")])
                    .size(120., 60.)
                    .fill(Role::Ink)
                    .clip()
                    .id("b"),
            ]
            .size(300., 200.)
            .id("root"),
        ))
        .unwrap();
        assert_eq!(discover_tree(&scene, 300., 200., 1), ["a", "b"]);
        let roots = discover_tree(&scene, 300., 200., 2);
        assert_eq!(roots, ["a", "b", "a-1", "a-2", "b-bar"]);
        let frame = capture_frame(&scene, 300, 200, 1., &roots).unwrap();
        let m = &frame["scene"];
        let paths: Vec<(&str, Option<&str>)> = m["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| (p["path"].as_str().unwrap(), p["parent"].as_str()))
            .collect();
        assert_eq!(
            paths,
            [
                ("a", None),
                ("b", None),
                ("a/a-1", Some("a")),
                ("a/a-2", Some("a")),
                ("b/b-bar", Some("b"))
            ]
        );
        let layer = |g: &str| {
            m["layers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|l| l["group"] == g)
                .unwrap()
                .clone()
        };
        assert_eq!(layer("a/a-1")["part"], "a-1");
        // The knob is its whole widget: dial, pointer and caption, framed
        // together, and its panel keeps none of them.
        let knob = m["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["path"] == "a/a-1")
            .unwrap();
        // (The panel row stretches the column to its height.)
        assert_eq!(
            (&knob["frame"][2], &knob["frame"][3]),
            (&30.0.into(), &60.0.into())
        );
        let a1 = layer("a/a-1");
        assert_eq!(a1["rect"][3], 38.);
        assert_eq!(layer("a/a-1")["parent"], "a");
        // The bar's clipped raster is `b`'s width; freed, it is its own.
        let bar = layer("b/b-bar");
        assert_eq!(bar["rect"][2], 120.);
        assert_eq!(bar["free"]["rect"][2], 160.);
        assert!(frame["images"][bar["free"]["src"].as_str().unwrap()].is_string());
        // A knob no clip cuts needs no second image.
        assert!(layer("a/a-1").get("free").is_none());
    }

    /// A knob named on its dial takes its pointer and caption along: the
    /// unnamed column around it holds no other named surface.
    #[test]
    fn a_control_named_on_an_inner_block_takes_its_widget() {
        let knob = |id: &str| {
            col([
                stack([
                    block(30., 30.).fill(Role::Primary).id(id),
                    block(4., 4.).fill(Role::Danger),
                ]),
                block(30., 6.).fill(Role::Dim),
            ])
        };
        let scene = resolve(&SceneSpec::new(
            col([row([knob("k1"), knob("k2")])
                .size(160., 80.)
                .fill(Role::Ink)
                .id("p")])
            .size(300., 200.)
            .id("root"),
        ))
        .unwrap();
        let roots = discover_tree(&scene, 300., 200., 2);
        assert_eq!(roots, ["p", "k1", "k2"]);
        let frame = capture_frame(&scene, 300, 200, 1., &roots).unwrap();
        let m = &frame["scene"];
        // The part's frame is its column's: dial and caption, 30 x 40ish.
        let part = m["parts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|p| p["id"] == "k1")
            .unwrap()
            .clone();
        assert!(part["frame"][3].as_f64().unwrap() > 30., "{part}");
        // Its fragment covers the caption under the dial.
        let rect = m["layers"]
            .as_array()
            .unwrap()
            .iter()
            .find(|l| l["group"] == "p/k1")
            .unwrap()["rect"]
            .clone();
        assert!(rect[3].as_f64().unwrap() > 30., "{rect}");
    }

    /// A slider named on its track, last in its column, takes the label
    /// row above it too.
    #[test]
    fn a_control_that_ends_its_widget_still_takes_it() {
        let fader = |id: &str| {
            col([
                row([
                    block(20., 8.).fill(Role::Dim),
                    block(10., 8.).fill(Role::Dim),
                ]),
                block(100., 12.).fill(Role::Primary).id(id),
            ])
        };
        let scene = resolve(&SceneSpec::new(
            col([col([fader("s1"), fader("s2")])
                .size(160., 80.)
                .fill(Role::Ink)
                .id("p")])
            .size(300., 200.)
            .id("root"),
        ))
        .unwrap();
        let roots = discover_tree(&scene, 300., 200., 2);
        assert_eq!(roots, ["p", "s1", "s2"]);
        let m = &capture_frame(&scene, 300, 200, 1., &roots).unwrap()["scene"];
        for id in ["s1", "s2"] {
            let part = m["parts"]
                .as_array()
                .unwrap()
                .iter()
                .find(|p| p["id"] == id)
                .unwrap()
                .clone();
            assert_eq!(part["frame"][3], 20., "{part}");
        }
    }
}
