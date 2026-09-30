//! Where layers sit: parenting, reset to default, and taking a 3D scene
//! flat.
//!
//! A layer's `parent` makes its transform relative to another layer's, as
//! in Cavalry and After Effects. [`compose`] runs inside [`eval`], so every
//! renderer (2D, 3D, Blender), `check` and the editor see the composed,
//! world-space values. [`reparent`] attaches or detaches a layer keeping it
//! where it is on screen, by rewriting its local transform.
use crate::{Anim, Drawn, Interp, Key, Kind, Layer, Mode, Project, Scene, eval, three};

/// A layer's transform at one time: where a point in its own space lands
/// in its parent's (for a world transform, in the frame). In 3D it is the
/// affine a slab is posed with ([`three::pose`]): moved to `x, y, z`,
/// turned by `ry`, `rx` then `rotation`, scaled, and its content pushed
/// `anchor` pixels towards the viewer from the pivot, so a child sits on
/// its parent's face. In 2D the 3D parts are zero.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Degrees, clockwise.
    pub rotation: f64,
    /// Degrees: `rx` tips the top away, `ry` turns the right side away.
    pub rx: f64,
    pub ry: f64,
    pub scale: f64,
    pub opacity: f64,
    pub anchor: f64,
}

/// A rotation in project space (x right, y down, z deeper), rows.
type M3 = [[f64; 3]; 3];

/// `ry`, then `rx`, then `rz` (degrees), as [`three::pose`] turns a slab,
/// in project space.
fn turn(rx: f64, ry: f64, rz: f64) -> M3 {
    let (sx, cx) = rx.to_radians().sin_cos();
    let (sy, cy) = ry.to_radians().sin_cos();
    let (sz, cz) = rz.to_radians().sin_cos();
    [
        [cy * cz + sy * sx * sz, -cy * sz + sy * sx * cz, -sy * cx],
        [cx * sz, cx * cz, sx],
        [sy * cz - cy * sx * sz, -sy * sz - cy * sx * cz, cy * cx],
    ]
}

/// [`turn`] backwards: `[rx, ry, rz]`, each moved by whole turns to lie
/// nearest `near` (so a spin keeps counting past 180).
fn euler(m: &M3, near: [f64; 3]) -> [f64; 3] {
    let rx = m[1][2].clamp(-1., 1.).asin();
    let (ry, rz) = if rx.cos() > 1e-9 {
        ((-m[0][2]).atan2(m[2][2]), m[1][0].atan2(m[1][1]))
    } else {
        // Edge on: only ry - rz (or ry + rz) is known; put it all in ry.
        (m[2][0].atan2(m[0][0]), 0.)
    };
    let mut out = [rx, ry, rz].map(f64::to_degrees);
    for (v, n) in out.iter_mut().zip(near) {
        *v += 360. * ((n - *v) / 360.).round();
    }
    out
}

fn mul(a: &M3, b: &M3) -> M3 {
    std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|k| a[r][k] * b[k][c]).sum()))
}
fn apply3(m: &M3, v: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|r| (0..3).map(|k| m[r][k] * v[k]).sum())
}
fn transpose(m: &M3) -> M3 {
    std::array::from_fn(|r| std::array::from_fn(|c| m[c][r]))
}

/// A unit vector along `pitch` and `yaw` ([`three::aim`]) and back.
fn unaim(d: [f64; 3], near: [f64; 2]) -> [f64; 2] {
    let pitch = d[1].clamp(-1., 1.).asin().to_degrees();
    let yaw = d[0].atan2(d[2]).to_degrees();
    let yaw = yaw + 360. * ((near[1] - yaw) / 360.).round();
    [pitch, yaw]
}

impl Xf {
    /// The frame itself: an unparented layer's parent.
    pub const FRAME: Xf = Xf {
        x: 0.,
        y: 0.,
        z: 0.,
        rotation: 0.,
        rx: 0.,
        ry: 0.,
        scale: 1.,
        opacity: 1.,
        anchor: 0.,
    };

    /// A layer's evaluated transform; `three` keeps its 3D turns and
    /// anchor (a 2D scene ignores them).
    pub fn of(d: &Drawn, three: bool) -> Xf {
        let k = if three { 1. } else { 0. };
        Xf {
            x: d.x,
            y: d.y,
            z: d.space.z,
            rotation: d.rotation,
            rx: k * d.space.rx,
            ry: k * d.space.ry,
            scale: d.scale,
            opacity: d.opacity,
            anchor: k * d.space.anchor_z,
        }
    }

