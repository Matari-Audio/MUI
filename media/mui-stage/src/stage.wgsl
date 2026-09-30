// The stage: MUI layers as textured slabs in a lit 3D scene over a floor that
// reflects them, then depth of field, bloom, aberration, vignette, tonemap
// and grain. All colour here is linear light until `fs_final` encodes it.

// pos: xyz, w kind (0 none, 1 directional, 2 point, 3 spot); dir: the way
// the light travels, w cos(outer cone); color: rgb times intensity,
// w cos(inner cone); extra: range (0 none), shadow layer (-1 none),
// softness in texels, depth bias.
struct Light {
    pos: vec4f,
    dir: vec4f,
    color: vec4f,
    extra: vec4f,
};

struct Globals {
    view_proj: mat4x4f,
    eye: vec4f,
    // x: time in seconds, yz: output size in pixels.
    time_res: vec4f,
    // The floor's colour, linear.
    floor: vec4f,
    // A solid background, linear; a: on.
    clear: vec4f,
    // Fog colour, linear; a: on. fog_range: near, far.
    fog: vec4f,
    fog_range: vec4f,
    // Summed ambient light; a: 1 when the shot has lights at all.
    ambient: vec4f,
    lights: array<Light, 4>,
    // One per light, then the floor's top-down contact view.
    shadow_vp: array<mat4x4f, 5>,
    // x: contact shadow strength.
    contact: vec4f,
    // The environment: intensity, cos and sin of its turn about y, on.
    env: vec4f,
    // x: the camera sees it; y: the chain's last mip level.
    env2: vec4f,
    // The view's axes; right.w and up.w: tan of the half field across and
    // up.
    cam_right: vec4f,
    cam_up: vec4f,
    cam_fwd: vec4f,
    // Ambient occlusion: strength, radius in world units, on.
    ao: vec4f,
    // The environment's irradiance, nine SH coefficients (see env.rs).
    sh: array<vec4f, 9>,
    // A beauty sample: xy its shift in clip space (eye rays undo it), zw
    // the turn of the occlusion's slices and steps.
    jitter: vec4f,
};
@group(0) @binding(0) var<uniform> g: Globals;
// The environment's GGX mip chain (equirectangular) and the split-sum LUT.
@group(0) @binding(1) var env_map: texture_2d<f32>;
@group(0) @binding(2) var env_samp: sampler;
@group(0) @binding(3) var brdf_lut: texture_2d<f32>;

struct Draw {
    model: mat4x4f,
    // xy: layer size in world units (logical px), z: slab depth, w: glow.
    size: vec4f,
    // rgb: edge colour (linear), a: opacity.
    edge: vec4f,
    // Reflected draws: x floor height, y strength, z height falloff,
    // w floor radius (0 = not a reflection).
    mirror: vec4f,
    // The layer texture's part on the face: u0, v0, u1, v1.
    uv: vec4f,
    // x: receives shadows; models: y metallic, z roughness.
    flags: vec4f,
};
@group(1) @binding(0) var<uniform> d: Draw;

@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var samp: sampler;

struct Post {
    // threshold, knee, bloom strength, exposure
    bloom: vec4f,
    // aberration, vignette, grain, time
    lens: vec4f,
    // x: tonemap on
    film: vec4f,
    // focus distance, aperture (px of blur at infinity), max blur px
    dof: vec4f,
};
@group(2) @binding(2) var<uniform> post: Post;
// Bloom for `fs_final`, view distance for `fs_dof`, raw Vello pixels for
// `fs_linearize`.
@group(2) @binding(3) var aux: texture_2d<f32>;
// The eye distances, for `fs_ao_apply`.
@group(2) @binding(4) var aux2: texture_2d<f32>;

