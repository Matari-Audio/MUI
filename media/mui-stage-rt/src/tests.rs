//! The tracer against analytic optics. GPU tests skip when no adapter has
//! hardware ray queries, and say so.
use super::*;
use mui_stage::{Camera, Material, Model, Post};

const W: u32 = 400;
const H: u32 = 400;

fn rig() -> Option<(Stage, Rt)> {
    match open(W, H) {
        Ok(r) => Some(r),
        Err(Error::Unavailable(why)) => {
            eprintln!("skipped: {why}");
            None
        }
        Err(e) => panic!("{e}"),
    }
}

/// A layer `size` pixels from `f(x, y)`, painted into the stage and handed
/// to the tracer.
fn paint(stage: &mut Stage, rt: &mut Rt, id: &str, size: [u32; 2], f: impl Fn(u32, u32) -> [u8; 4]) {
    let tex = stage
        .layer_target(id, size, wgpu::TextureFormat::Rgba8Unorm, 1)
        .unwrap();
    let data: Vec<u8> = (0..size[1])
        .flat_map(|y| (0..size[0]).map(move |x| (x, y)))
        .flat_map(|(x, y)| f(x, y))
        .collect();
    rt.queue.write_texture(
        tex.as_image_copy(),
        &data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size[0] * 4),
            rows_per_image: None,
        },
        tex.size(),
    );
    stage.layer_done(id).unwrap();
    rt.layer(id, &tex);
}

/// An unlit shot through a long lens (rays all but parallel): faces show
/// their layer's pixels.
fn shot(planes: Vec<Plane>) -> Shot {
    Shot {
        planes,
        clear: Some([0.; 3]),
        post: Post::NONE,
        ssr: false,
        ..Shot::new(Camera::front(400., 2.))
    }
}

/// Black left of x = 0, white right of it, 600 units behind the origin.
fn edge(stage: &mut Stage, rt: &mut Rt) -> Plane {
    paint(stage, rt, "edge", [1024, 4], |x, _| {
        if x < 512 { [0, 0, 0, 255] } else { [255; 4] }
    });
    // The pane over it: clear glass, white.
    paint(stage, rt, "pane", [4, 4], |_, _| [255; 4]);
    Plane::new("edge", 2400., 2400.).at(0., 0., -600.)
}

fn glass(ior: f32) -> Material {
    Material {
        transmission: 1.,
        ior,
        roughness: 0.,
        ..Material::SLAB
    }
}

/// Where channel `c` of row `y` crosses halfway from its dark left to its
/// bright right, in pixels, interpolated between the two pixels around it.
fn crossing(f: &Frame, y: u32, c: usize) -> f32 {
    let at = |x: u32| f.rgba[((y * f.width + x) * 4) as usize + c];
    let (lo, hi) = (at(20), at(f.width - 20));
    let mid = (lo + hi) * 0.5;
    assert!(hi - lo > 0.2, "row {y} has no edge: {lo} .. {hi}");
    for x in 20..f.width - 21 {
        let (a, b) = (at(x), at(x + 1));
        if a < mid && b >= mid {
            return x as f32 + 0.5 + (mid - a) / (b - a);
        }
    }
    panic!("row {y}: no crossing");
}

/// How far a slab `d` thick, turned `theta` to the ray, moves the ray
/// sideways at index `n`.
fn shift(d: f32, theta: f32, n: f32) -> f32 {
    let (s, c) = theta.sin_cos();
    d * s * (1. - c / (n * n - s * s).sqrt())
}

/// World units at `z` to pixels, for the test's camera.
fn px_per_unit(z: f32) -> f32 {
    let cam = Camera::front(400., 2.);
    let dist = cam.eye[2] - z;
    H as f32 / (2. * dist * (cam.fov.to_radians() * 0.5).tan())
}

/// A slab over the top half of the frame, `d` thick and turned `deg`
/// about y.
fn slab(d: f32, deg: f32, m: Material) -> Plane {
    Plane::new("pane", 700., 190.)
        .at(0., 100., 0.)
        .offset([0., 0., d * 0.5])
        .rotate(0., deg, 0.)
        .depth(d)
        .material(m)
}

#[test]
fn a_slab_moves_an_edge_by_the_analytic_amount() {
    let Some((mut stage, mut rt)) = rig() else {
        return;
    };
    let bg = edge(&mut stage, &mut rt);
    let (d, deg, n) = (100., 30f32, 1.5);
    let row = |rt: &mut Rt, stage: &mut Stage, n: f32| {
        let s = shot(vec![bg.clone(), slab(d, deg, glass(n))]);
        crossing(&rt.render(stage, &s, 64).unwrap(), 100, 1)
    };
    // Glass of index 1 bends nothing: that is where the edge is.
    let moved = row(&mut rt, &mut stage, n) - row(&mut rt, &mut stage, 1.);
    let want = shift(d, deg.to_radians(), n) * px_per_unit(-600.);
    assert!(
        (moved.abs() - want).abs() < 0.15,
        "moved {moved} px, Snell says {want} px"
    );
    // The raster glass cannot: it guesses along the screen.
    assert!(want > 10.);
}

