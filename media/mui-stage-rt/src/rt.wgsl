// mui-stage-rt: a hardware ray-query path tracer for the stage's glass.
//
// One compute dispatch traces `spp` paths per pixel through the scene's
// acceleration structure and adds them to a running sum; `denoise` turns
// the sum into a mean and runs an edge-aware a-trous filter over it while
// few samples are in; `fs_composite` lays the glass pixels over the raster
// frame. Everything here is linear light until `fs_composite` encodes it.
//
// Algorithms and their sources (all permissive; see NOTICE):
// - exact Fresnel for a dielectric, and refraction by Snell's law: the
//   textbook formulas as pbrt-v4 writes them (`FrDielectric`, `Refract`,
//   Apache-2.0), rewritten here.
// - rough dielectric reflection and transmission: Walter, Marschner, Li and
//   Torrance, "Microfacet Models for Refraction through Rough Surfaces"
//   (EGSR 2007): GGX normals sampled by D(m)|m.n|, weighted by eq. 41.
// - a-trous wavelet filter with edge-stopping functions: Dammertz et al.,
//   "Edge-Avoiding A-Trous Wavelet Transform" (HPG 2010), as SVGF uses it
//   (Schied et al. 2017), without the variance estimate.
// - PCG hash: Jarzynski and Olano, "Hash Functions for GPU Rendering"
//   (JCGT 2020).

enable wgpu_ray_query;

// pos: xyz, w kind (0 none, 1 directional, 2 point, 3 spot); dir: the way
// the light travels, w cos(outer cone); color: rgb times intensity, w
// cos(inner cone); extra: x range (0 none), y casts shadows, z softness
// (directional: tan of the disc's angle; else its radius, world units).
struct Light {
    pos: vec4f,
    dir: vec4f,
    color: vec4f,
    extra: vec4f,
};

struct Globals {
    // w: samples already in the sum.
    eye: vec4f,
    // The view's axes; right.w and up.w: tan of the half field across and up.
    right: vec4f,
    up: vec4f,
    // w: samples this dispatch adds.
    fwd: vec4f,
    // x, y: size in pixels; z: bounces; w: 1 when the shot is lit.
    res: vec4f,
    // A solid background, linear; a: on.
    clear: vec4f,
    // Summed ambient light.
    ambient: vec4f,
    // The floor: colour, w on; y, reflect, falloff, radius.
    floor: vec4f,
    floor2: vec4f,
    // 1 / the mean of each colour's spectral weight over 380..700 nm;
    // w the time in seconds, which ripples run by.
    spec: vec4f,
    // The sky, packed as mui-stage packs it (its `sky_seen` reads these).
    sky0: vec4f,
    sky1: vec4f,
    sky2: vec4f,
    sky3: vec4f,
    lights: array<Light, 4>,
};

// An instance: what it is and what it is made of.
// kind: x 0 plane, 1 mesh, 2 floor; y the layer texture slot; z the first
// index of its walls (a plane) or triangles (a mesh) in `indices`; w the
// first vertex in `vertices`.
struct Inst {
    kind: vec4u,
    // The layer texture's part on the face: u0, v0, u1, v1.
    uv: vec4f,
    // Layer size in its own units, slab depth, glow.
    size: vec4f,
    // Edge colour (a plane's walls and back; a mesh's base), opacity.
    edge: vec4f,
    // metallic, roughness, transmission, ior.
    mat: vec4f,
    // Path length `tint` is measured over (0: once per crossing),
    // dispersion (20 / Abbe), print, bevel.
    mat2: vec4f,
    // What is left of white light after mat2.x inside; w receives shadows.
    tint: vec4f,
    // Pressed glass: the steepest slope of its reeds, hammered dimples and
    // ripples; and their sizes.
    relief: vec4f,
    relief_scale: vec4f,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var acc: acceleration_structure;
@group(0) @binding(2) var<storage, read> insts: array<Inst>;
// Position and normal per vertex, six floats.
@group(0) @binding(3) var<storage, read> vertices: array<f32>;
@group(0) @binding(4) var<storage, read> indices: array<u32>;
// rgb: summed glass radiance (times coverage); a: summed coverage.
@group(0) @binding(5) var<storage, read_write> accum: array<vec4f>;
// The first glass surface: its normal and distance, for the filter.
@group(0) @binding(6) var<storage, read_write> guide: array<vec4f>;
@group(0) @binding(7) var samp: sampler;
@group(0) @binding(8) var tex0: texture_2d<f32>;
@group(0) @binding(9) var tex1: texture_2d<f32>;
@group(0) @binding(10) var tex2: texture_2d<f32>;
@group(0) @binding(11) var tex3: texture_2d<f32>;

const PI: f32 = 3.14159265;
const BIG: f32 = 1e7;
const KIND_PLANE: u32 = 0u;
const KIND_MESH: u32 = 1u;
const KIND_FLOOR: u32 = 2u;

// --- random numbers --------------------------------------------------------

var<private> rng: u32;
fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}
fn rand() -> f32 {
    rng = pcg(rng);
    return f32(rng >> 8u) / 16777216.;
}
fn rand2() -> vec2f { return vec2f(rand(), rand()); }
// This sample's place in the spectrum, 0..1.
var<private> hero: f32;
// The instance the path is inside, or NONE. Its layer says where light
// gets in; once in, every face of it is glass, so the way out is too.
const NONE: u32 = 0xFFFFFFFFu;
var<private> body: u32 = NONE;