// Colour, and the distance from the eye for depth of field.
struct Out {
    @location(0) color: vec4f,
    @location(1) dist: vec4f,
};
fn out(c: vec4f, world: vec3f) -> Out {
    var o: Out;
    let dist = length(world - g.eye.xyz);
    o.color = c;
    if (g.fog.a > 0.5) {
        let f = smoothstep(g.fog_range.x, g.fog_range.y, dist);
        o.color = vec4f(mix(c.rgb, g.fog.rgb * c.a, f), c.a);
    }
    // g: the distance to the surface itself, which occlusion reads: a
    // reflection is the floor where the eye ray meets it, not the mirror
    // image's virtual depth below it (a hole to GTAO).
    var surf = dist;
    if (d.mirror.w > 0.) {
        surf = dist * clamp((g.eye.y - d.mirror.x) / max(g.eye.y - world.y, 1e-4), 0., 1.);
    }
    o.dist = vec4f(dist, surf, 0., 1.);
    return o;
}

@group(3) @binding(0) var shadow_map: texture_depth_2d_array;
@group(3) @binding(1) var shadow_cmp: sampler_comparison;

fn lit_shot() -> bool { return g.ambient.a > 0.5; }

// How much of light `k` reaches `world` past its shadow map: a 5x5 PCF
// of hardware-filtered compares, `soft` texels apart.
fn shadow(k: i32, world: vec3f, n: vec3f, soft: f32, bias: f32) -> f32 {
    if (k < 0) { return 1.; }
    // Nudged off the surface along its normal, against shadow acne.
    let p = g.shadow_vp[k] * vec4f(world + n * 1.5, 1.);
    let q = p.xyz / p.w;
    let uv = vec2f(q.x * 0.5 + 0.5, 0.5 - q.y * 0.5);
    if (p.w <= 0. || any(uv < vec2f(0.)) || any(uv > vec2f(1.)) || q.z > 1.) { return 1.; }
    let texel = soft / f32(textureDimensions(shadow_map).x);
    var s = 0.;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            s += textureSampleCompareLevel(shadow_map, shadow_cmp,
                uv + vec2f(f32(x), f32(y)) * texel, k, q.z - bias);
        }
    }
    return s / 25.;
}

// The floor's contact shadow: 1 open, less under what stands close above.
fn contact(world: vec3f) -> f32 {
    if (g.contact.x <= 0.) { return 1.; }
    let p = g.shadow_vp[4] * vec4f(world, 1.);
    let uv = vec2f(p.x * 0.5 + 0.5, 0.5 - p.y * 0.5);
    let size = vec2f(textureDimensions(shadow_map));
    var occ = 0.;
    for (var k = 0; k < 24; k++) {
        let r = sqrt((f32(k) + 0.5) / 24.) * 20.;
        let a = f32(k) * 2.39996;
        let at = clamp(vec2i(uv * size + vec2f(cos(a), sin(a)) * r), vec2i(0), vec2i(size) - 1);
        // Depth 1 is the floor, 0 the top of the contact range: the nearer
        // the floor, the darker.
        let z = textureLoad(shadow_map, at, 4, 0);
        occ += select(0., z * z, z < 0.999);
    }
    return 1. - g.contact.x * occ / 24.;
}

// --- the environment --------------------------------------------------

fn env_on() -> bool { return g.env.w > 0.5; }
// A world direction as the image sees it, turned by the rotation.
fn env_dir(d: vec3f) -> vec3f {
    return vec3f(g.env.y * d.x - g.env.z * d.z, d.y, g.env.z * d.x + g.env.y * d.z);
}
fn env_uv(d: vec3f) -> vec2f {
    return vec2f(0.5 + atan2(d.z, d.x) * 0.15915494, 0.5 - asin(clamp(d.y, -1., 1.)) * 0.31830989);
}
// Light arriving along -d, blurred for `rough`.
fn env_radiance(d: vec3f, rough: f32) -> vec3f {
    let lod = clamp(rough, 0., 1.) * g.env2.y;
    return textureSampleLevel(env_map, env_samp, env_uv(env_dir(d)), lod).rgb * g.env.x;
}
// What a white Lambertian surface facing `n` reflects of the environment.
fn env_irradiance(n: vec3f) -> vec3f {
    let d = env_dir(n);
    let x = d.x;
    let y = d.y;
    let z = d.z;
    let e = g.sh[0].rgb * 0.282095
        + (g.sh[1].rgb * y + g.sh[2].rgb * z + g.sh[3].rgb * x) * 0.488603
        + (g.sh[4].rgb * x * y + g.sh[5].rgb * y * z + g.sh[7].rgb * x * z) * 1.092548
        + g.sh[6].rgb * 0.315392 * (3. * z * z - 1.)
        + g.sh[8].rgb * 0.546274 * (x * x - y * y);
    return max(e, vec3f(0.)) * g.env.x;
}
// Split-sum specular: the prefiltered reflection times F0's scale and bias.
fn env_spec(n: vec3f, v: vec3f, rough: f32, f0: vec3f) -> vec3f {
    if (!env_on()) { return vec3f(0.); }
    let nv = clamp(dot(n, v), 1e-3, 1.);
    let ab = textureSampleLevel(brdf_lut, samp, vec2f(nv, rough), 0.).rg;
    return env_radiance(reflect(-v, n), rough) * (f0 * ab.x + ab.y);
}
// Faces, walls and backs: a plain dielectric.
const SLAB_ROUGH: f32 = 0.42;

