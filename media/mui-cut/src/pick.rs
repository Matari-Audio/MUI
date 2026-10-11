//! Interact in 3D: which plugin layer, and where on its UI, a point of the
//! viewport hits. The pointer's ray from the camera is met with each part
//! slab's plane as the 3D view draws it (turned, parented, exploded), and
//! the nearest slab it lands inside wins. 2D layers carry their own map
//! (`Quad::ui`); this is the 3D one.
use mui_stage::Mat4;
use serde::Serialize;

use crate::render::Assets;
use crate::{Drawn, Frame, Kind};

/// A hit on a plugin layer's slab.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Hit {
    /// The plugin layer.
    pub layer: String,
    /// The slab's image: pass it back as `only` to keep a drag on it.
    pub slab: String,
    /// The point in the UI's own pixels, where the adapter's pointer goes.
    pub ui: [f64; 2],
}

/// The plugin layer under `(x, y)` (project pixels) in 3D frame `f`, and
/// the point on its UI. With `only`, that slab's plane wherever the ray
/// meets it, inside it or not: a knob keeps turning as the drag leaves it.
pub fn pick(assets: &Assets, f: &Frame, x: f64, y: f64, only: Option<&str>) -> Option<Hit> {
    let view = f.view.as_ref()?;
    let [fw, fh] = f.size.map(f64::from);
    let vp = crate::three::stage_camera(f.size, &view.camera).view_proj((fw / fh) as f32);
    let (nx, ny) = ((x / fw * 2. - 1.) as f32, (1. - y / fh * 2.) as f32);
    let mut best: Option<(f32, Hit)> = None;
    for l in f.layers.iter().filter(|l| l.opacity > 0. && l.scale != 0.) {
        let Some(cap) = l.plugin.as_ref().and_then(|p| assets.capture(&p.state)) else {
            continue;
        };
        for s in assets.slabs(l) {
            let Kind::Image { path } = &s.kind else {
                continue;
            };
            if only.is_some_and(|o| o != path) {
                continue;
            }
            // The UI's pixels the slab shows: its fragment's rect.
            let Some(rect) = cap.fragments.iter().find_map(|fr| {
                let src = |s: &str| *path == format!("{}/{s}", crate::plugin::CACHE);
                if src(&fr.src) {
                    Some(fr.rect)
                } else {
                    fr.free.as_ref().filter(|i| src(&i.src)).map(|i| i.rect)
                }
            }) else {
                continue;
            };
            let Ok((_, size, corner)) = assets.element(&Drawn {
                opacity: 1.,
                ..s.clone()
            }) else {
                continue;
            };
            let (pw, ph) = (size.width as f32, size.height as f32);
            let offset = [
                (corner.x + size.width / 2.) as f32,
                -(corner.y + size.height / 2.) as f32,
                s.space.anchor_z as f32,
            ];
            let pose = crate::three::pose(
                f.size,
                &s,
                Mat4::scale(s.scale as f32) * Mat4::translate(offset),
            );
            let Some((a, b, depth)) = meet(&(vp * pose).0, nx, ny) else {
                continue;
            };
            let inside = a.abs() <= pw / 2. && b.abs() <= ph / 2.;
            if !(inside || only.is_some()) || best.as_ref().is_some_and(|(d, _)| *d <= depth) {
                continue;
            }
            let (u, v) = (f64::from((a + pw / 2.) / pw), f64::from((ph / 2. - b) / ph));
            let hit = Hit {
                layer: l.id.clone(),
                slab: path.clone(),
                ui: [rect[0] + u * rect[2], rect[1] + v * rect[3]],
            };
            best = Some((depth, hit));
        }
    }
    best.map(|(_, h)| h)
}