    /// Turned in 3D, not only about z.
    pub fn tilted(&self) -> bool {
        self.rx != 0. || self.ry != 0.
    }

    fn turn(&self) -> M3 {
        turn(self.rx, self.ry, self.rotation)
    }

    /// A point in this space, in the space around it.
    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let q = apply3(&self.turn(), [p[0], p[1], p[2] - self.anchor]);
        let k = self.scale;
        [self.x + k * q[0], self.y + k * q[1], self.z + k * q[2]]
    }

    /// [`Xf::apply`] backwards. A parent with no scale keeps its child's
    /// offsets as if it had a tiny one.
    pub fn unapply(&self, p: [f64; 3]) -> [f64; 3] {
        let k = if self.scale.abs() < 1e-9 {
            1e-9
        } else {
            self.scale
        };
        let d = [
            (p[0] - self.x) / k,
            (p[1] - self.y) / k,
            (p[2] - self.z) / k,
        ];
        let q = apply3(&transpose(&self.turn()), d);
        [q[0], q[1], q[2] + self.anchor]
    }

    /// A child with transform `local` in this space, in the space around.
    /// Turns about z alone add up; a tilt composes the rotations and
    /// splits the result back into `rx`, `ry` and `rotation`.
    pub fn then(&self, local: &Xf) -> Xf {
        let [x, y, z] = self.apply([local.x, local.y, local.z]);
        let naive = [
            self.rx + local.rx,
            self.ry + local.ry,
            self.rotation + local.rotation,
        ];
        let [rx, ry, rotation] = if self.tilted() || local.tilted() {
            euler(&mul(&self.turn(), &local.turn()), naive)
        } else {
            naive
        };
        Xf {
            x,
            y,
            z,
            rotation,
            rx,
            ry,
            scale: self.scale * local.scale,
            opacity: self.opacity * local.opacity,
            anchor: local.anchor,
        }
    }

    /// A light's or camera's aim (`rx` pitch, `ry` yaw, [`three::aim`]) in
    /// this space, in the space around.
    fn aim(&self, pitch: f64, yaw: f64) -> [f64; 2] {
        let d = apply3(&self.turn(), three::aim(pitch, yaw));
        unaim(d, [pitch, yaw])
    }
}

/// Aims a light or camera: its `rx` and `ry` are a direction, not a turn.
fn aims(k: &Kind) -> bool {
    matches!(k, Kind::Light { .. } | Kind::Camera { .. })
}

/// Every layer of `scene` (evaluated into `layers`, same order) moved into
/// world space: a parented layer's transform goes through its parent's,
/// its parent's parent's and so on (in 3D the full turn, tilts included;
/// a light or camera's aim turns with its parent). A cycle (which loading
/// refuses) is broken where it closes.
pub fn compose(scene: &Scene, layers: &mut [Drawn]) {
    if scene.layers.iter().all(|l| l.parent.is_empty()) {
        return;
    }
    let three = scene.mode == Mode::ThreeD;
    let parent: Vec<Option<usize>> = scene
        .layers
        .iter()
        .map(|l| scene.layers.iter().position(|p| p.id == l.parent))
        .collect();
    // A light's or camera's `rx`/`ry` aim it; they do not turn its space.
    let local: Vec<Xf> = layers
        .iter()
        .map(|d| Xf::of(d, three && !aims(&d.kind)))
        .collect();
    let mut world: Vec<Option<Xf>> = vec![None; layers.len()];
    for (i, d) in layers.iter_mut().enumerate() {
        let w = resolve(i, 0, &parent, &local, &mut world);
        (d.x, d.y, d.space.z) = (w.x, w.y, w.z);
        (d.rotation, d.scale, d.opacity) = (w.rotation, w.scale, w.opacity);
        if !three {
            continue;
        }
        if aims(&d.kind) {
            if let Some(p) = parent[i] {
                let pw = resolve(p, 0, &parent, &local, &mut world);
                [d.space.rx, d.space.ry] = pw.aim(d.space.rx, d.space.ry);
            }
        } else {
            (d.space.rx, d.space.ry) = (w.rx, w.ry);
        }
    }
}