struct Shade {
    diffuse: vec3f,
    spec: vec3f,
};
// Every light at `world` on a surface facing `n`, seen from the eye; `shine`
// is a Blinn-Phong exponent (0: no highlight).
fn shade(world: vec3f, n: vec3f, receive: f32, shine: f32) -> Shade {
    var o: Shade;
    o.diffuse = g.ambient.rgb;
    if (env_on()) { o.diffuse += env_irradiance(n); }
    o.spec = vec3f(0.);
    let v = normalize(g.eye.xyz - world);
    for (var i = 0; i < 4; i++) {
        let li = g.lights[i];
        if (li.pos.w < 0.5) { continue; }
        var l = -li.dir.xyz;
        var att = 1.;
        if (li.pos.w > 1.5) {
            let to = li.pos.xyz - world;
            let d = length(to);
            l = to / max(d, 1e-4);
            if (li.extra.x > 0.) {
                let f = clamp(1. - d / li.extra.x, 0., 1.);
                att = f * f;
            }
            if (li.pos.w > 2.5) {
                att *= smoothstep(li.dir.w, max(li.color.w, li.dir.w + 1e-4), dot(-l, li.dir.xyz));
            }
        }
        let ndl = max(dot(n, l), 0.);
        if (ndl * att <= 0.) { continue; }
        var sh = 1.;
        if (receive > 0.5) {
            sh = shadow(i32(li.extra.y), world, n, li.extra.z, li.extra.w);
        }
        let c = li.color.rgb * att * sh;
        o.diffuse += c * ndl;
        if (shine > 0.) {
            let h = normalize(l + v);
            o.spec += c * pow(max(dot(n, h), 0.), shine) * ndl;
        }
    }
    return o;
}