/// Where the ray through clip-space `(nx, ny)` meets the plane `z = 0` of
/// the model whose clip transform is `m` (column-major): its `(a, b)` on
/// that plane and its depth; none when the plane is edge-on or behind.
fn meet(m: &[f32; 16], nx: f32, ny: f32) -> Option<(f32, f32, f32)> {
    // clip = m * (a, b, 0, 1); x/w = nx and y/w = ny are linear in a, b.
    let row = |r: usize| [m[r], m[4 + r], m[12 + r]];
    let (x, y, z, w) = (row(0), row(1), row(2), row(3));
    let (p, q) = (
        [x[0] - nx * w[0], x[1] - nx * w[1], x[2] - nx * w[2]],
        [y[0] - ny * w[0], y[1] - ny * w[1], y[2] - ny * w[2]],
    );
    let det = p[0] * q[1] - p[1] * q[0];
    if det.abs() < 1e-12 {
        return None;
    }
    let a = (-p[2] * q[1] + p[1] * q[2]) / det;
    let b = (-p[0] * q[2] + p[2] * q[0]) / det;
    let cw = w[0] * a + w[1] * b + w[2];
    (cw > 0.).then(|| (a, b, (z[0] * a + z[1] * b + z[2]) / cw))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Project, eval};

    fn solid_png(w: u32, h: u32) -> Vec<u8> {
        let px: Vec<u8> = (0..w * h).flat_map(|_| [40, 40, 46, 255]).collect();
        let mut out = Vec::new();
        let mut enc = png::Encoder::new(&mut out, w, h);
        enc.set_color(png::ColorType::Rgba);
        enc.write_header().unwrap().write_image_data(&px).unwrap();
        out
    }

    /// A tilted, parented, exploded 3D plugin layer: a point of its part
    /// through the camera picks that part back, at that point of the UI.
    #[test]
    fn a_pointer_in_3d_lands_on_the_part_under_it() {
        let p = Project::load(
            r##"{"size":[640,360],"fps":30,"scenes":[{"name":"a","duration":2,"mode":"3d","layers":[
              {"id":"rig","kind":"rect","x":320,"y":180,"ry":25,"fill":"#00000000"},
              {"id":"syn","kind":"plugin","source":{"bin":"x"},"parent":"rig","x":0,"y":0,"scale":1.5,
               "rx":12,"explode":0.6,"backdrop":0.35}]}]}"##,
        )
        .unwrap();
        let f = eval(&p, &p.scenes[0], 0.);
        let l = f.layers.iter().find(|l| l.id == "syn").unwrap();
        let key = &l.plugin.as_ref().unwrap().state;
        let cap = serde_json::json!({"width": 200, "height": 100,
        "parts": [{"path": "a", "id": "a", "frame": [10, 10, 180, 80]}],
        "layers": [
            {"group": "background", "rect": [0, 0, 200, 100], "src": "img/bg.png"},
            {"group": "a", "rect": [120, 20, 60, 60], "src": "img/a.png"},
        ]});
        let mut assets = Assets::default();
        let c = crate::plugin::CACHE;
        assets
            .add_asset(&format!("{c}/{key}.json"), cap.to_string().as_bytes())
            .unwrap();
        assets
            .add_asset(&format!("{c}/img/bg.png"), &solid_png(200, 100))
            .unwrap();
        assets
            .add_asset(&format!("{c}/img/a.png"), &solid_png(60, 60))
            .unwrap();

        // The part's UI point (140, 35) as the 3D view draws it.
        let slab = assets
            .slabs(l)
            .into_iter()
            .find(|s| s.id == "syn#a")
            .unwrap();
        let view = f.view.as_ref().unwrap();
        let vp = crate::three::stage_camera(f.size, &view.camera).view_proj(640. / 360.);
        let k = slab.scale as f32;
        let local = [(140. - 150.) as f32, -(35. - 50.) as f32, 0.];
        let m = crate::three::pose(f.size, &slab, Mat4::scale(k));
        let q = vp.project(m.project(local));
        let at = (f64::from(q[0] + 1.) * 320., f64::from(1. - q[1]) * 180.);
        let hit = pick(&assets, &f, at.0, at.1, None).expect("a hit");
        assert_eq!(hit.layer, "syn");
        assert_eq!(
            hit.slab,
            format!("{c}/img/a.png"),
            "the exploded part is nearer than the backdrop"
        );
        assert!(
            (hit.ui[0] - 140.).abs() < 0.5 && (hit.ui[1] - 35.).abs() < 0.5,
            "{hit:?}"
        );

        // Off the slabs: nothing, unless the drag holds on to one.
        assert_eq!(pick(&assets, &f, 2., 2., None), None);
        let held = pick(&assets, &f, 2., 2., Some(&hit.slab)).expect("held");
        assert!(held.ui[0] < 120. && held.ui[1] < 20., "{held:?}");
    }
}