/// Layer `i`'s world transform, its parents' first, memoised in `world`.
fn resolve(
    i: usize,
    depth: usize,
    parent: &[Option<usize>],
    local: &[Xf],
    world: &mut [Option<Xf>],
) -> Xf {
    if let Some(w) = world[i] {
        return w;
    }
    let w = match parent[i] {
        Some(p) if depth < parent.len() => {
            resolve(p, depth + 1, parent, local, world).then(&local[i])
        }
        _ => local[i],
    };
    world[i] = Some(w);
    w
}

/// Every `parent` names another layer of the scene and no chain of them
/// comes back round. The error is the index of a layer it is about.
pub(crate) fn check(s: &Scene) -> Result<(), (usize, String)> {
    for (li, l) in s.layers.iter().enumerate() {
        let mut chain = vec![l.id.as_str()];
        let mut at = l;
        while !at.parent.is_empty() {
            let Some(p) = s.layers.iter().find(|o| o.id == at.parent) else {
                return Err((
                    li,
                    format!("layer `{}`: no layer `{}` to parent to", at.id, at.parent),
                ));
            };
            if chain.contains(&p.id.as_str()) {
                chain.push(&p.id);
                return Err((
                    li,
                    format!("layer `{}`: a parent cycle ({})", l.id, chain.join(" -> ")),
                ));
            }
            chain.push(&p.id);
            at = p;
        }
    }
    Ok(())
}

/// How a layer's turn changes when its space does.
enum Turn<'a> {
    /// `rotation` shifted by these degrees; `rx`, `ry` unchanged.
    Z(f64),
    /// `[rx, ry, rotation]` at one time to the new ones.
    Full(&'a dyn Fn([f64; 3]) -> [f64; 3]),
}

/// Tracks rewritten together by `f`, from all their values at one time to
/// all the new ones. A plain track takes `f` at `t`; keys take it at their
/// own times. When `f` mixes the tracks (`mixes`: a turn moves x into y)
/// and they are keyed at different times, every one is first keyed at all
/// their key times (the missing keys baked from its own curve), so each
/// new key is exact. Value handles are multiplied by `handles`.
// ponytail: a key baked into a bezier segment splits it into two eases, so
// the curve between keys can drift a little; the keys themselves are exact.
fn together(
    tracks: &mut [&mut Anim<f64>],
    t: f64,
    mixes: bool,
    f: &dyn Fn(&[f64]) -> Vec<f64>,
    handles: f64,
) {
    let old: Vec<Anim<f64>> = tracks.iter().map(|a| (**a).clone()).collect();
    let at = |tk: f64| old.iter().map(|a| a.at(tk)).collect::<Vec<_>>();
    let mut times: Vec<f64> = old
        .iter()
        .flat_map(|a| match a {
            Anim::Keys(ks) => ks.iter().map(|k| k.t).collect(),
            Anim::Value(_) => Vec::new(),
        })
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup();
    if mixes && !times.is_empty() {
        for (a, o) in tracks.iter_mut().zip(&old) {
            let key = |tk: f64| match o {
                Anim::Keys(ks) => ks.iter().find(|k| k.t == tk).cloned().unwrap_or(Key {
                    t: tk,
                    v: o.at(tk),
                    interp: ks
                        .iter()
                        .rev()
                        .find(|k| k.t < tk)
                        .map_or(Interp::Linear, |k| k.interp),
                    in_: None,
                    out: None,
                }),
                Anim::Value(v) => Key {
                    t: tk,
                    v: *v,
                    interp: Interp::Linear,
                    in_: None,
                    out: None,
                },
            };
            **a = Anim::Keys(times.iter().map(|&tk| key(tk)).collect());
        }
    }
    for (i, a) in tracks.iter_mut().enumerate() {
        match &mut **a {
            Anim::Value(v) => *v = f(&at(t))[i],
            Anim::Keys(keys) => {
                for k in keys {
                    k.v = f(&at(k.t))[i];
                    for h in k.in_.iter_mut().chain(k.out.iter_mut()) {
                        h[1] *= handles;
                    }
                }
            }
        }
    }
}