fn hash2(p: vec2f) -> f32 {
    let q = fract(p * vec2f(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}
fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3. - 2. * f);
    return mix(mix(hash2(i), hash2(i + vec2f(1., 0.)), u.x),
               mix(hash2(i + vec2f(0., 1.)), hash2(i + vec2f(1., 1.)), u.x), u.y);
}
fn fbm(p: vec2f) -> f32 {
    var v = 0.;
    var a = 0.5;
    var q = p;
    for (var i = 0; i < 5; i++) {
        v += a * noise(q);
        q = q * 2.03 + vec2f(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}

// --- the background: `Stage::background` splices its function in below ---

{{BACKGROUND}}

// --- full-screen passes ---------------------------------------------------

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

// The eye's ray through `uv` (0..1, y down).
fn eye_ray(uv: vec2f) -> vec3f {
    let q = vec2f(uv.x * 2. - 1., 1. - uv.y * 2.) - g.jitter.xy;
    return normalize(g.cam_fwd.xyz + q.x * g.cam_right.w * g.cam_right.xyz + q.y * g.cam_up.w * g.cam_up.xyz);
}

@fragment fn fs_bg(i: Full) -> Out {
    var o: Out;
    o.color = vec4f(select(background(i.uv, g.time_res.x), g.clear.rgb, g.clear.a > 0.5), 1.);
    if (env_on() && g.env2.x > 0.5) {
        o.color = vec4f(env_radiance(eye_ray(i.uv), 0.), 1.);
    }
    o.dist = vec4f(60000., 60000., 0., 1.);
    return o;
}

// --- slabs ----------------------------------------------------------------

struct Cap {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) world: vec3f,
};
fn cap(i: u32, z: f32) -> Cap {
    var corners = array<vec2f, 6>(
        vec2f(0., 0.), vec2f(1., 0.), vec2f(0., 1.),
        vec2f(0., 1.), vec2f(1., 0.), vec2f(1., 1.));
    let uv = corners[i];
    // Layer space is y-down pixels; the world is y-up, centred on the layer.
    let local = vec4f((uv.x - 0.5) * d.size.x, (0.5 - uv.y) * d.size.y, z, 1.);
    let world = d.model * local;
    var o: Cap;
    o.pos = g.view_proj * world;
    o.uv = mix(d.uv.xy, d.uv.zw, uv);
    o.world = world.xyz;
    return o;
}
// The face's normal, toward whoever sees it.
fn face_normal(world: vec3f) -> vec3f {
    let n = normalize((d.model * vec4f(0., 0., 1., 0.)).xyz);
    return select(-n, n, dot(n, g.eye.xyz - world) > 0.);
}
@vertex fn vs_front(@builtin(vertex_index) i: u32) -> Cap { return cap(i, 0.); }
@vertex fn vs_back(@builtin(vertex_index) i: u32) -> Cap { return cap(i, -d.size.z); }

// Seen in the floor, a surface keeps its coverage, so a nearer reflection
// hides a farther one, but its light fades into the floor's colour with its
// depth below the floor; the whole reflection thins out toward the floor's
// rim. Anything the mirror put above the floor is not seen in it.
fn mirrored(c: vec4f, world: vec3f) -> vec4f {
    if (d.mirror.w <= 0.) { return c; }
    let below = d.mirror.x - world.y;
    if (below < -0.5) { return vec4f(0.); }
    let r = length(world.xz) / d.mirror.w;
    let f = d.mirror.y * exp(-max(below, 0.) / d.mirror.z);
    return vec4f(mix(g.floor.rgb * c.a, c.rgb, f), c.a) * exp(-r * r);
}

@fragment fn fs_front(i: Cap) -> Out {
    var c = textureSample(tex, samp, i.uv);
    if (lit_shot()) {
        let n = face_normal(i.world);
        let v = normalize(g.eye.xyz - i.world);
        c = vec4f(c.rgb * shade(i.world, n, d.flags.x, 0.).diffuse
            + env_spec(n, v, SLAB_ROUGH, vec3f(0.04)) * c.a, c.a);
    }
    let o = mirrored(vec4f(c.rgb * d.size.w, c.a) * d.edge.a, i.world);
    if (o.a < 0.004) { discard; }
    return out(o, i.world);
}
@fragment fn fs_back(i: Cap) -> Out {
    let a = textureSample(tex, samp, i.uv).a;
    var rgb = d.edge.rgb * 0.3;
    if (lit_shot()) {
        let n = face_normal(i.world);
        rgb = d.edge.rgb * shade(i.world, n, d.flags.x, 0.).diffuse
            + env_spec(n, normalize(g.eye.xyz - i.world), SLAB_ROUGH, vec3f(0.04));
    }
    let o = mirrored(vec4f(rgb, 1.) * a * d.edge.a, i.world);
    if (o.a < 0.004) { discard; }
    return out(o, i.world);
}

struct Wall {
    @builtin(position) pos: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
};
@vertex fn vs_wall(@location(0) p: vec3f, @location(1) n: vec3f) -> Wall {
    let world = d.model * vec4f(p, 1.);
    var o: Wall;
    o.pos = g.view_proj * world;
    o.world = world.xyz;
    o.normal = normalize((d.model * vec4f(n, 0.)).xyz);
    return o;
}
@fragment fn fs_wall(i: Wall) -> Out {
    let n = normalize(i.normal);
    if (lit_shot()) {
        let s = shade(i.world, n, d.flags.x, 48.);
        let v = normalize(g.eye.xyz - i.world);
        let c = d.edge.rgb * s.diffuse + s.spec * 0.3 + env_spec(n, v, SLAB_ROUGH, vec3f(0.04));
        let o = mirrored(vec4f(c, 1.) * d.edge.a, i.world);
        if (o.a < 0.004) { discard; }
        return out(o, i.world);
    }
    let l = normalize(vec3f(-0.4, 0.6, 0.7));
    let v = normalize(g.eye.xyz - i.world);
    let h = normalize(l + v);
    let diffuse = 0.3 + 0.7 * max(dot(n, l), 0.);
    let spec = pow(max(dot(n, h), 0.), 48.) * 0.8;
    let rim = pow(1. - max(dot(n, v), 0.), 3.) * 0.35;
    let o = mirrored(vec4f(d.edge.rgb * diffuse + vec3f(spec + rim), 1.) * d.edge.a, i.world);
    if (o.a < 0.004) { discard; }
    return out(o, i.world);
}

// The floor: `size.x` radius, `edge.rgb` colour, `mirror.x` height. It fades
// out radially so it melts into the background instead of ending.
@vertex fn vs_floor(@builtin(vertex_index) i: u32) -> Cap {
    var corners = array<vec2f, 6>(
        vec2f(-1., -1.), vec2f(1., -1.), vec2f(-1., 1.),
        vec2f(-1., 1.), vec2f(1., -1.), vec2f(1., 1.));
    let c = corners[i] * d.size.x * 3.;
    let world = vec3f(c.x, d.mirror.x, c.y);
    var o: Cap;
    o.pos = g.view_proj * vec4f(world, 1.);
    o.uv = c;
    o.world = world;
    return o;
}
@fragment fn fs_floor(i: Cap) -> Out {
    let r = length(i.world.xz) / d.size.x;
    let a = exp(-r * r) * d.edge.a;
    var c = d.edge.rgb * contact(i.world);
    if (lit_shot()) {
        c *= shade(i.world, vec3f(0., 1., 0.), 1., 0.).diffuse;
    }
    return out(vec4f(c * a, a), i.world);
}

// A model: base colour, metallic and roughness, lit by the shot's lights
// or, in an unlit shot, by the walls' fixed key light.
@fragment fn fs_mesh(i: Wall) -> Out {
    var n = normalize(i.normal);
    if (dot(n, g.eye.xyz - i.world) < 0.) { n = -n; }
    let base = d.edge.rgb;
    let metal = d.flags.y;
    let rough = d.flags.z;
    let shine = 2. / max(rough * rough * rough * rough, 1e-3) - 2.;
    let f0 = mix(vec3f(0.04), base, metal);
    var c: vec3f;
    if (lit_shot()) {
        let s = shade(i.world, n, d.flags.x, shine);
        c = base * (1. - metal) * s.diffuse + f0 * s.spec * (shine + 8.) / 25.;
        if (env_on()) {
            c += env_spec(n, normalize(g.eye.xyz - i.world), rough, f0);
        } else {
            // The ambient light stands in for the environment a metal mirrors.
            c += f0 * g.ambient.rgb;
        }
    } else {
        let l = normalize(vec3f(-0.4, 0.6, 0.7));
        let v = normalize(g.eye.xyz - i.world);
        let h = normalize(l + v);
        let ndl = max(dot(n, l), 0.);
        c = base * (1. - metal) * (0.3 + 0.7 * ndl)
            + f0 * pow(max(dot(n, h), 0.), shine) * ndl * (shine + 8.) / 25.;
    }
    let o = mirrored(vec4f(c, 1.) * d.edge.a, i.world);
    if (o.a < 0.004) { discard; }
    return out(o, i.world);
}

// --- shadow maps ------------------------------------------------------------

// The instance index is the shadow map layer, and so the light's matrix.
struct ShadowCap {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};
@vertex fn vs_shadow_face(@builtin(vertex_index) i: u32, @builtin(instance_index) k: u32) -> ShadowCap {
    let c = cap(i, 0.);
    var o: ShadowCap;
    o.pos = g.shadow_vp[k] * vec4f(c.world, 1.);
    o.uv = c.uv;
    return o;
}
// A face casts the shape of its pixels, not of its rectangle.
@fragment fn fs_shadow_face(i: ShadowCap) {
    if (textureSample(tex, samp, i.uv).a * d.edge.a < 0.5) { discard; }
}
@vertex fn vs_shadow_solid(@location(0) p: vec3f, @location(1) n: vec3f, @builtin(instance_index) k: u32) -> @builtin(position) vec4f {
    return g.shadow_vp[k] * d.model * vec4f(p, 1.);
}

// --- layers ---------------------------------------------------------------

// Vello's premultiplied sRGB bytes to premultiplied linear light, so an
// antialiased edge composites at its true brightness and mips average light.
@fragment fn fs_linearize(i: Full) -> @location(0) vec4f {
    let c = textureLoad(aux, vec2i(i.pos.xy), 0);
    if (c.a <= 0.) { return vec4f(0.); }
    let s = clamp(c.rgb / c.a, vec3f(0.), vec3f(1.));
    let lin = select(pow((s + 0.055) / 1.055, vec3f(2.4)), s / 12.92, s <= vec3f(0.04045));
    return vec4f(lin * c.a, c.a);
}

// --- post -----------------------------------------------------------------

@fragment fn fs_copy(i: Full) -> @location(0) vec4f {
    return textureSampleLevel(tex, samp, i.uv, 0.);
}

fn coc(dist: f32) -> f32 {
    return clamp(post.dof.y * abs(dist - post.dof.x) / max(dist, 1.), 0., post.dof.z);
}
// Gather depth of field: 64 taps on a golden-angle spiral. A tap counts where
// its own circle of confusion reaches this pixel; one behind a sharper pixel
// is held to this pixel's circle, so a blurred background does not bleed over
// a sharp foreground edge.
@fragment fn fs_dof(i: Full) -> @location(0) vec4f {
    let px = 1. / g.time_res.yz;
    let d0 = textureSampleLevel(aux, samp, i.uv, 0.).r;
    let c0 = coc(d0);
    var sum = textureSampleLevel(tex, samp, i.uv, 0.).rgb;
    var w = 1.;
    for (var k = 1; k < 64; k++) {
        let r = sqrt(f32(k) / 64.) * post.dof.z;
        let a = f32(k) * 2.39996;
        let uv = i.uv + vec2f(cos(a), sin(a)) * r * px;
        let dk = textureSampleLevel(aux, samp, uv, 0.).r;
        let ck = select(coc(dk), min(coc(dk), c0), dk > d0);
        let wk = smoothstep(r - 1., r + 1., ck);
        sum += textureSampleLevel(tex, samp, uv, 0.).rgb * wk;
        w += wk;
    }
    return vec4f(sum / w, 1.);
}

@fragment fn fs_prefilter(i: Full) -> @location(0) vec4f {
    let c = textureSampleLevel(tex, samp, i.uv, 0.).rgb;
    let br = max(c.r, max(c.g, c.b));
    let knee = post.bloom.y;
    var soft = clamp(br - post.bloom.x + knee, 0., 2. * knee);
    soft = soft * soft / (4. * knee + 1e-5);
    let w = max(soft, br - post.bloom.x) / max(br, 1e-5);
    return vec4f(c * w, 1.);
}

@fragment fn fs_down(i: Full) -> @location(0) vec4f {
    let t = 1. / vec2f(textureDimensions(tex));
    var c = textureSampleLevel(tex, samp, i.uv, 0.) * 0.5;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, -t.y), 0.) * 0.125;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, -t.y), 0.) * 0.125;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, t.y), 0.) * 0.125;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, t.y), 0.) * 0.125;
    return vec4f(c.rgb, 1.);
}

