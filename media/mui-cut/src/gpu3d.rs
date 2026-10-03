//! A 3D frame on the GPU. Every layer's 2D content is painted by the
//! canvas's own Vello engine into one atlas, then mui-stage sets each on a
//! slab in the lit scene the frame's [`View`] describes and draws it into
//! the target. The atlas is repainted only when some layer's content (not
//! its placement) changes, so a card flying about costs no Vello work. A
//! layer with effects gets room in the atlas for what they spread, and its
//! box there runs through its stack before the slabs sample it.
//!
//! A backdrop effect (`glass`, `light_wrap`) reads the scene behind its
//! layer: the shot is drawn once without that layer and all that is nearer
//! the camera, that frame is mapped through the layer's plane onto its box
//! (`warp.wgsl`), and the stack runs over it; layers farthest first, so a
//! pane sees the panes behind it. Glass then draws unlit: what it shows is
//! already lit.
use mui_stage::{
    Ao, Environment, Floor, Fog, Light, LightKind, Mat4, Material, Model, Plane, Post, STUDIO,
    Shot, Stage,
};
use mui_vello::kurbo::Affine;

use crate::render::Assets;
use crate::three::Space as Space3;
use crate::{Drawn, Frame, GpuCanvas, Kind, LightType, Quad, Rgba, View};

/// Gutter around each layer in the atlas, pixels, and its mip levels: a
/// slab seen edge-on samples a long anisotropic footprint (up to 16 texels
/// of level 2), which must stay inside its own gutter.
const PAD: u32 = 32;
const MIPS: u32 = 3;
const ATLAS: u32 = 4096;

/// A layer's box in the atlas: top left, size in pixels, the pixels per
/// project pixel it was painted at, and the room its effects take each
/// side, pixels.
#[derive(Clone, Debug, PartialEq)]
struct Slot {
    at: [u32; 2],
    px: [u32; 2],
    k: f64,
    reach: u32,
}

type Painted = (Vec<Drawn>, Vec<Slot>, [u32; 2], (f64, u32));

pub(crate) struct Space {
    stage: Stage,
    atlas: [u32; 2],
    /// What the atlas holds: each layer's content with its placement
    /// zeroed, where it is, and the frame's time and seed if an effect
    /// moves with them.
    painted: Option<Painted>,
    /// Layer effects, run over boxes of the atlas.
    fx: Option<crate::fx::gpu::Passes>,
    /// Skinned parts' meshes, by id, and the animation time they are bent
    /// to.
    skinned: std::collections::HashMap<String, f64>,
    /// The ray-traced glass ([`GpuCanvas::glass`]), made by the first
    /// frame that wants it, and the atlas it samples.
    #[cfg(not(target_arch = "wasm32"))]
    rt: Option<mui_stage_rt::Rt>,
    atlas_tex: Option<wgpu::Texture>,
    /// The frame behind a backdrop layer, at the canvas's size.
    behind: Option<([u32; 2], wgpu::TextureView)>,
}

/// Whether `stack` reads what is behind its layer.
fn backdrop(stack: &[crate::fx::Fx]) -> bool {
    stack
        .iter()
        .any(|f| crate::fx::def(&f.kind).is_some_and(|(_, d)| d.backdrop))
}

/// How far `m`'s origin is from the camera of `vp`: clip w.
fn depth(vp: Mat4, m: Mat4) -> f32 {
    (vp * m).0[15]
}

/// The 3x3 map, rows first, from a pixel of a layer's box in the atlas
/// (`k` pixels per unit, the plane `size` units across) to the screen's uv,
/// homogeneous, through `vp * model`.
fn box_to_screen(vp: Mat4, model: Mat4, size: [f32; 2], k: f32) -> [f32; 9] {
    let c = (vp * model).0;
    let at = |r: usize, col: usize| c[col * 4 + r];
    // Box pixel (x, y) is (x / k - w / 2, h / 2 - y / k) on the plane.
    let row = |r: usize| {
        [
            at(r, 0) / k,
            -at(r, 1) / k,
            -at(r, 0) * size[0] / 2. + at(r, 1) * size[1] / 2. + at(r, 3),
        ]
    };
    let (x, y, w) = (row(0), row(1), row(3));
    // Clip to uv: u = (x / w + 1) / 2, v = (1 - y / w) / 2.
    let u: [f32; 3] = std::array::from_fn(|i| 0.5 * (x[i] + w[i]));
    let v: [f32; 3] = std::array::from_fn(|i| 0.5 * (w[i] - y[i]));
    [u[0], u[1], u[2], v[0], v[1], v[2], w[0], w[1], w[2]]
}