// --- the sky: spliced from mui-stage's stage.wgsl at build time -----------

{{SKY}}

// --- layer textures --------------------------------------------------------

// A layer's pixels at `uv`: Vello paints premultiplied sRGB; straight
// linear colour and coverage out.
fn layer(slot: u32, uv: vec2f) -> vec4f {
    var c: vec4f;
    switch slot {
        case 0u: { c = textureSampleLevel(tex0, samp, uv, 0.); }
        case 1u: { c = textureSampleLevel(tex1, samp, uv, 0.); }
        case 2u: { c = textureSampleLevel(tex2, samp, uv, 0.); }
        case 3u: { c = textureSampleLevel(tex3, samp, uv, 0.); }
        default: { return vec4f(1.); }
    }
    if (c.a <= 0.) { return vec4f(0.); }
    let s = clamp(c.rgb / c.a, vec3f(0.), vec3f(1.));
    let lin = select(pow((s + 0.055) / 1.055, vec3f(2.4)), s / 12.92, s <= vec3f(0.04045));
    return vec4f(lin, c.a);
}
// A plane's local point to its layer uv (local is y up, centred).
fn face_uv(inst: Inst, local: vec3f) -> vec2f {
    let f = vec2f(local.x / inst.size.x + 0.5, 0.5 - local.y / inst.size.y);
    return mix(inst.uv.xy, inst.uv.zw, f);
}

// --- tracing ---------------------------------------------------------------

struct Hit {
    hit: bool,
    t: f32,
    inst: u32,
    geom: u32,
    prim: u32,
    bary: vec2f,
    w2o: mat4x3f,
};

// Whether a candidate the hardware found is really there: a face is cut
// out by its layer's coverage and everything by its opacity, each as a
// coin toss, so partial cover converges to its fraction.
fn solid(c: RayIntersection, o: vec3f, d: vec3f) -> bool {
    if (c.instance_custom_data == body) { return true; }
    let inst = insts[c.instance_custom_data];
    if (inst.kind.x == KIND_FLOOR) { return true; }
    var a = inst.edge.a;
    if (inst.kind.x == KIND_PLANE && c.geometry_index < 2u) {
        let local = c.world_to_object * vec4f(o + d * c.t, 1.);
        a *= layer(inst.kind.y, face_uv(inst, local)).a;
    }
    return a >= 1. || (a > 0.004 && rand() < a);
}

fn closest(o: vec3f, d: vec3f) -> Hit {
    var rq: ray_query;
    rayQueryInitialize(&rq, acc, RayDesc(0u, 0xFFu, 0., BIG, o, d));
    while (rayQueryProceed(&rq)) {
        let c = rayQueryGetCandidateIntersection(&rq);
        if (solid(c, o, d)) { rayQueryConfirmIntersection(&rq); }
    }
    let h = rayQueryGetCommittedIntersection(&rq);
    var out: Hit;
    out.hit = h.kind != RAY_QUERY_INTERSECTION_NONE;
    out.t = h.t;
    out.inst = h.instance_custom_data;
    out.geom = h.geometry_index;
    out.prim = h.primitive_index;
    out.bary = h.barycentrics;
    out.w2o = h.world_to_object;
    return out;
}

// --- surfaces --------------------------------------------------------------

// 0 front face, 1 back face, 2 wall, 3 mesh, 4 floor.
struct Surf {
    p: vec3f,
    // Outward shading normal, world.
    n: vec3f,
    // Outward geometric normal, world.
    ng: vec3f,
    base: vec3f,
    part: u32,
    inst: u32,
};

// An object normal to the world: the inverse transpose of object-to-world.
fn to_world_n(w2o: mat4x3f, n: vec3f) -> vec3f {
    return normalize(n * mat3x3f(w2o[0], w2o[1], w2o[2]));
}