/// Rewrite a layer's local transform through `map`, a function from a
/// point in its old parent's space to the new one's: `x`, `y` (and `z`
/// with `three`) key by key, each key's point completed by the other
/// tracks at that key's time; its turn by `turn`, and `scale` (and
/// position handles) multiplied by `grow`.
fn remap(
    l: &mut Layer,
    t: f64,
    three: bool,
    map: &dyn Fn([f64; 3]) -> [f64; 3],
    turn: Turn<'_>,
    grow: f64,
) {
    // Does `map` move one axis into another near where the layer is?
    let p = [l.x.at(t), l.y.at(t), l.z.at(t)];
    let o = map(p);
    let n = if three { 3 } else { 2 };
    let mixes = (0..n).any(|i| {
        let mut q = p;
        q[i] += 1.;
        let d = map(q);
        (0..n).any(|j| j != i && (d[j] - o[j]).abs() > 1e-9 * (1. + o[j].abs()))
    });
    // A 2D map ignores depth.
    let point = |v: &[f64]| map([v[0], v[1], v.get(2).copied().unwrap_or(0.)]).to_vec();
    let Layer { x, y, z, .. } = l;
    if three {
        together(&mut [x, y, z], t, mixes, &point, grow);
    } else {
        together(&mut [x, y], t, mixes, &point, grow);
    }
    match turn {
        Turn::Z(by) => together(&mut [&mut l.rotation], t, false, &|v| vec![v[0] + by], 1.),
        Turn::Full(f) => {
            let Layer {
                rx, ry, rotation, ..
            } = l;
            together(
                &mut [rx, ry, rotation],
                t,
                true,
                &|v| f([v[0], v[1], v[2]]).to_vec(),
                1.,
            );
        }
    }
    together(&mut [&mut l.scale], t, false, &|v| vec![v[0] * grow], grow);
}

/// Layer `id` of `scene` attached to `parent` (`None` detaches it), still
/// where it is on screen at `t`: its local position, turns and scale are
/// rewritten from the old parent's space into the new one's, through both
/// parents' world transforms at `t`, as Cavalry and After Effects do (in
/// 3D, tilts included). Returns the rewritten layer; refuses a parent that
/// is missing, is the layer, or hangs under it (a cycle).
pub fn reparent(
    project: &Project,
    scene: &Scene,
    id: &str,
    parent: Option<&str>,
    t: f64,
) -> Result<Layer, String> {
    let i = scene
        .layers
        .iter()
        .position(|l| l.id == id)
        .ok_or_else(|| format!("no layer `{id}`"))?;
    let parent = parent.filter(|p| !p.is_empty());
    if let Some(p) = parent {
        let mut at = Some(p);
        while let Some(a) = at {
            if a == id {
                return Err(format!("`{p}` hangs under `{id}`: that would be a cycle"));
            }
            at = scene
                .layers
                .iter()
                .find(|l| l.id == a)
                .ok_or_else(|| format!("no layer `{a}` to parent to"))
                .map(|l| Some(l.parent.as_str()).filter(|s| !s.is_empty()))?;
        }
    }
    let three = scene.mode == Mode::ThreeD;
    let frame = eval(project, scene, t);
    let world = |pid: &str| {
        (!pid.is_empty())
            .then(|| scene.layers.iter().position(|l| l.id == pid))
            .flatten()
            .map_or(Xf::FRAME, |j| {
                Xf::of(&frame.layers[j], three && !aims(&frame.layers[j].kind))
            })
    };
    let (old, new) = (world(&scene.layers[i].parent), world(parent.unwrap_or("")));
    if new.scale.abs() < 1e-9 {
        return Err(format!("`{}` has no scale at {t}s", parent.unwrap_or("")));
    }
    let mut l = scene.layers[i].clone();
    // The turn from the old parent's space into the new one's.
    let q = mul(&transpose(&new.turn()), &old.turn());
    let spin = old.rotation - new.rotation;
    let aimed = aims(&l.kind);
    let full = |v: [f64; 3]| {
        let [rx, ry, rz] = v;
        if aimed {
            let d = apply3(&q, three::aim(rx, ry));
            let [px, py] = unaim(d, [rx, ry]);
            [px, py, rz + spin]
        } else {
            euler(&mul(&q, &turn(rx, ry, rz)), [rx, ry, rz + spin])
        }
    };
    // An aim follows any turn; a tilt makes turns mix.
    let tilts = old.tilted() || new.tilted() || (three && aimed && spin != 0.);
    let turn = if tilts {
        Turn::Full(&full)
    } else {
        Turn::Z(spin)
    };
    remap(
        &mut l,
        t,
        three,
        &|p| new.unapply(old.apply(p)),
        turn,
        old.scale / new.scale,
    );
    l.parent = parent.unwrap_or("").to_owned();
    Ok(l)
}

