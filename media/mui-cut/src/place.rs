//! Where layers sit: parenting, reset to default, and taking a 3D scene
//! flat.
//!
//! A layer's `parent` makes its transform relative to another layer's, as
//! in Cavalry and After Effects. [`compose`] runs inside [`eval`], so every
//! renderer (2D, 3D, Blender), `check` and the editor see the composed,
//! world-space values. [`reparent`] attaches or detaches a layer keeping it
//! where it is on screen, by rewriting its local transform.
use crate::{Anim, Drawn, Kind, Layer, Mode, Project, Scene, eval, three};

/// A layer's transform at one time: where a point in its own space lands
/// in its parent's (for a world transform, in the frame).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Xf {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    /// Degrees, clockwise.
    pub rotation: f64,
    pub scale: f64,
    pub opacity: f64,
}

impl Xf {
    /// The frame itself: an unparented layer's parent.
    pub const FRAME: Xf = Xf {
        x: 0.,
        y: 0.,
        z: 0.,
        rotation: 0.,
        scale: 1.,
        opacity: 1.,
    };

    pub fn of(d: &Drawn) -> Xf {
        Xf {
            x: d.x,
            y: d.y,
            z: d.space.z,
            rotation: d.rotation,
            scale: d.scale,
            opacity: d.opacity,
        }
    }

    /// A point in this space, in the space around it. Depth is scaled with
    /// the layer but not turned: turning in 3D (`rx`, `ry`) is not
    /// inherited.
    // ponytail: z-only 3D inheritance; compose full 3D rotations (a 4x4 per
    // layer) when children must orbit with a tilted parent.
    pub fn apply(&self, p: [f64; 3]) -> [f64; 3] {
        let (s, c) = self.rotation.to_radians().sin_cos();
        let k = self.scale;
        [
            self.x + k * (c * p[0] - s * p[1]),
            self.y + k * (s * p[0] + c * p[1]),
            self.z + k * p[2],
        ]
    }

    /// [`Xf::apply`] backwards. A parent with no scale keeps its child's
    /// offsets as if it had a tiny one.
    pub fn unapply(&self, p: [f64; 3]) -> [f64; 3] {
        let (s, c) = self.rotation.to_radians().sin_cos();
        let k = if self.scale.abs() < 1e-9 {
            1e-9
        } else {
            self.scale
        };
        let (dx, dy) = ((p[0] - self.x) / k, (p[1] - self.y) / k);
        [c * dx + s * dy, -s * dx + c * dy, (p[2] - self.z) / k]
    }

    /// A child with transform `local` in this space, in the space around.
    pub fn then(&self, local: &Xf) -> Xf {
        let [x, y, z] = self.apply([local.x, local.y, local.z]);
        Xf {
            x,
            y,
            z,
            rotation: self.rotation + local.rotation,
            scale: self.scale * local.scale,
            opacity: self.opacity * local.opacity,
        }
    }
}

/// Every layer of `scene` (evaluated into `layers`, same order) moved into
/// world space: a parented layer's transform goes through its parent's,
/// its parent's parent's and so on. A cycle (which loading refuses) is
/// broken where it closes.
pub fn compose(scene: &Scene, layers: &mut [Drawn]) {
    if scene.layers.iter().all(|l| l.parent.is_empty()) {
        return;
    }
    let parent: Vec<Option<usize>> = scene
        .layers
        .iter()
        .map(|l| scene.layers.iter().position(|p| p.id == l.parent))
        .collect();
    let local: Vec<Xf> = layers.iter().map(Xf::of).collect();
    let mut world: Vec<Option<Xf>> = vec![None; layers.len()];
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
    for i in 0..layers.len() {
        let w = resolve(i, 0, &parent, &local, &mut world);
        let d = &mut layers[i];
        (d.x, d.y, d.space.z) = (w.x, w.y, w.z);
        (d.rotation, d.scale, d.opacity) = (w.rotation, w.scale, w.opacity);
    }
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

/// Rewrite a layer's local transform through `map`, a function from a
/// point in its old parent's space to the new one's: `x`, `y` (and `z`
/// with `three`) key by key, each key's point completed by the other
/// tracks at that key's time; `rotation` shifted by `turn` degrees and
/// `scale` (and position handles) multiplied by `grow`.
// ponytail: when `map` turns and only one of x/y is keyed, the plain one is
// solved at `t` and is exact there alone; key both to keep every key exact.
fn remap(
    l: &mut Layer,
    t: f64,
    three: bool,
    map: &dyn Fn([f64; 3]) -> [f64; 3],
    turn: f64,
    grow: f64,
) {
    let (x, y, z) = (l.x.clone(), l.y.clone(), l.z.clone());
    let at = |tk: f64| [x.at(tk), y.at(tk), z.at(tk)];
    let axis = |a: &mut Anim<f64>, i: usize| match a {
        Anim::Value(v) => *v = map(at(t))[i],
        Anim::Keys(keys) => {
            for k in keys {
                k.v = map(at(k.t))[i];
                for h in k.in_.iter_mut().chain(k.out.iter_mut()) {
                    h[1] *= grow;
                }
            }
        }
    };
    axis(&mut l.x, 0);
    axis(&mut l.y, 1);
    if three {
        axis(&mut l.z, 2);
    }
    let each = |a: &mut Anim<f64>, f: &dyn Fn(f64) -> f64, handles: f64| match a {
        Anim::Value(v) => *v = f(*v),
        Anim::Keys(keys) => {
            for k in keys {
                k.v = f(k.v);
                for h in k.in_.iter_mut().chain(k.out.iter_mut()) {
                    h[1] *= handles;
                }
            }
        }
    };
    each(&mut l.rotation, &|r| r + turn, 1.);
    each(&mut l.scale, &|s| s * grow, grow);
}

/// Layer `id` of `scene` attached to `parent` (`None` detaches it), still
/// where it is on screen at `t`: its local position, rotation and scale are
/// rewritten from the old parent's space into the new one's, through both
/// parents' world transforms at `t`, as Cavalry and After Effects do.
/// Returns the rewritten layer; refuses a parent that is missing, is the
/// layer, or hangs under it (a cycle).
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
    let frame = eval(project, scene, t);
    let world = |pid: &str| {
        (!pid.is_empty())
            .then(|| scene.layers.iter().position(|l| l.id == pid))
            .flatten()
            .map_or(Xf::FRAME, |j| Xf::of(&frame.layers[j]))
    };
    let (old, new) = (world(&scene.layers[i].parent), world(parent.unwrap_or("")));
    if new.scale.abs() < 1e-9 {
        return Err(format!("`{}` has no scale at {t}s", parent.unwrap_or("")));
    }
    let mut l = scene.layers[i].clone();
    let three = scene.mode == Mode::ThreeD;
    remap(
        &mut l,
        t,
        three,
        &|p| new.unapply(old.apply(p)),
        old.rotation - new.rotation,
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
            remap(l, t, true, &|p| screen(p), turn, grow);
        }
        for a in [&mut l.z, &mut l.rx, &mut l.ry] {
            *a = Anim::Value(0.);
        }
    }
    out
}