// A face's bevel: its normal in local space, tipped out toward the nearer
// edges within `bevel` of them, a quarter round (mui-stage's `bevel_tilt`).
// A face's shading normal at `local` (y up, centred, layer units), in
// local space, tilted as mui-stage's `face_tilt` tilts it: out toward the
// nearer edges within the bevel, a quarter round, and by the relief.
fn face_normal(inst: Inst, local: vec3f) -> vec3f {
    // mui-stage's relief runs y down.
    let r = relief(inst, vec2f(local.x, -local.y));
    var slope = vec2f(-r.x, r.y);
    let b = min(inst.mat2.w, 0.5 * min(inst.size.x, inst.size.y));
    if (b > 0.) {
        let t = 1. - clamp((inst.size.xy * 0.5 - abs(local.xy)) / b, vec2f(0.), vec2f(1.));
        slope += sign(local.xy) * t / sqrt(max(1. - t * t, vec2f(0.04)));
    }
    return normalize(vec3f(slope, 1.));
}

fn hash22(p: vec2f) -> vec2f { return vec2f(hash2(p), hash2(p + vec2f(19.19, 7.31))); }
// The slope (rise along x and along y, y down) glass is pressed to at `q`,
// as mui-stage's `relief` has it: reeds up the face, each a cylindrical
// lens across it; hammered dimples, each a bowl round the nearest of
// jittered points; and three crossing ripples that run with time.
fn relief(inst: Inst, q: vec2f) -> vec2f {
    var s = vec2f(0.);
    let k = inst.relief;
    let size = inst.relief_scale;
    if (k.x > 0.) {
        s.x += k.x * (2. * fract(q.x / size.x) - 1.);
    }
    if (k.y > 0.) {
        let p = q / size.y;
        let i = floor(p);
        var near = vec2f(9.);
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let o = i + vec2f(f32(x), f32(y));
                let r = p - (o + 0.15 + 0.7 * hash22(o));
                if (dot(r, r) < dot(near, near)) { near = r; }
            }
        }
        let b = near / 0.75;
        s += k.y * b / max(1., length(b));
    }
    if (k.z > 0.) {
        let p = q / size.z * 6.2831853;
        let t = g.spec.w;
        let k0 = vec2f(1., 0.);
        let k1 = vec2f(-0.5, 0.866) * 1.23;
        let k2 = vec2f(-0.5, -0.866) * 0.81;
        let w = cos(dot(p, k0) + t * 1.3) * k0 + cos(dot(p, k1) - t * 1.1) * k1 / 1.23
            + cos(dot(p, k2) + t * 0.9) * k2 / 0.81;
        s += k.z * w / 3.;
    }
    return s;
}

fn vertex_normal(i: u32) -> vec3f {
    return vec3f(vertices[i * 6u + 3u], vertices[i * 6u + 4u], vertices[i * 6u + 5u]);
}
fn vertex_pos(i: u32) -> vec3f {
    return vec3f(vertices[i * 6u], vertices[i * 6u + 1u], vertices[i * 6u + 2u]);
}

fn surface(h: Hit, o: vec3f, d: vec3f) -> Surf {
    let inst = insts[h.inst];
    var s: Surf;
    s.p = o + d * h.t;
    s.inst = h.inst;
    let local = h.w2o * vec4f(s.p, 1.);
    var n = vec3f(0., 1., 0.);
    var ng = n;
    if (inst.kind.x == KIND_FLOOR) {
        s.part = 4u;
        s.base = g.floor.rgb;
    } else if (inst.kind.x == KIND_PLANE && h.geom < 2u) {
        s.part = h.geom;
        let c = layer(inst.kind.y, face_uv(inst, local));
        if (h.geom == 0u) {
            ng = vec3f(0., 0., 1.);
            n = face_normal(inst, local);
            s.base = c.rgb;
        } else {
            ng = vec3f(0., 0., -1.);
            n = ng;
            s.base = inst.edge.rgb;
        }
    } else {
        // Walls and meshes: the triangle's own normals, interpolated.
        s.part = select(3u, 2u, inst.kind.x == KIND_PLANE);
        let first = inst.kind.z + h.prim * 3u;
        let a = inst.kind.w + indices[first];
        let b = inst.kind.w + indices[first + 1u];
        let c = inst.kind.w + indices[first + 2u];
        let w = vec3f(1. - h.bary.x - h.bary.y, h.bary.x, h.bary.y);
        n = normalize(vertex_normal(a) * w.x + vertex_normal(b) * w.y + vertex_normal(c) * w.z);
        ng = normalize(cross(vertex_pos(b) - vertex_pos(a), vertex_pos(c) - vertex_pos(a)));
        // Wound either way: the geometric normal follows the shading one.
        ng = select(-ng, ng, dot(ng, n) >= 0.);
        s.base = inst.edge.rgb;
    }
    s.n = to_world_n(h.w2o, n);
    s.ng = to_world_n(h.w2o, ng);
    if (s.part == 2u || s.part == 3u) {
        // A wall's or solid's relief is pressed along the world's x and y,
        // as mui-stage presses it.
        let r = relief(inst, vec2f(s.p.x, -s.p.y));
        s.n = normalize(s.n + vec3f(-r.x, r.y, 0.));
    }
    return s;
}

// --- lights ----------------------------------------------------------------