/// sRGB bytes to linear light.
pub(crate) fn linear(c: Rgba) -> [f32; 3] {
    std::array::from_fn(|i| {
        let s = f32::from(c.0[i]) / 255.;
        if s <= 0.04045 {
            s / 12.92
        } else {
            ((s + 0.055) / 1.055).powf(2.4)
        }
    })
}

/// Shelf-pack `sizes` into rows `width` wide; the boxes' corners and the
/// height used.
fn pack(sizes: &[[u32; 2]], width: u32) -> (Vec<[u32; 2]>, u32) {
    let (mut x, mut y, mut row) = (0, 0, 0);
    let at = sizes
        .iter()
        .map(|&[w, h]| {
            if x + w > width {
                (x, y, row) = (0, y + row, 0);
            }
            let p = [x, y];
            x += w;
            row = row.max(h);
            p
        })
        .collect();
    (at, y + row)
}

impl Space {
    pub fn new(stage: Stage) -> Self {
        Self {
            stage,
            atlas: [ATLAS, 256],
            painted: None,
            fx: None,
            skinned: std::collections::HashMap::new(),
            #[cfg(not(target_arch = "wasm32"))]
            rt: None,
            atlas_tex: None,
            behind: None,
        }
    }

    pub fn draw(
        &mut self,
        canvas: &mut GpuCanvas,
        assets: &Assets,
        frame: &Frame,
        view: &View,
        target: &wgpu::TextureView,
        format: wgpu::TextureFormat,
    ) -> Result<Vec<Quad>, String> {
        let [fw, fh] = frame.size.map(f64::from);
        let out = f64::from(canvas.size()[1]) / fh;
        #[cfg(not(target_arch = "wasm32"))]
        if canvas.glass.is_some() && self.rt.is_none() {
            let [w, h] = canvas.size();
            let mut rt = mui_stage_rt::Rt::new(&canvas.device, &canvas.queue, w, h)
                .map_err(|e| e.to_string())?;
            if let Some(atlas) = &self.atlas_tex {
                rt.layer("atlas", atlas);
            }
            self.rt = Some(rt);
        }
        // A plugin layer is a slab per part; overlays are drawn flat after.
        let layers: Vec<Drawn> = frame
            .layers
            .iter()
            .filter(|l| !l.space.overlay)
            .flat_map(|l| assets.slabs(l))
            .collect();
        let w = |p: [f64; 3]| crate::three::world(frame.size, p);
        let wd = |d: [f64; 3]| [d[0] as f32, -d[1] as f32, -d[2] as f32];

        // Each drawn layer's content, placement zeroed.
        let shown = |l: &&Drawn| {
            !matches!(
                l.kind,
                Kind::Camera { .. } | Kind::Light { .. } | Kind::Model { .. } | Kind::Audio { .. }
            ) && l.opacity > 0.
                && l.scale != 0.
        };
        let contents: Vec<Drawn> = layers
            .iter()
            .filter(shown)
            .map(|l| Drawn {
                x: 0.,
                y: 0.,
                rotation: 0.,
                scale: 1.,
                opacity: 1.,
                space: Space3::default(),
                ..l.clone()
            })
            .collect();
        let elements = contents
            .iter()
            .map(|c| assets.element(c))
            .collect::<Result<Vec<_>, _>>()?;
        let cam = &view.camera;
        let camera = crate::three::stage_camera(frame.size, cam);
        let vp = camera.view_proj((fw / fh) as f32);
        let to_px = |m: &Mat4, p: [f32; 3]| {
            let q = m.project(p);
            let q = vp.project(q);
            [
                (f64::from(q[0]) + 1.) * 0.5 * fw,
                (1. - f64::from(q[1])) * 0.5 * fh,
            ]
        };
        // Pixels per project pixel: the output's, with headroom, times
        // how magnified the layer is on screen from this camera (its scale
        // and its distance), in octaves, so a moving camera or an animated
        // scale repaints only as it crosses one.
        let base = (out * 1.5).clamp(0.5, 4.);
        let scales: Vec<f64> = layers
            .iter()
            .filter(shown)
            .map(|l| {
                let m = crate::three::pose(frame.size, l, Mat4::scale(l.scale as f32));
                let o = to_px(&m, [0., 0., 0.]);
                let seen = [[1., 0., 0.], [0., 1., 0.]]
                    .map(|a| {
                        let p = to_px(&m, a);
                        (p[0] - o[0]).hypot(p[1] - o[1])
                    })
                    .into_iter()
                    .fold(0., f64::max);
                let seen = if seen.is_finite() {
                    seen
                } else {
                    l.scale.abs()
                };
                2f64.powf(seen.max(1e-3).log2().ceil().clamp(-2., 2.))
            })
            .collect();
        let reaches: Vec<f64> = contents
            .iter()
            .map(|c| crate::fx::reach(&c.effects))
            .collect();
        let mut fit = 1.;
        let (slots, height) = loop {
            let ks: Vec<f64> = scales.iter().map(|s| base * s * fit).collect();
            let rs: Vec<u32> = reaches
                .iter()
                .zip(&ks)
                .map(|(r, k)| ((r * k).ceil() as u32).min(ATLAS / 4))
                .collect();
            let px: Vec<[u32; 2]> = elements
                .iter()
                .zip(&ks)
                .zip(&rs)
                .map(|(((_, size, _), k), r)| {
                    let gutter = 2 * (PAD + r);
                    let side = |v: f64| ((v * k).ceil() as u32).clamp(1, ATLAS - gutter);
                    [side(size.width) + gutter, side(size.height) + gutter]
                })
                .collect();
            let (at, height) = pack(&px, ATLAS);
            if height <= ATLAS || fit < 0.05 {
                let slots = at
                    .into_iter()
                    .zip(px)
                    .zip(ks)
                    .zip(rs)
                    .map(|(((at, px), k), reach)| Slot { at, px, k, reach })
                    .collect::<Vec<_>>();
                break (slots, height.min(ATLAS));
            }
            fit *= 0.7;
        };
        // The atlas only grows, so a steady scene keeps one size.
        self.atlas[1] = self.atlas[1].max(height.next_multiple_of(256)).min(ATLAS);
        let moving = contents.iter().any(|c| !c.effects.is_empty());
        let stamp = if moving {
            (frame.t, frame.seed)
        } else {
            (0., 0)
        };
        let key = (contents, slots, self.atlas, stamp);
        if self.painted.as_ref() != Some(&key) {
            let atlas = self
                .stage
                .layer_target("atlas", self.atlas, format, MIPS)
                .map_err(|e| e.to_string())?;
            let view = atlas.create_view(&wgpu::TextureViewDescriptor::default());
            let placed: Vec<_> = elements
                .iter()
                .zip(&key.1)
                .map(|((scene, _, _), s)| {
                    let at = [s.at[0] + PAD + s.reach, s.at[1] + PAD + s.reach].map(f64::from);
                    (
                        scene,
                        Affine::translate((at[0], at[1])) * Affine::scale(s.k),
                    )
                })
                .collect();
            canvas.paint_scenes(assets, &placed, None, None, self.atlas, &view)?;
            let boxes: Vec<_> = key
                .0
                .iter()
                .zip(&key.1)
                // A backdrop's stack runs once the scene behind is drawn.
                .filter(|(c, _)| !c.effects.is_empty() && !backdrop(&c.effects))
                .map(|(c, s)| {
                    let at = [s.at[0] + PAD, s.at[1] + PAD];
                    let px = [s.px[0] - 2 * PAD, s.px[1] - 2 * PAD];
                    (at, px, s.k, &c.effects[..], None)
                })
                .collect();
            if !boxes.is_empty() {
                let fx = match &mut self.fx {
                    Some(fx) => fx,
                    none => none.insert(crate::fx::gpu::Passes::new(&canvas.device, format)?),
                };
                fx.boxes(canvas, frame, &atlas, &boxes);
            }
            self.stage.layer_done("atlas").map_err(|e| e.to_string())?;
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(rt) = &mut self.rt {
                rt.layer("atlas", &atlas);
            }
            self.atlas_tex = Some(atlas);
            self.painted = Some(key);
        }
        let painted = self.painted.as_ref().expect("painted above");
        let slots = &painted.1;
        // Backdrop layers: plane index, box, stack.
        let mut behind = Vec::new();

        // Planes.
        let [aw, ah] = self.atlas.map(|v| v as f32);
        let mut planes = Vec::new();
        let mut shown_i = 0;
        let mut quads = Vec::with_capacity(layers.len());
        let marker = |l: &Drawn, id: &str| {
            let [x, y] = to_px(&Mat4::IDENTITY, w([l.x, l.y, l.space.z]));
            let r = 14.;
            Quad {
                id: id.to_owned(),
                ui: None,
                pts: [
                    [x - r, y - r],
                    [x + r, y - r],
                    [x + r, y + r],
                    [x - r, y + r],
                ],
            }
        };
        let mut models = Vec::new();
        for l in &layers {
            let s = &l.space;
            let pose = |offset: Mat4| crate::three::pose(frame.size, l, offset);
            match &l.kind {
                Kind::Camera { .. } | Kind::Light { .. } => quads.push(marker(l, &l.id)),
                Kind::Model { path } => {
                    let Some(mesh) = assets.model(path) else {
                        quads.push(marker(l, &l.id));
                        continue;
                    };
                    let tall = (mesh.max[1] - mesh.min[1]).max(1e-6);
                    let k = (l.height / f64::from(tall) * l.scale) as f32;
                    let mid: [f32; 3] = std::array::from_fn(|i| -(mesh.min[i] + mesh.max[i]) / 2.);
                    let m = pose(Mat4::scale(k) * Mat4::translate(mid));
                    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
                    for i in 0..8 {
                        let c = |a: usize| {
                            if i >> a & 1 == 0 {
                                mesh.min[a]
                            } else {
                                mesh.max[a]
                            }
                        };
                        let p = to_px(&m, [c(0), c(1), c(2)]);
                        for a in 0..2 {
                            lo[a] = lo[a].min(p[a]);
                            hi[a] = hi[a].max(p[a]);
                        }
                    }
                    quads.push(Quad {
                        id: l.id.clone(),
                        ui: None,
                        pts: [
                            [lo[0], lo[1]],
                            [hi[0], lo[1]],
                            [hi[0], hi[1]],
                            [lo[0], hi[1]],
                        ],
                    });
                    let tint = linear(l.fill);
                    let pose = mesh.pose(mesh.animated().then_some(l.time));
                    for (i, part) in mesh.parts.iter().enumerate() {
                        let mut id = format!("{path}#{i}");
                        // The glTF's thickness is in model units.
                        let own = Material {
                            thickness: part.material.thickness * k,
                            ..part.material
                        };
                        if let Some(bent) = mesh.skinned(part, &pose) {
                            // Bent per layer, re-uploaded as its time moves.
                            id = format!("{id}@{}", l.id);
                            if self.skinned.get(&id) != Some(&l.time) {
                                self.stage.mesh_uv(&id, &bent, &part.uvs, &part.indices);
                                #[cfg(not(target_arch = "wasm32"))]
                                if let Some(rt) = &mut self.rt {
                                    rt.mesh(&id, &bent, &part.indices);
                                }
                                self.skinned.insert(id.clone(), l.time);
                            }
                        } else if !self.stage.has_mesh(&id) {
                            self.stage
                                .mesh_uv(&id, &part.vertices, &part.uvs, &part.indices);
                        }
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(rt) = &mut self.rt
                            && !rt.has_mesh(&id)
                        {
                            rt.mesh(&id, &part.vertices, &part.indices);
                        }
                        let maps = std::array::from_fn(|k| {
                            let img = part.maps[k]?;
                            let pic = &mesh.images[img];
                            let tid = format!("{path}@{img}:{}", k == 0);
                            if !self.stage.has_texture(&tid) {
                                self.stage.texture(&tid, &pic.rgba, pic.size, k == 0);
                            }
                            Some(tid)
                        });
                        models.push(Model {
                            mesh: id,
                            maps,
                            transform: m * mesh.place(part, &pose),
                            color: [
                                part.color[0] * tint[0],
                                part.color[1] * tint[1],
                                part.color[2] * tint[2],
                                part.color[3] * l.opacity as f32,
                            ],
                            material: s.material.map_or(own, |m| m.over(own)),
                            cast: s.cast_shadows,
                            receive: s.receive_shadows,
                        });
                    }
                }
                _ => {
                    let (_, size, corner) = assets.element(&Drawn {
                        opacity: 1.,
                        ..l.clone()
                    })?;
                    let (pw, ph) = (size.width as f32, size.height as f32);
                    let offset = [
                        (corner.x + size.width / 2.) as f32,
                        -(corner.y + size.height / 2.) as f32,
                        s.anchor_z as f32,
                    ];
                    let m = pose(Mat4::scale(l.scale as f32) * Mat4::translate(offset));
                    let (x, y) = (pw / 2., ph / 2.);
                    quads.push(Quad {
                        id: l.id.clone(),
                        ui: None,
                        pts: [[-x, y], [x, y], [x, -y], [-x, -y]]
                            .map(|[a, b]| to_px(&m, [a, b, 0.])),
                    });
                    if !shown(&l) {
                        continue;
                    }
                    let slot = &slots[shown_i];
                    shown_i += 1;
                    // A layer with effects shows the room they spread into.
                    let [x0, y0] = [slot.at[0] + PAD, slot.at[1] + PAD].map(|v| v as f32);
                    let r = slot.reach as f32;
                    let (iw, ih) = (
                        (size.width * slot.k) as f32 + 2. * r,
                        (size.height * slot.k) as f32 + 2. * r,
                    );
                    let grow = (f64::from(slot.reach) / slot.k) as f32;
                    let mut plane = Plane::new("atlas", pw + 2. * grow, ph + 2. * grow)
                        .uv([x0 / aw, y0 / ah, (x0 + iw) / aw, (y0 + ih) / ah])
                        .depth(s.extrude as f32)
                        .edge(linear(s.edge))
                        .opacity(l.opacity as f32)
                        .scale(l.scale as f32)
                        .rotate(-s.rx as f32, s.ry as f32, -l.rotation as f32)
                        .offset(offset);
                    plane.position = w([l.x, l.y, s.z]);
                    plane.cast = s.cast_shadows;
                    plane.receive = s.receive_shadows;
                    if let Some(m) = &s.material {
                        plane.material = m.over(Material::SLAB);
                    }
                    if s.extrude > 0.
                        && let Some(mut o) = assets.outline(l, size, corner)
                    {
                        o.translate(mui_geometry::Vec2::new(f64::from(grow), f64::from(grow)));
                        plane = plane.outline(std::sync::Arc::new(o));
                    }
                    let stack = &painted.0[shown_i - 1].effects;
                    if backdrop(stack) {
                        // Glass shows the lit scene behind it, and lets
                        // light through rather than casting a shadow.
                        if stack.iter().any(|f| f.kind == "glass") {
                            plane.unlit = true;
                            plane.cast = false;
                        }
                        let at = [slot.at[0] + PAD, slot.at[1] + PAD];
                        let px = [slot.px[0] - 2 * PAD, slot.px[1] - 2 * PAD];
                        let map = box_to_screen(vp, plane.model(), plane.size, slot.k as f32);
                        behind.push((planes.len(), at, px, slot.k, stack.clone(), map));
                    }
                    planes.push(plane);
                }
            }
        }

        let lights = view
            .lights
            .iter()
            .map(|l| {
                let mut out = Light::new(match l.kind {
                    LightType::Directional => LightKind::Directional,
                    LightType::Spot => LightKind::Spot,
                    LightType::Point => LightKind::Point,
                    LightType::Ambient => LightKind::Ambient,
                });
                out.color = linear(l.color).map(|c| c * l.intensity as f32);
                out.position = w(l.position);
                out.direction = wd(l.direction);
                out.range = l.range as f32;
                out.cone = l.cone as f32;
                out.feather = l.feather as f32;
                out.shadows = l.shadows;
                out.softness = l.softness as f32;
                out
            })
            .collect();
        let floor = view.ground.as_ref().map(|g| Floor {
            y: (fh / 2. - g.y) as f32,
            color: linear(g.color),
            reflect: g.reflect.clamp(0., 1.) as f32,
            falloff: 120.,
            radius: g.radius.max(1.) as f32,
            contact: g.contact.clamp(0., 1.) as f32,
            contact_height: 160.,
        });
        let fog = view.fog.as_ref().map(|f| Fog {
            color: linear(f.color),
            near: f.near as f32,
            far: f.far as f32,
        });
        // An HDRI not loaded (yet) lights as the studio does.
        let environment = view.environment.as_ref().map(|e| {
            let loaded = !e.hdri.is_empty()
                && (self.stage.has_environment(&e.hdri)
                    || assets.env(&e.hdri).is_some_and(|img| {
                        self.stage.environment(&e.hdri, img);
                        true
                    }));
            Environment {
                map: if loaded {
                    e.hdri.clone()
                } else {
                    STUDIO.into()
                },
                intensity: e.intensity as f32,
                rotation: e.rotation as f32,
                background: e.background,
            }
        });
        let ao = view.ao.as_ref().map(|a| Ao {
            strength: a.strength.max(0.) as f32,
            radius: a.radius.max(0.) as f32,
        });
        let blur = (cam.aperture * out).min(64.) as f32;
        let shot = Shot {
            planes,
            models,
            lights,
            floor,
            fog,
            clear: Some(linear(frame.background)),
            environment,
            sky: view.sky,
            ao,
            sample: canvas.sample,
            trace: canvas.trace,
            post: Post {
                focus: cam.focus as f32,
                aperture: blur,
                max_blur: blur,
                bloom: view
                    .bloom
                    .as_ref()
                    .map_or(0., |b| b.strength.max(0.) as f32),
                threshold: view
                    .bloom
                    .as_ref()
                    .map_or(1., |b| b.threshold.max(0.) as f32),
                knee: 0.5,
                ..Post::NONE
            },
            ..Shot::new(camera)
        };
        if !behind.is_empty() {
            self.backdrops(canvas, frame, &shot, behind, vp, format)?;
        }
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(rt), Some(spp)) = (&mut self.rt, canvas.glass) {
            rt.draw(&mut self.stage, &shot, 0., spp, target, format)
                .map_err(|e| e.to_string())?;
            // Profiling: the tracer's GPU time per pass, each frame (waits).
            if std::env::var_os("MUI_RT_TIMES").is_some()
                && let Some(g) = rt.gpu_times()
            {
                eprintln!(
                    "mui-stage-rt: sky+tlas {:.2} ms, trace {:.2} ms, filter {:.2} ms, composite {:.2} ms",
                    g.tlas, g.trace, g.filter, g.composite
                );
            }
            return Ok(quads);
        }
        self.stage
            .draw(&shot, 0., target, format)
            .map_err(|e| e.to_string())?;
        Ok(quads)
    }

    /// Each backdrop layer's stack over the scene behind it, farthest
    /// first: `shot` without the layer and what is nearer, drawn, mapped
    /// onto its box, the stack run there, the atlas brought up to date.
    /// The atlas is painted again next frame: its boxes now hold results.
    fn backdrops(
        &mut self,
        canvas: &GpuCanvas,
        frame: &Frame,
        shot: &Shot,
        mut layers: Vec<Behind>,
        vp: Mat4,
        format: wgpu::TextureFormat,
    ) -> Result<(), String> {
        let size = canvas.size();
        if self.behind.as_ref().is_none_or(|b| b.0 != size) {
            let view = canvas
                .device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some("mui-cut 3D backdrop"),
                    size: wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                        | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                })
                .create_view(&wgpu::TextureViewDescriptor::default());
            self.behind = Some((size, view));
        }
        let view = self.behind.as_ref().expect("made above").1.clone();
        let atlas = self.atlas_tex.clone().ok_or("no atlas")?;
        let plane_depth: Vec<f32> = shot.planes.iter().map(|p| depth(vp, p.model())).collect();
        layers.sort_by(|a, b| plane_depth[b.0].total_cmp(&plane_depth[a.0]));
        for (i, at, px, k, stack, map) in &layers {
            let d = plane_depth[*i];
            let farther = |e: f32| e > d;
            let only = Shot {
                planes: shot
                    .planes
                    .iter()
                    .zip(&plane_depth)
                    .filter(|(_, e)| farther(**e))
                    .map(|(p, _)| p.clone())
                    .collect(),
                models: shot
                    .models
                    .iter()
                    .filter(|m| farther(depth(vp, m.transform)))
                    .cloned()
                    .collect(),
                ..shot.clone()
            };
            self.stage
                .draw(&only, 0., &view, format)
                .map_err(|e| e.to_string())?;
            let fx = match &mut self.fx {
                Some(fx) => fx,
                none => none.insert(crate::fx::gpu::Passes::new(&canvas.device, format)?),
            };
            fx.boxes(
                canvas,
                frame,
                &atlas,
                &[(*at, *px, *k, stack, Some((&view, *map)))],
            );
            self.stage.layer_done("atlas").map_err(|e| e.to_string())?;
        }
        self.painted = None;
        Ok(())
    }
}

/// A backdrop layer of a 3D frame: its plane's index in the shot, its box
/// in the atlas (corner, size, pixels per unit), its stack, and the map
/// from the box to the screen.
type Behind = (usize, [u32; 2], [u32; 2], f64, Vec<crate::fx::Fx>, [f32; 9]);