impl Layer {
    /// Back to where it was placed, the default layout: every transform
    /// key and offset cleared, and a plugin's explode and part offsets
    /// (the UI as captured). An unparented layer goes to the middle of a
    /// `size` frame (where new layers go), a parented one onto its parent.
    /// A plugin's parameters and pointer stay: they are what it shows, not
    /// where.
    pub fn reset(&mut self, size: [u32; 2]) {
        let home = if self.parent.is_empty() {
            size.map(|v| f64::from(v) / 2.)
        } else {
            [0., 0.]
        };
        (self.x, self.y) = (Anim::Value(home[0]), Anim::Value(home[1]));
        for a in [&mut self.rotation, &mut self.z, &mut self.rx, &mut self.ry] {
            *a = Anim::Value(0.);
        }
        (self.scale, self.opacity) = (Anim::Value(1.), Anim::Value(1.));
        if let Kind::Plugin { parts, .. } = &mut self.kind {
            parts.clear();
            (self.explode, self.backdrop) = (Anim::Value(0.), Anim::Value(1.));
        }
    }
}

/// Scene `scene` as a 2D scene that looks, at `t`, like its 3D shot: each
/// unparented layer moved to where the camera shows it (every key, through
/// the camera at `t`), scaled by its distance and turned by the camera's
/// roll; depth and 3D turns (`z`, `rx`, `ry`) are dropped. A 2D scene comes
/// back as it is. Going the other way needs nothing: a 3D scene's default
/// camera shows the z = 0 plane exactly as the 2D frame.
// ponytail: layers turned in 3D lose their foreshortening, and children
// keep their local offsets (their parent's new scale carries them).
pub fn flatten(project: &Project, scene: &Scene, t: f64) -> Scene {
    let f = eval(project, scene, t);
    let Some(view) = &f.view else {
        return scene.clone();
    };
    let size = project.size;
    let [w, h] = size.map(f64::from);
    let vp = three::stage_camera(size, &view.camera).view_proj((w / h) as f32);
    let screen = |p: [f64; 3]| {
        let [nx, ny, _] = vp.project(three::world(size, p)).map(f64::from);
        [(nx + 1.) / 2. * w, (1. - ny) / 2. * h, 0.]
    };
    let mut out = scene.clone();
    (out.mode, out.ground, out.fog) = (Mode::TwoD, None, None);
    for (l, d) in out.layers.iter_mut().zip(&f.layers) {
        let flat = matches!(l.kind, Kind::Camera { .. } | Kind::Light { .. });
        if l.parent.is_empty() && !flat {
            // The screen's own x axis at the layer: its length is the
            // camera's magnification there, its angle the roll.
            let a = screen([d.x, d.y, d.space.z]);
            let b = screen([d.x + 10., d.y, d.space.z]);
            let grow = (b[0] - a[0]).hypot(b[1] - a[1]) / 10.;
            let turn = (b[1] - a[1]).atan2(b[0] - a[0]).to_degrees();
            remap(l, t, true, &|p| screen(p), Turn::Z(turn), grow);
        }
        for a in [&mut l.z, &mut l.rx, &mut l.ry] {
            *a = Anim::Value(0.);
        }
    }
    out
}