// How much light gets from `p` along `l` for `dist`: zero past anything
// opaque; glass passes what its tint and two Fresnel crossings leave (no
// caustic focusing: a path tracer from the eye cannot find a point light
// through glass).
fn transmittance(p: vec3f, l: vec3f, dist: f32) -> vec3f {
    var rq: ray_query;
    var tr = vec3f(1.);
    rayQueryInitialize(&rq, acc, RayDesc(RAY_FLAG_TERMINATE_ON_FIRST_HIT, 0xFFu, 0., dist, p, l));
    while (rayQueryProceed(&rq)) {
        let c = rayQueryGetCandidateIntersection(&rq);
        let inst = insts[c.instance_custom_data];
        if (inst.kind.x == KIND_FLOOR) { continue; }
        if (!solid(c, p, l)) { continue; }
        let trans = inst.mat.z;
        if (trans > 0.) {
            // Each surface is half the way through: the tint's root, and one
            // crossing's share of the light at normal incidence.
            let f0 = pow((inst.mat.w - 1.) / (inst.mat.w + 1.), 2.);
            tr *= mix(vec3f(1.), sqrt(max(inst.tint.rgb, vec3f(0.))) * (1. - f0) * (1. - inst.mat.x), trans);
            if (max(tr.x, max(tr.y, tr.z)) < 1e-3) { rayQueryConfirmIntersection(&rq); }
        } else {
            rayQueryConfirmIntersection(&rq);
        }
    }
    let h = rayQueryGetCommittedIntersection(&rq);
    return select(tr, vec3f(0.), h.kind != RAY_QUERY_INTERSECTION_NONE);
}

struct LightSample {
    l: vec3f,
    c: vec3f,
    // Toward the light's middle, unjittered.
    mid: vec3f,
};
// Light `i` reaching `p` (direction toward it and radiance), shadowed when
// `receive`; a soft light is sampled over its disc (mui-stage's `area`).
fn light_at(i: u32, p: vec3f, ng: vec3f, receive: bool) -> LightSample {
    var o: LightSample;
    o.c = vec3f(0.);
    o.l = vec3f(0., 1., 0.);
    let li = g.lights[i];
    if (li.pos.w < 0.5) { return o; }
    let dir = li.dir.xyz;
    let side = select(vec3f(1., 0., 0.), vec3f(0., 1., 0.), abs(dir.y) < 0.9);
    let t = normalize(cross(dir, side));
    let u = cross(dir, t);
    let r = sqrt(rand());
    let a = 2. * PI * rand();
    let disc = (t * cos(a) + u * sin(a)) * r * li.extra.z;
    var l = -normalize(dir + disc);
    o.mid = -dir;
    var dist = BIG;
    var att = 1.;
    if (li.pos.w > 1.5) {
        let to = li.pos.xyz + disc - p;
        o.mid = normalize(li.pos.xyz - p);
        dist = length(to);
        l = to / max(dist, 1e-4);
        if (li.extra.x > 0.) {
            let f = clamp(1. - dist / li.extra.x, 0., 1.);
            att = f * f;
        }
        if (li.pos.w > 2.5) {
            att *= smoothstep(li.dir.w, max(li.color.w, li.dir.w + 1e-4), dot(-l, dir));
        }
    }
    if (att <= 0. || dot(ng, l) <= 0.) { return o; }
    var vis = vec3f(1.);
    if (receive && li.extra.y > 0.5) { vis = transmittance(p + ng * 0.05, l, dist - 0.1); }
    o.l = l;
    o.c = li.color.rgb * att * vis;
    return o;
}

struct Shade {
    diffuse: vec3f,
    spec: vec3f,
};
// Every light at `p` on a surface facing `n` from `v`: diffuse irradiance
// and a Blinn-Phong highlight of exponent `shine`, as mui-stage's `shade`.
fn shade(p: vec3f, n: vec3f, v: vec3f, receive: bool, shine: f32) -> Shade {
    var o: Shade;
    o.diffuse = g.ambient.rgb;
    o.spec = vec3f(0.);
    for (var i = 0u; i < 4u; i++) {
        let s = light_at(i, p, n, receive);
        let ndl = max(dot(n, s.l), 0.);
        o.diffuse += s.c * ndl;
        if (shine > 0.) {
            o.spec += s.c * pow(max(dot(n, normalize(s.l + v)), 0.), shine) * ndl;
        }
    }
    return o;
}

fn sky_is_on() -> bool { return g.sky0.w > 0.5; }
// What a ray that meets nothing sees.
fn background(d: vec3f) -> vec3f {
    if (sky_is_on()) { return sky(d); }
    return select(vec3f(0.), g.clear.rgb, g.clear.a > 0.5);
}
// What a reflection leaving the scene brings, as mui-stage's
// `spec_fallback`.
fn spec_fallback(r: vec3f, rough: f32) -> vec3f {
    if (sky_is_on()) { return sky_spec(r, rough); }
    return g.ambient.rgb;
}