#[test]
fn dispersion_splits_red_from_blue() {
    let Some((mut stage, mut rt)) = rig() else {
        return;
    };
    let bg = edge(&mut stage, &mut rt);
    let split = |rt: &mut Rt, stage: &mut Stage, k: f32| {
        let m = Material {
            dispersion: k,
            ..glass(1.5)
        };
        let s = shot(vec![bg.clone(), slab(300., 45., m)]);
        let f = rt.render(stage, &s, 256).unwrap();
        // Red bends less than blue.
        crossing(&f, 100, 0) - crossing(&f, 100, 2)
    };
    let none = split(&mut rt, &mut stage, 0.);
    let some = split(&mut rt, &mut stage, 1.);
    assert!(none.abs() < 0.25, "no dispersion, yet red and blue {none} px apart");
    // Abbe 20: the bands' indices differ by about 0.025, about 2 px here.
    assert!(some.abs() > 1., "dispersion 1 split red and blue only {some} px");
}

/// A right-angle prism: its leg faces the camera at z = 0, its hypotenuse
/// runs from (-s, z 0) to (s, z -2s), a 45 degree turn, and its other leg
/// at x = s. Flat normals, outward.
fn prism(s: f32) -> (Vec<[f32; 6]>, Vec<u32>) {
    let tri = [[-s, 0.], [s, 0.], [s, -2. * s]];
    let mut v = Vec::new();
    let mut quad = |a: [f32; 2], b: [f32; 2], n: [f32; 2]| {
        for (p, y) in [(a, -s), (b, -s), (b, s), (a, s)] {
            v.push([p[0], y, p[1], n[0], 0., n[1]]);
        }
    };
    let r = std::f32::consts::FRAC_1_SQRT_2;
    quad(tri[0], tri[1], [0., 1.]);
    quad(tri[1], tri[2], [1., 0.]);
    quad(tri[2], tri[0], [-r, -r]);
    for y in [-s, s] {
        for p in tri {
            v.push([p[0], y, p[1], 0., y.signum(), 0.]);
        }
    }
    let mut ix = Vec::new();
    for q in 0..3 {
        let b = q * 4;
        ix.extend_from_slice(&[b, b + 1, b + 2, b, b + 2, b + 3]);
    }
    ix.extend_from_slice(&[12, 13, 14, 15, 16, 17]);
    (v, ix)
}

#[test]
fn total_internal_reflection_turns_light_in_a_prism() {
    let Some((mut stage, mut rt)) = rig() else {
        return;
    };
    paint(&mut stage, &mut rt, "red", [4, 4], |_, _| [255, 0, 0, 255]);
    paint(&mut stage, &mut rt, "blue", [4, 4], |_, _| [0, 0, 255, 255]);
    let (v, ix) = prism(120.);
    rt.mesh("prism", &v, &ix);
    stage.mesh("prism", &v, &ix);
    // Red off to the right, facing back at the prism; blue far behind.
    let red = Plane::new("red", 4000., 4000.).at(1500., 0., -120.).rotate(0., -90., 0.);
    let blue = Plane::new("blue", 20000., 20000.).at(0., 0., -3000.);
    let mut at = |ior: f32| {
        let mut s = shot(vec![red.clone(), blue.clone()]);
        s.models.push(Model {
            mesh: "prism".into(),
            transform: Mat4::IDENTITY,
            color: [1.; 4],
            material: glass(ior),
            cast: false,
            receive: false,
            maps: [None, None, None],
        });
        let f = rt.render(&mut stage, &s, 64).unwrap();
        let i = ((200 * W + 200) * 4) as usize;
        [f.rgba[i], f.rgba[i + 2]]
    };
    // At 1.5 the critical angle is 41.8 degrees: the 45 degree face is a
    // mirror and the prism shows red.
    let [r, b] = at(1.5);
    assert!(r > 0.85 && b < 0.1, "ior 1.5: red {r} blue {b}");
    // At 1.3 it is 50.3 degrees: most light leaves through that face.
    let [r, b] = at(1.3);
    assert!(r < 0.6, "ior 1.3 still mirrors: red {r} blue {b}");
}