@fragment fn fs_up(i: Full) -> @location(0) vec4f {
    let t = 1. / vec2f(textureDimensions(tex));
    var c = textureSampleLevel(tex, samp, i.uv, 0.) * 4.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, 0.), 0.) * 2.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, 0.), 0.) * 2.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(0., -t.y), 0.) * 2.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(0., t.y), 0.) * 2.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, -t.y), 0.);
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, -t.y), 0.);
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, t.y), 0.);
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, t.y), 0.);
    return vec4f(c.rgb / 16., 1.);
}

// --- ambient occlusion ----------------------------------------------------

// Ground-truth ambient occlusion (Jimenez et al. 2016, after XeGTAO) at half
// resolution: per pixel, three slices through the view vector, each marched
// both ways in screen space for the highest horizon within the radius, and
// the cosine-weighted visible arc between them integrated in closed form.
const AO_SLICES: i32 = 3;
const AO_STEPS: i32 = 6;
const HALF_PI: f32 = 1.5707964;

// The world point at full-resolution pixel `p`, and its eye distance in w.
fn ao_point(p: vec2i) -> vec4f {
    let size = vec2i(g.time_res.yz);
    let q = clamp(p, vec2i(0), size - 1);
    let dist = textureLoad(tex, q, 0).g;
    let uv = (vec2f(q) + 0.5) / g.time_res.yz;
    return vec4f(g.eye.xyz + eye_ray(uv) * dist, dist);
}
fn inside(p: vec2i) -> bool {
    return all(p >= vec2i(0)) && all(p < vec2i(g.time_res.yz));
}
// The smaller of two differences: across an edge the other side is not the
// surface. At the frame's edge one side is the pixel itself.
fn nearer(c: vec3f, a: vec3f, b: vec3f) -> vec3f {
    let da = c - a;
    let db = b - c;
    let la = dot(da, da);
    let lb = dot(db, db);
    return select(db, da, lb < 1e-8 || (la > 1e-8 && la < lb));
}
@fragment fn fs_gtao(i: Full) -> @location(0) vec4f {
    let half = vec2i(i.pos.xy);
    let p = half * 2;
    let c4 = ao_point(p);
    if (c4.w > 50000.) { return vec4f(1., c4.w, 0., 1.); }
    let c = c4.xyz;
    let dx = nearer(c, ao_point(p - vec2i(2, 0)).xyz, ao_point(p + vec2i(2, 0)).xyz);
    let dy = nearer(c, ao_point(p - vec2i(0, 2)).xyz, ao_point(p + vec2i(0, 2)).xyz);
    let v = normalize(g.eye.xyz - c);
    var n = normalize(cross(dx, dy));
    if (dot(n, v) < 0.) { n = -n; }
    let radius = g.ao.y;
    let view_z = max(dot(c - g.eye.xyz, g.cam_fwd.xyz), 1.);
    let reach = clamp(radius * g.time_res.z * 0.5 / (g.cam_up.w * view_z), 4., g.time_res.z * 0.25);
    // Interleaved gradient noise turns the slices and staggers the steps.
    // A beauty sample turns both further, so the mean of many converges.
    let ign = fract(52.982918 * fract(dot(vec2f(half), vec2f(0.06711056, 0.00583715))));
    let noise = fract(ign + g.jitter.z);
    let jitter = fract(ign * 7.13 + 0.37 + g.jitter.w);
    let fall_range = 0.615 * radius;
    let fall_mul = -1. / fall_range;
    let fall_add = (radius - fall_range) / fall_range + 1.;
    var vis = 0.;
    for (var k = 0; k < AO_SLICES; k++) {
        let phi = (f32(k) + noise) * 3.14159265 / f32(AO_SLICES);
        let omega = vec2f(cos(phi), -sin(phi));
        let dir = normalize(g.cam_right.xyz * cos(phi) + g.cam_up.xyz * sin(phi));
        let ortho = dir - v * dot(dir, v);
        let axis = normalize(cross(ortho, v));
        let pn = n - axis * dot(n, axis);
        let pn_len = length(pn);
        let cos_n = clamp(dot(pn, v) / max(pn_len, 1e-5), 0., 1.);
        let nang = sign(dot(ortho, pn)) * acos(cos_n);
        let low0 = cos(nang + HALF_PI);
        let low1 = cos(nang - HALF_PI);
        var h0 = low0;
        var h1 = low1;
        for (var j = 0; j < AO_STEPS; j++) {
            var t = (f32(j) + jitter) / f32(AO_STEPS);
            t = t * t;
            let off = vec2i(round(omega * max(t * reach, 2. + f32(j))));
            let q0 = p + off;
            let q1 = p - off;
            let s0 = ao_point(q0).xyz - c;
            let s1 = ao_point(q1).xyz - c;
            let l0 = length(s0);
            let l1 = length(s1);
            // Past the frame's edge there is nothing to see: no horizon.
            let w0 = select(0., clamp(l0 * fall_mul + fall_add, 0., 1.), inside(q0) && l0 > 1e-3);
            let w1 = select(0., clamp(l1 * fall_mul + fall_add, 0., 1.), inside(q1) && l1 > 1e-3);
            h0 = max(h0, mix(low0, dot(s0 / max(l0, 1e-4), v), w0));
            h1 = max(h1, mix(low1, dot(s1 / max(l1, 1e-4), v), w1));
        }
        var a0 = -acos(clamp(h1, -1., 1.));
        var a1 = acos(clamp(h0, -1., 1.));
        a0 = nang + clamp(a0 - nang, -HALF_PI, HALF_PI);
        a1 = nang + clamp(a1 - nang, -HALF_PI, HALF_PI);
        let arc0 = (cos_n + 2. * a0 * sin(nang) - cos(2. * a0 - nang)) * 0.25;
        let arc1 = (cos_n + 2. * a1 * sin(nang) - cos(2. * a1 - nang)) * 0.25;
        vis += pn_len * (arc0 + arc1);
    }
    return vec4f(clamp(vis / f32(AO_SLICES), 0., 1.), c4.w, 0., 1.);
}
// The half-resolution occlusion upsampled by a 3x3 gather weighted by how
// close each sample's eye distance is to this pixel's, which also smooths
// its noise, then laid on the frame.
@fragment fn fs_ao_apply(i: Full) -> @location(0) vec4f {
    let px = vec2i(i.pos.xy);
    let c = textureLoad(tex, px, 0);
    let d0 = textureLoad(aux2, px, 0).g;
    if (d0 > 50000.) { return c; }
    let hs = vec2i(textureDimensions(aux)) - 1;
    let base = px / 2;
    var sum = 0.;
    var w = 0.;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let s = textureLoad(aux, clamp(base + vec2i(x, y), vec2i(0), hs), 0);
            let wk = exp(-abs(s.g - d0) / (0.02 * d0)) * select(0.5, 1., x == 0 && y == 0) + 1e-4;
            sum += s.r * wk;
            w += wk;
        }
    }
    let ao = pow(clamp(sum / w, 0., 1.), g.ao.x);
    return vec4f(c.rgb * ao, c.a);
}

fn aces(x: vec3f) -> vec3f {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3f(0.), vec3f(1.));
}
fn srgb(c: vec3f) -> vec3f {
    return select(1.055 * pow(c, vec3f(1. / 2.4)) - 0.055, c * 12.92, c <= vec3f(0.0031308));
}

@fragment fn fs_final(i: Full) -> @location(0) vec4f {
    let dir = i.uv - 0.5;
    let ca = post.lens.x * dir;
    var c = vec3f(
        textureSampleLevel(tex, samp, i.uv - ca, 0.).r,
        textureSampleLevel(tex, samp, i.uv, 0.).g,
        textureSampleLevel(tex, samp, i.uv + ca, 0.).b);
    c += textureSampleLevel(aux, samp, i.uv, 0.).rgb * post.bloom.z;
    c *= post.bloom.w;
    c *= mix(1., smoothstep(0.85, 0.2, length(dir * vec2f(1., 0.8))), post.lens.y);
    var o = srgb(select(c, aces(c), post.film.x > 0.5));
    let px = i.uv * g.time_res.yz;
    o += (hash2(px + fract(post.lens.w * 7.31) * 1000.) - 0.5) * post.lens.z;
    return vec4f(clamp(o, vec3f(0.), vec3f(1.)), 1.);
}