// An opaque surface's light toward `v`, as mui-stage's raster passes shade
// it (diffuse direct light, a mirror term, no bounce light), so a surface
// seen through glass matches the same surface seen directly.
fn opaque(s: Surf, v: vec3f) -> vec3f {
    let inst = insts[s.inst];
    let receive = inst.tint.w > 0.5;
    let lit = g.res.w > 0.5;
    let n = select(-s.n, s.n, dot(s.n, v) >= 0.);
    let metal = inst.mat.x;
    let rough = inst.mat.y;
    if (s.part == 0u) {
        if (!lit) { return s.base * inst.size.w; }
        let f0 = mix(vec3f(0.04), s.base, metal);
        let c = s.base * (1. - metal) * shade(s.p, n, v, receive, 0.).diffuse
            + f0 * spec_fallback(reflect(-v, n), rough);
        return c * inst.size.w;
    }
    if (s.part == 1u) {
        if (!lit) { return inst.edge.rgb * 0.3; }
        return inst.edge.rgb * shade(s.p, n, v, receive, 0.).diffuse;
    }
    if (s.part == 2u || s.part == 3u) {
        let shine = select(48., 2. / max(pow(rough, 4.), 1e-3) - 2., s.part == 3u);
        let f0 = select(vec3f(0.04), mix(vec3f(0.04), s.base, metal), s.part == 3u);
        if (!lit) {
            let l = normalize(vec3f(-0.4, 0.6, 0.7));
            let h = normalize(l + v);
            let ndl = max(dot(n, l), 0.);
            if (s.part == 2u) {
                let rim = pow(1. - max(dot(n, v), 0.), 3.) * 0.35;
                return s.base * (0.3 + 0.7 * ndl) + vec3f(pow(max(dot(n, h), 0.), 48.) * 0.8 + rim);
            }
            return s.base * (1. - metal) * (0.3 + 0.7 * ndl)
                + f0 * pow(max(dot(n, h), 0.), shine) * ndl * (shine + 8.) / 25.;
        }
        let sh = shade(s.p, n, v, receive, shine);
        if (s.part == 2u) { return s.base * sh.diffuse + sh.spec * 0.3; }
        return s.base * (1. - metal) * sh.diffuse + f0 * sh.spec * (shine + 8.) / 25.
            + f0 * spec_fallback(reflect(-v, n), rough);
    }
    // The floor.
    var c = g.floor.rgb;
    if (lit) { c *= shade(s.p, vec3f(0., 1., 0.), v, true, 0.).diffuse; }
    return c;
}

// --- the dielectric -------------------------------------------------------

// Unpolarised Fresnel reflectance of a dielectric interface, `cos_i` the
// cosine of incidence and `eta` the ratio n_transmitted / n_incident.
// After pbrt-v4's FrDielectric (Apache-2.0).
fn fresnel(cos_i: f32, eta: f32) -> f32 {
    let c = clamp(cos_i, 0., 1.);
    let sin2_t = (1. - c * c) / (eta * eta);
    if (sin2_t >= 1.) { return 1.; }
    let cos_t = sqrt(max(1. - sin2_t, 0.));
    let r_par = (eta * c - cos_t) / (eta * c + cos_t);
    let r_perp = (c - eta * cos_t) / (c + eta * cos_t);
    return 0.5 * (r_par * r_par + r_perp * r_perp);
}

// The index at wavelength `nm` for glass of index `ior` at 587.6 nm and
// dispersion `k` (20 / Abbe): Cauchy's A + B / l^2 through n_F - n_C =
// (ior - 1) / V, exactly as mui-stage's raster glass has it.
fn ior_at(ior: f32, k: f32, nm: f32) -> f32 {
    let b = (ior - 1.) * k / 20. / (1. / (0.4861 * 0.4861) - 1. / (0.6563 * 0.6563));
    let l = nm * 0.001;
    return ior - b / (0.5876 * 0.5876) + b / (l * l);
}
// The RGB a single wavelength carries: three bands, each scaled so the
// mean over 380..700 nm is white. A colour-matching stand-in, not CIE.
fn band(nm: f32) -> vec3f {
    let x = (vec3f(nm) - vec3f(605., 545., 455.)) / vec3f(42., 38., 32.);
    return exp(-0.5 * x * x) * g.spec.rgb;
}