impl Rt {
    /// The glass as the composite reads it: colour and coverage.
    fn glass_now(&self) -> Vec<[f32; 4]> {
        let size = u64::from(self.width) * u64::from(self.height) * 16;
        let rb = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = self.device.create_command_encoder(&Default::default());
        enc.copy_buffer_to_buffer(&self.ping[self.out], 0, &rb, 0, size);
        self.queue.submit([enc.finish()]);
        rb.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        self.finish().unwrap();
        let v = bytemuck::cast_slice(&rb.slice(..).get_mapped_range().unwrap()).to_vec();
        v
    }
}

#[test]
fn the_mean_converges_as_samples_come_in() {
    let Some((mut stage, mut rt)) = rig() else {
        return;
    };
    let bg = edge(&mut stage, &mut rt);
    let m = Material {
        roughness: 0.3,
        dispersion: 0.5,
        ..glass(1.5)
    };
    let s = shot(vec![bg, slab(100., 30., m)]);
    let run = |rt: &mut Rt, n: u32| {
        rt.reset();
        for _ in 0..n.div_ceil(16) {
            rt.trace(&s, n.min(16)).unwrap();
        }
        rt.glass_now()
    };
    rt.denoise(false);
    let truth = run(&mut rt, 4096);
    let err = |rt: &mut Rt, n: u32| {
        let got = run(rt, n);
        let (mut sum, mut k) = (0., 0);
        for (a, b) in got.iter().zip(&truth) {
            if b[3] > 0.99 {
                sum += (0..3).map(|c| (a[c] - b[c]).powi(2)).sum::<f32>();
                k += 1;
            }
        }
        assert!(k > 1000, "too little glass on screen");
        (sum / k as f32).sqrt()
    };
    let e = [1, 16, 256].map(|n| err(&mut rt, n));
    // Monte Carlo halves the error per four times the samples; the even
    // sequences do better.
    assert!(e[1] < e[0] * 0.35 && e[2] < e[1] * 0.35, "error does not fall with samples: {e:?}");
    // The filter cleans the first samples: one filtered sample is nearer
    // the truth than one plain one.
    rt.denoise(true);
    assert!(err(&mut rt, 1) < e[0] * 0.8, "the filter does not help");
    // The same shot adds to the sum; a moved one starts again.
    rt.trace(&s, 1).unwrap();
    assert_eq!(rt.samples(), 2);
    let mut moved = s.clone();
    moved.camera = moved.camera.orbit(1., 0.);
    rt.trace(&moved, 1).unwrap();
    assert_eq!(rt.samples(), 1);
}

#[test]
fn an_adapter_without_ray_queries_falls_back() {
    // The GL backend has no ray queries anywhere.
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::GL,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let Ok(adapter) = pollster::block_on(instance.request_adapter(&Default::default())) else {
        eprintln!("skipped: no GL adapter");
        return;
    };
    assert!(!supported(&adapter));
    // The descriptor asks for nothing it cannot have.
    assert_eq!(device_descriptor(&adapter).required_features, wgpu::Features::empty());
    let (device, queue) =
        pollster::block_on(adapter.request_device(&device_descriptor(&adapter))).unwrap();
    assert!(matches!(
        Rt::new(&device, &queue, 8, 8),
        Err(Error::Unavailable(_))
    ));
}

#[test]
fn the_shaders_have_the_stage_sky() {
    let (tracer, filter, composite) = sources().unwrap();
    assert!(tracer.contains("fn sky_seen(") && !tracer.contains("{{SKY}}"));
    assert!(filter.contains("fn denoise(") && composite.contains("fn fs_composite("));
}


#[test]
fn light_leaves_a_slab_by_its_back_face_wherever_the_print_is() {
    let Some((mut stage, mut rt)) = rig() else {
        return;
    };
    // White behind; the slab's layer covers only thin upright stripes, as
    // a value's glyphs do. Light that went in through a stripe comes out
    // of the back face well to the side of it, where the layer is clear.
    paint(&mut stage, &mut rt, "white", [4, 4], |_, _| [255; 4]);
    paint(&mut stage, &mut rt, "pane", [70, 4], |x, _| {
        if x % 7 == 0 { [255; 4] } else { [0; 4] }
    });
    let bg = Plane::new("white", 2400., 2400.).at(0., 0., -600.);
    let m = Material {
        tint: [0.5; 3],
        thickness: 100.,
        ..glass(1.5)
    };
    let s = shot(vec![bg, slab(100., 30., m)]);
    let f = rt.render(&mut stage, &s, 64).unwrap();
    let row = 100;
    let darkest = (20..W - 20)
        .map(|x| f.rgba[((row * W + x) * 4) as usize + 1])
        .fold(1f32, f32::min);
    // Half the light is left after one thickness, less the two faces'
    // reflections; not what is left after the 600 units to the backdrop.
    assert!(darkest > 0.3, "darkest {darkest}");
}