/// Scene `si` of the project text `root` rewritten by `op` (a reparent, a
/// reset, taking it flat) in every variant: the declared defaults and each
/// named one, where bindings and overrides can put layers elsewhere.
/// Returns the new text. What `op` leaves alone keeps its bindings; a
/// value it changes is written resolved: into the file where the defaults
/// changed, and into a variant's `scene/layer` override wherever that
/// variant's result differs from what the file now gives it.
pub fn rewrite(
    root: &serde_json::Value,
    si: usize,
    op: &dyn Fn(&Project, &Scene) -> Result<Scene, String>,
) -> Result<serde_json::Value, String> {
    use serde_json::{Map, Value};
    let names: Vec<Option<String>> = std::iter::once(None)
        .chain(
            root.get("variants")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|v| Some(Some(v.get("name")?.as_str()?.to_owned()))),
        )
        .collect();
    // Per variant: the scene before and after, as JSON objects.
    let mut runs = Vec::new();
    for name in &names {
        let p = Project::from_root(root, name.as_deref())?;
        let s = p.scenes.get(si).ok_or("no such scene")?;
        let after = op(&p, s)?;
        if after.layers.len() != s.layers.len() {
            return Err("a rewrite keeps the layers".into());
        }
        let json = |s: &Scene| serde_json::to_value(s).map_err(|e| e.to_string());
        runs.push((name.as_deref(), json(s)?, json(&after)?));
    }
    let scene_name = runs[0].1["name"].as_str().unwrap_or("").to_owned();
    let mut out = root.clone();
    // The fields `op` changed on one object (the scene with `layer` None,
    // else that layer), each written where it now has to be.
    let fix = |out: &mut Value, layer: Option<usize>| -> Result<(), String> {
        let pick = |v: &Value| -> Map<String, Value> {
            let o = match layer {
                Some(j) => &v["layers"][j],
                None => v,
            };
            let mut m = o.as_object().cloned().unwrap_or_default();
            m.remove("layers");
            m
        };
        let before: Vec<_> = runs.iter().map(|r| pick(&r.1)).collect();
        let after: Vec<_> = runs.iter().map(|r| pick(&r.2)).collect();
        let id = before[0].get("id").and_then(Value::as_str).unwrap_or("");
        let target = match layer {
            Some(_) => format!("{scene_name}/{id}"),
            None => scene_name.clone(),
        };
        let mut keys: Vec<&String> = before.iter().chain(&after).flat_map(|m| m.keys()).collect();
        keys.sort();
        keys.dedup();
        for k in keys {
            let changed = |i: usize| before[i].get(k) != after[i].get(k);
            if !(0..runs.len()).any(changed) {
                continue;
            }
            let base_changed = changed(0);
            if base_changed {
                let raw = match layer {
                    Some(j) => &mut out["scenes"][si]["layers"][j],
                    None => &mut out["scenes"][si],
                };
                let raw = raw
                    .as_object_mut()
                    .ok_or("the scene or layer is not an object")?;
                match after[0].get(k) {
                    Some(v) => raw.insert(k.clone(), v.clone()),
                    None => raw.remove(k),
                };
            }
            for (i, (name, ..)) in runs.iter().enumerate().skip(1) {
                let name = name.expect("variants past the defaults");
                let vi = i - 1;
                let overridden = overrides_field(root, vi, &scene_name, layer.map(|_| id), k);
                // Where this variant's value will come from after the edit.
                let from_file = base_changed && !overridden;
                let need = if from_file {
                    after[i].get(k) != after[0].get(k)
                } else {
                    changed(i)
                };
                if !need {
                    continue;
                }
                if scene_name.contains('/') || scene_name.is_empty() || scene_name == "*" {
                    return Err(format!(
                        "variant `{name}` needs its own `{k}` for `{target}`, and scene `{scene_name}` cannot be named in an override: rename the scene"
                    ));
                }
                let v = after[i].get(k).cloned().unwrap_or_else(|| default_of(k));
                let o = out["variants"][vi]
                    .as_object_mut()
                    .ok_or("a variant is not an object")?
                    .entry("overrides")
                    .or_insert_with(|| Value::Object(Map::new()))
                    .as_object_mut()
                    .ok_or("`overrides` is not an object")?
                    .entry(target.clone())
                    .or_insert_with(|| Value::Object(Map::new()));
                o.as_object_mut()
                    .ok_or_else(|| format!("override `{target}` is not an object"))?
                    .insert(k.clone(), v);
            }
        }
        Ok(())
    };
    fix(&mut out, None)?;
    let n = runs[0].1["layers"].as_array().map_or(0, Vec::len);
    for j in 0..n {
        fix(&mut out, Some(j))?;
    }
    Ok(out)
}

/// Does variant `vi` of `root` override field `k` of scene `scene` (or of
/// its layer `layer`)?
fn overrides_field(
    root: &serde_json::Value,
    vi: usize,
    scene: &str,
    layer: Option<&str>,
    k: &str,
) -> bool {
    let Some(o) = root["variants"][vi]["overrides"].as_object() else {
        return false;
    };
    o.iter().any(|(key, patch)| {
        let (s, l) = match key.split_once('/') {
            Some((s, l)) => (s, Some(l)),
            None => (key.as_str(), None),
        };
        (s == "*" || s == scene) && l == layer && patch.get(k).is_some()
    })
}

/// A field's value when a save leaves it out: what an override must spell
/// out to put it back.
fn default_of(k: &str) -> serde_json::Value {
    match k {
        "scale" | "opacity" | "backdrop" => 1.into(),
        "parent" => "".into(),
        "mode" => "2d".into(),
        "parts" => serde_json::json!({}),
        "ground" | "fog" => serde_json::Value::Null,
        _ => 0.into(),
    }
}