// GGX: a microfacet normal about `n`, drawn by D(m)|m.n| (Walter eq. 35).
fn ggx_sample(n: vec3f, alpha: f32, u: vec2f) -> vec3f {
    let tan2 = alpha * alpha * u.x / max(1. - u.x, 1e-6);
    let cos_t = 1. / sqrt(1. + tan2);
    let sin_t = sqrt(max(1. - cos_t * cos_t, 0.));
    let phi = 2. * PI * u.y;
    let up = select(vec3f(1., 0., 0.), vec3f(0., 0., 1.), abs(n.z) < 0.999);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return normalize(t * (sin_t * cos(phi)) + b * (sin_t * sin(phi)) + n * cos_t);
}
// Smith's G1 for GGX (Walter eq. 34).
fn ggx_g1(v: vec3f, m: vec3f, n: vec3f, alpha: f32) -> f32 {
    let vn = dot(v, n);
    if (dot(v, m) * vn <= 0.) { return 0.; }
    let c2 = vn * vn;
    let tan2 = max(1. - c2, 0.) / max(c2, 1e-8);
    return 2. / (1. + sqrt(1. + alpha * alpha * tan2));
}
fn ggx_d(m: vec3f, n: vec3f, alpha: f32) -> f32 {
    let c = dot(m, n);
    if (c <= 0.) { return 0.; }
    let a2 = alpha * alpha;
    let k = c * c * (a2 - 1.) + 1.;
    return a2 / (PI * k * k);
}

// --- the path -------------------------------------------------------------

struct Path {
    l: vec3f,
    // Coverage: the eye ray's first surface is glass.
    glass: f32,
    // The first surface's normal and distance.
    n: vec3f,
    t: f32,
};

fn luminance(c: vec3f) -> f32 { return dot(c, vec3f(0.2126, 0.7152, 0.0722)); }

// Light found after the first bounce, its brightness capped: a rare path
// through a rough lobe onto a highlight is a firefly hundreds of samples
// do not average away. Biased (a little dimmer), as every real-time path
// tracer's clamp is.
const TAME: f32 = 8.;
fn tame(c: vec3f, bounce: u32) -> vec3f {
    let m = max(c.x, max(c.y, c.z));
    if (bounce == 0u || m <= TAME) { return c; }
    return c * (TAME / m);
}

fn trace_path(o0: vec3f, d0: vec3f) -> Path {
    var out: Path;
    out.l = vec3f(0.);
    out.glass = 0.;
    out.n = vec3f(0.);
    out.t = BIG;
    var o = o0;
    var d = d0;
    var thr = vec3f(1.);
    // The hero wavelength once a dispersive surface picks one; 0 none.
    var nm = 0.;
    // The glass the path is inside: what it absorbs over how far.
    var inside = false;
    body = NONE;
    var tint = vec3f(1.);
    var reach = 0.;
    // A floor reflection fades with its height above the floor.
    var fade = 0.;
    let bounces = u32(g.res.z);
    for (var bounce = 0u; bounce < bounces; bounce++) {
        let h = closest(o, d);
        if (!h.hit) {
            out.l += tame(thr * background(d) * select(1., 0., fade > 0.), bounce);
            break;
        }
        if (inside && reach > 0.) { thr *= pow(max(tint, vec3f(1e-4)), vec3f(h.t / reach)); }
        if (fade > 0.) { thr *= exp(-h.t * abs(d.y) / fade); fade = 0.; }
        let s = surface(h, o, d);
        let inst = insts[s.inst];
        let trans = inst.mat.z;
        let glass = trans > 0. && s.part != 4u;
        if (bounce == 0u) {
            out.glass = select(0., 1., glass);
            out.n = s.n;
            out.t = h.t;
        }
        let v = -d;
        if (s.part == 4u) {
            // The floor: fades into the background toward its rim, mirrors
            // `reflect` of what stands on it.
            let r = length(s.p.xz) / g.floor2.w;
            if (rand() > exp(-r * r)) {
                o = s.p + d * 0.05;
                continue;
            }
            out.l += tame(thr * opaque(s, v), bounce);
            thr *= g.floor2.y;
            fade = g.floor2.z;
            d = reflect(d, vec3f(0., 1., 0.));
            o = s.p + vec3f(0., 0.05, 0.);
            continue;
        }
        if (!glass) {
            out.l += tame(thr * opaque(s, v), bounce);
            break;
        }
        // Printed glass: the layer's light marks are ink on it.
        let print = inst.mat2.z;
        let ink = print * smoothstep(0.02, 0.25, luminance(s.base));
        let through = smoothstep(0., 0.3, trans) * trans * (1. - ink);
        // The opaque share is shaded here, not drawn by lot: a bright
        // print against dark glass would be noise for hundreds of samples.
        if (through < 1.) {
            out.l += tame(thr * (1. - through) * opaque(s, v), bounce);
            if (through <= 0.) { break; }
            thr *= through;
        }
        thr *= 1. - inst.mat.x;
        let k = inst.mat2.y;
        if (k > 0. && nm == 0.) {
            nm = 380. + 320. * hero;
            thr *= band(nm);
        }
        let ior = select(max(inst.mat.w, 1.), ior_at(max(inst.mat.w, 1.), k, nm), k > 0.);
        let entering = dot(d, s.ng) < 0.;
        let nf = select(-s.n, s.n, entering);
        let ngf = select(-s.ng, s.ng, entering);
        let eta = select(1. / ior, ior, entering);
        let alpha = max(inst.mat.y * inst.mat.y, 0.);
        var m = nf;
        if (alpha > 4e-4) { m = ggx_sample(nf, alpha, rand2()); }
        let cos_i = dot(v, m);
        if (cos_i <= 0.) { break; }
        // The layer's colour stains what passes its face, unless printed;
        // a mesh's base colour stains what passes it, as glTF has it.
        let stain = select(vec3f(1.), mix(s.base, vec3f(1.), print), s.part == 0u || s.part == 3u);
        let card = inst.kind.x == KIND_PLANE && inst.size.z <= 0.;
        var f = fresnel(cos_i, eta);
        if (card) {
            // A pane with no depth: both faces at once, every inter-
            // reflection summed; the light goes on as it came.
            f = f + (1. - f) * (1. - f) * f / max(1. - f * f, 1e-4);
        }
        // Lights glint off the surface: the GGX lobe toward each light's
        // middle, at least as wide as a small lamp (a sharp lobe against
        // a jittered disc is all fireflies), shadowed by the disc sample.
        // Only off the outside: from inside, light reaches the eye by
        // refraction, which these lobes are not.
        if (g.res.w > 0.5 && entering) {
            let a = max(alpha, 0.06);
            for (var i = 0u; i < 4u; i++) {
                let ls = light_at(i, s.p, ngf, inst.tint.w > 0.5);
                if (dot(ls.c, ls.c) <= 0.) { continue; }
                let l = ls.mid;
                let nl = dot(nf, l);
                if (nl <= 0.) { continue; }
                let hv = normalize(l + v);
                let nv = max(dot(nf, v), 1e-3);
                let spec = ggx_d(hv, nf, a) * ggx_g1(v, hv, nf, a) * ggx_g1(l, hv, nf, a)
                    * fresnel(dot(v, hv), eta) / (4. * nv);
                out.l += tame(thr * ls.c * spec, bounce);
            }
        }
        var next: vec3f;
        if (rand() < f) {
            next = reflect(d, m);
            if (dot(next, ngf) <= 0.) { break; }
        } else if (card) {
            next = d;
            thr *= stain * inst.tint.rgb;
        } else {
            next = refract(d, m, 1. / eta);
            if (dot(next, next) < 1e-6 || dot(next, ngf) >= 0.) { break; }
            if (entering) {
                thr *= stain;
                inside = true;
                body = s.inst;
                tint = inst.tint.rgb;
                reach = inst.mat2.x;
                // No length to measure the tint over: it is the crossing's.
                if (reach <= 0.) { thr *= tint; }
            } else {
                inside = false;
                body = NONE;
            }
        }
        if (alpha > 4e-4) {
            // Walter eq. 41: |i.m| G / (|i.n| |m.n|), with the Fresnel and
            // the Jacobian cancelled by sampling.
            let w = abs(dot(v, m)) * ggx_g1(v, m, nf, alpha) * ggx_g1(next, m, nf, alpha)
                / max(abs(dot(v, nf)) * abs(dot(m, nf)), 1e-4);
            thr *= min(w, 4.);
        }
        d = next;
        o = s.p + select(-ngf, ngf, dot(d, ngf) > 0.) * 0.02;
        if (max(thr.x, max(thr.y, thr.z)) < 1e-4) { break; }
    }
    return out;
}

// The eye ray through pixel `px` (y down), offset by `jit` within it.
fn eye_ray(px: vec2f) -> vec3f {
    let uv = px / g.res.xy;
    let q = vec2f(uv.x * 2. - 1., 1. - uv.y * 2.);
    return normalize(g.fwd.xyz + q.x * g.right.w * g.right.xyz + q.y * g.up.w * g.up.xyz);
}

@compute @workgroup_size(8, 8)
fn trace(@builtin(global_invocation_id) id: vec3u) {
    let size = vec2u(g.res.xy);
    if (id.x >= size.x || id.y >= size.y) { return; }
    let i = id.y * size.x + id.x;
    let done = u32(g.eye.w);
    let n = u32(g.fwd.w);
    var sum = vec4f(0.);
    var first: Path;
    for (var k = 0u; k < n; k++) {
        // The pixel's offset and wavelength step through R2 and golden-
        // ratio sequences (shifted per pixel), so every run of samples
        // spreads evenly over the pixel and the spectrum.
        let j = f32(done + k);
        let shift = vec3f(f32(pcg(i)), f32(pcg(i + 77777u)), f32(pcg(i + 999331u))) / 4294967296.;
        let r2 = fract(shift.xy + j * vec2f(0.7548777, 0.5698403));
        hero = fract(shift.z + j * 0.618034);
        rng = pcg(i * 9781u + pcg(done + k) * 6271u);
        let p = trace_path(g.eye.xyz, eye_ray(vec2f(id.xy) + r2));
        sum += vec4f(p.l * p.glass, p.glass);
        if (k == 0u) { first = p; }
    }
    // Keep the mean finite: one wild sample must not blank a pixel.
    sum = select(vec4f(0.), sum, all(sum == sum) && all(abs(sum) < vec4f(1e30)));
    accum[i] = select(accum[i] + sum, sum, done == 0u);
    if (done == 0u) { guide[i] = vec4f(first.n, first.t); }
}

// --- the filter -----------------------------------------------------------

fn luminance(c: vec3f) -> f32 { return dot(c, vec3f(0.2126, 0.7152, 0.0722)); }

struct Filter {
    // x: step in pixels (0: just the mean); y: samples in the sum; zw: size.
    a: vec4f,
};
@group(0) @binding(0) var<uniform> flt: Filter;
@group(0) @binding(1) var<storage, read> f_accum: array<vec4f>;
@group(0) @binding(2) var<storage, read> f_guide: array<vec4f>;
@group(0) @binding(3) var<storage, read> f_src: array<vec4f>;
@group(0) @binding(4) var<storage, read_write> f_dst: array<vec4f>;

// The mean: rgb the glass's own colour (not times coverage), a coverage.
fn mean_at(i: u32) -> vec4f {
    let s = f_accum[i];
    return vec4f(s.rgb / max(s.a, 1e-6), s.a / max(flt.a.y, 1.));
}

@compute @workgroup_size(8, 8)
fn denoise(@builtin(global_invocation_id) id: vec3u) {
    let size = vec2u(flt.a.zw);
    if (id.x >= size.x || id.y >= size.y) { return; }
    let i = id.y * size.x + id.x;
    let step = i32(flt.a.x);
    if (step == 0) {
        f_dst[i] = mean_at(i);
        return;
    }
    let c = f_src[i];
    if (c.a <= 0.) {
        f_dst[i] = c;
        return;
    }
    let gc = f_guide[i];
    let lc = luminance(c.rgb);
    // The colour tolerance narrows as samples come in.
    let sigma_l = 4. / sqrt(max(flt.a.y, 1.));
    let kernel = array<f32, 3>(0.375, 0.25, 0.0625);
    var sum = vec3f(0.);
    var wsum = 0.;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            let q = vec2i(id.xy) + vec2i(x, y) * step;
            if (q.x < 0 || q.y < 0 || q.x >= i32(size.x) || q.y >= i32(size.y)) { continue; }
            let j = u32(q.y) * size.x + u32(q.x);
            let s = f_src[j];
            if (s.a <= 0.) { continue; }
            let gq = f_guide[j];
            let wn = pow(max(dot(gc.xyz, gq.xyz), 0.), 32.);
            let wz = exp(-abs(gc.w - gq.w) / (0.02 * gc.w + 1.));
            let wl = exp(-abs(lc - luminance(s.rgb)) / (sigma_l * (lc + 0.05)));
            let w = kernel[abs(x)] * kernel[abs(y)] * wn * wz * wl;
            sum += s.rgb * w;
            wsum += w;
        }
    }
    f_dst[i] = vec4f(sum / max(wsum, 1e-6), c.a);
}

// --- onto the raster frame ------------------------------------------------

struct Post {
    // exposure, vignette, tonemap on, width.
    a: vec4f,
};
@group(0) @binding(0) var<uniform> post: Post;
@group(0) @binding(1) var<storage, read> c_glass: array<vec4f>;

struct Full {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};
@vertex fn vs_full(@builtin(vertex_index) i: u32) -> Full {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Full;
    o.pos = vec4f(p * 2. - 1., 0., 1.);
    o.uv = vec2f(p.x, 1. - p.y);
    return o;
}
fn aces(x: vec3f) -> vec3f {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3f(0.), vec3f(1.));
}
fn srgb(c: vec3f) -> vec3f {
    return select(1.055 * pow(c, vec3f(1. / 2.4)) - 0.055, c * 12.92, c <= vec3f(0.0031308));
}
// The glass, encoded as mui-stage's `fs_final` encodes the frame (no bloom,
// aberration or grain), premultiplied by its coverage.
@fragment fn fs_composite(i: Full) -> @location(0) vec4f {
    let px = vec2u(i.pos.xy);
    let s = c_glass[px.y * u32(post.a.w) + px.x];
    if (s.a <= 0.) { discard; }
    let dir = i.uv - 0.5;
    var c = s.rgb * post.a.x;
    c *= mix(1., smoothstep(0.85, 0.2, length(dir * vec2f(1., 0.8))), post.a.y);
    let o = clamp(srgb(select(c, aces(c), post.a.z > 0.5)), vec3f(0.), vec3f(1.));
    return vec4f(o * s.a, s.a);
}
