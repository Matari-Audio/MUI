// The stage: MUI layers as textured slabs in a lit 3D scene over a floor that
// reflects them, then depth of field, bloom, aberration, vignette, tonemap
// and grain. All colour here is linear light until `fs_final` encodes it.

// pos: xyz, w kind (0 none, 1 directional, 2 point, 3 spot); dir: the way
// the light travels, w cos(outer cone); color: rgb times intensity,
// w cos(inner cone); extra: range (0 none), shadow layer (-1 none; 8 + n
// light n's cube of point shadows, whose far reach is then dir.w),
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
    // Fog colour, linear; a: on. fog_range: near, far; zw: a beauty
    // sample's two numbers for rays (an R2 sequence).
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
    // The sky (`Shot::sky`): toward the sun, w on; zenith, w cloud cover;
    // horizon, w drift x; sunlight, w drift z.
    sky0: vec4f,
    sky1: vec4f,
    sky2: vec4f,
    sky3: vec4f,
};
// env2.z: 1 in a beauty sample (rays jitter per sample); env2.w: the last
// mip level of the scene colour chain (`aux` in the SSR and glass passes).
fn beauty() -> bool { return g.env2.z > 0.5; }
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
    // x: receives shadows; y metallic, z roughness; w lift: how many
    // overlapping faces in its plane it is drawn in front of.
    flags: vec4f,
    // Glass: transmission, ior, thickness (world units), dispersion (20 / Abbe).
    glass: vec4f,
    // rgb: what is left of white light after `thickness` inside, linear;
    // a: 1 on a slab (light leaves parallel to how it came), 0 on a solid.
    tint: vec4f,
    // Glass: x print (the layer's dark is clear, its light is ink), y the
    // bevel in world units.
    glass2: vec4f,
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

// Colour; the distance from the eye (depth of field, occlusion, SSR) and the
// surface's normal, octahedral; and what screen-space reflection reads: the
// weight the surface's reflection carries (rgb) and its roughness.
struct Out {
    @location(0) color: vec4f,
    @location(1) dist: vec4f,
    @location(2) spec: vec4f,
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
    o.spec = vec4f(0.);
    return o;
}
// `out`, and the normal and reflection weight SSR reads; a mirror image in
// the floor takes no screen-space reflection of its own.
fn out_spec(c: vec4f, world: vec3f, n: vec3f, weight: vec3f, rough: f32) -> Out {
    var o = out(c, world);
    o.dist = vec4f(o.dist.xy, oct_encode(n));
    if (d.mirror.w <= 0.) { o.spec = vec4f(weight * c.a, rough); }
    return o;
}
fn oct_encode(n: vec3f) -> vec2f {
    let p = n.xy / (abs(n.x) + abs(n.y) + abs(n.z));
    if (n.z < 0.) { return (1. - abs(p.yx)) * select(vec2f(-1.), vec2f(1.), p >= vec2f(0.)); }
    return p;
}
fn oct_decode(e: vec2f) -> vec3f {
    var n = vec3f(e, 1. - abs(e.x) - abs(e.y));
    let t = max(-n.z, 0.);
    n.x += select(t, -t, n.x >= 0.);
    n.y += select(t, -t, n.y >= 0.);
    return normalize(n);
}

@group(3) @binding(0) var shadow_map: texture_depth_2d_array;
@group(3) @binding(1) var shadow_cmp: sampler_comparison;
// Point lights' shadows: six faces per light, +x -x +y -y +z -z.
@group(3) @binding(2) var cube_map: texture_depth_2d_array;

// Each face sees a hair past 90 degrees, so the filter stays on it.
const CUBE_SPAN: f32 = 1.03;
// The face of a point light's cube that direction `v` from it falls on.
fn cube_face(v: vec3f) -> u32 {
    let a = abs(v);
    if (a.x >= a.y && a.x >= a.z) { return select(1u, 0u, v.x > 0.); }
    if (a.y >= a.z) { return select(3u, 2u, v.y > 0.); }
    return select(5u, 4u, v.z > 0.);
}
// `world` in clip space as face `f` of light `li`'s cube sees it: across
// and up the face over its axis, depth from 4 units out to `dir.w`.
fn cube_clip(li: Light, f: u32, world: vec3f) -> vec4f {
    let v = world - li.pos.xyz;
    var q: vec3f;
    switch f {
        case 0u: { q = vec3f(-v.z, v.y, v.x); }
        case 1u: { q = vec3f(v.z, v.y, -v.x); }
        case 2u: { q = vec3f(v.x, -v.z, v.y); }
        case 3u: { q = vec3f(v.x, v.z, -v.y); }
        case 4u: { q = vec3f(v.x, v.y, v.z); }
        default: { q = vec3f(-v.x, v.y, -v.z); }
    }
    let near = 4.;
    let far = max(li.dir.w, near + 1.);
    return vec4f(q.xy / CUBE_SPAN, (q.z - near) * far / (far - near), q.z);
}
// Shadow map `k`'s clip space: a light's matrix, or a cube face's.
fn shadow_clip(k: u32, world: vec3f) -> vec4f {
    if (k < 8u) { return g.shadow_vp[k] * vec4f(world, 1.); }
    let c = k - 8u;
    return cube_clip(g.lights[c / 6u], c % 6u, world);
}

fn lit_shot() -> bool { return g.ambient.a > 0.5; }

// How much of light `k` reaches `world` past its shadow map: a 5x5 PCF
// of hardware-filtered compares, `soft` texels apart.
fn shadow(k: i32, world: vec3f, n: vec3f, soft: f32, bias: f32) -> f32 {
    if (k < 0) { return 1.; }
    if (k >= 8) { return point_shadow(k - 8, world + n * 1.5, soft); }
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

// `shadow` for point light `i`: the cube face `world` is seen on, filtered
// the same way.
fn point_shadow(i: i32, world: vec3f, soft: f32) -> f32 {
    let li = g.lights[i];
    let f = cube_face(world - li.pos.xyz);
    let p = cube_clip(li, f, world);
    let q = p.xyz / p.w;
    if (p.w <= 0. || q.z > 1.) { return 1.; }
    let uv = vec2f(q.x * 0.5 + 0.5, 0.5 - q.y * 0.5);
    let texel = soft / f32(textureDimensions(cube_map).x);
    let layer = i * 6 + i32(f);
    var s = 0.;
    for (var y = -2; y <= 2; y++) {
        for (var x = -2; x <= 2; x++) {
            s += textureSampleCompareLevel(cube_map, shadow_cmp,
                uv + vec2f(f32(x), f32(y)) * texel, layer, q.z);
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
// The weight a surface's mirrored light carries: the split sum's F0 scale
// and bias, or with no environment F0 itself against the ambient light.
fn spec_weight(n: vec3f, v: vec3f, rough: f32, f0: vec3f) -> vec3f {
    if (!env_on()) { return f0; }
    let nv = clamp(dot(n, v), 1e-3, 1.);
    let ab = textureSampleLevel(brdf_lut, samp, vec2f(nv, rough), 0.).rg;
    return f0 * ab.x + ab.y;
}
// The light a reflection off screen brings: the environment, else the
// ambient light standing in for it.
fn spec_fallback(r: vec3f, rough: f32) -> vec3f {
    if (env_on()) { return env_radiance(r, rough); }
    if (sky_on()) { return sky(r); }
    return g.ambient.rgb;
}
// Walls and backs: a plain dielectric.
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

// --- the sky: `Shot::sky` ---------------------------------------------------

fn sky_on() -> bool { return g.sky0.w > 0.5; }
// Value noise turned a little each octave, so no axis shows through.
fn cloud_fbm(p: vec2f) -> f32 {
    let r = mat2x2f(0.8, 0.6, -0.6, 0.8);
    var v = 0.;
    var a = 0.5;
    var q = p;
    for (var i = 0; i < 6; i++) {
        v += a * noise(q);
        q = r * q * 2.03 + vec2f(3.1, 1.7);
        a *= 0.5;
    }
    return v;
}
// Cloud at `p` on the layer: domain-warped fbm, billowed, cut at the cover.
fn cloud_density(p: vec2f) -> f32 {
    let w = vec2f(cloud_fbm(p * 0.45 + 4.3), cloud_fbm(p * 0.45 + vec2f(-2.7, 8.1)));
    let n = cloud_fbm(p + 1.8 * w);
    let cut = mix(0.66, 0.3, g.sky1.w);
    return smoothstep(cut, cut + 0.22, n);
}
// Radiance along `d`: the gradient, the sun, and the clouds on a layer
// above (a sea of them below the horizon), lit from the sun.
fn sky(d: vec3f) -> vec3f {
    let sun = g.sky0.xyz;
    let sunc = g.sky3.rgb;
    let up = d.y;
    let h = 1. - clamp(abs(up), 0., 1.);
    let mu = max(dot(d, sun), 0.);
    var c = mix(g.sky1.rgb, g.sky2.rgb, pow(h, 6.));
    // Below the horizon the haze darkens toward the ground.
    c = select(c, mix(g.sky2.rgb, g.sky2.rgb * 0.55 + g.sky1.rgb * 0.15, smoothstep(0., 0.5, -up)), up < 0.);
    c += sunc * (0.06 * pow(mu, 4.) + 0.35 * pow(mu, 64.));
    // The layer: farther toward the horizon, so smaller and hazier there.
    let y = max(abs(up), 0.035);
    let below = up < 0.;
    let drift = vec2f(g.sky2.w, g.sky3.w);
    let p = d.xz / y * select(0.55, 0.4, below) + drift + select(vec2f(0.), vec2f(17.3, -41.9), below);
    let dens = cloud_density(p);
    if (dens > 0.001) {
        // Thinner toward the sun: march a little that way through the layer.
        let toward = normalize(vec2f(sun.x, sun.z) + vec2f(1e-4)) * 0.18;
        let shadow = cloud_density(p + toward) * 0.6 + cloud_density(p + 2. * toward) * 0.4;
        let lit = exp(-2.2 * shadow) * select(1., 1.3, below);
        let amb = g.sky1.rgb * 0.75 + g.sky2.rgb * 0.35;
        // Bright edges against the sun, as droplets scatter it forward.
        let silver = 1. + 2.5 * pow(mu, 10.) * (1. - dens);
        let cloud = amb * (0.55 + 0.45 * dens) + sunc * 0.42 * lit * silver;
        let fade = smoothstep(0.035, 0.16, abs(up));
        c = mix(c, cloud, dens * fade);
    }
    // The disc, over the clouds only where they are thin.
    c += sunc * 30. * smoothstep(0.99985, 0.99995, mu) * (1. - dens);
    return c;
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

// The background at `uv`: the shader, the clear colour or the environment.
fn backdrop(uv: vec2f) -> vec3f { return backdrop_along(eye_ray(uv), uv); }
// The background along `d`, which leaves the frame at `uv`: the
// environment and the sky by direction (a ray bent out of the frame sees
// on round them), the screen's own background at the frame's edge.
fn backdrop_along(d: vec3f, uv: vec2f) -> vec3f {
    if (env_on() && g.env2.x > 0.5) { return env_radiance(d, 0.); }
    if (sky_on()) { return sky(d); }
    let e = clamp(uv, vec2f(0.), vec2f(1.));
    return select(background(e, g.time_res.x), g.clear.rgb, g.clear.a > 0.5);
}
@fragment fn fs_bg(i: Full) -> Out {
    var o: Out;
    o.color = vec4f(backdrop(i.uv), 1.);
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
    // A few ulps of depth per lift win the tie with a coplanar face.
    o.pos.z -= d.flags.w * 5e-7 * o.pos.w;
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
    let n = face_normal(i.world);
    var weight = vec3f(0.);
    let metal = d.flags.y;
    let rough = d.flags.z;
    if (lit_shot()) {
        let v = normalize(g.eye.xyz - i.world);
        let base = c.rgb / max(c.a, 1e-4);
        weight = spec_weight(n, v, rough, mix(vec3f(0.04), base, metal));
        c = vec4f(c.rgb * (1. - metal) * shade(i.world, n, d.flags.x, 0.).diffuse
            + weight * spec_fallback(reflect(-v, n), rough) * c.a, c.a);
    }
    let o = mirrored(vec4f(c.rgb * d.size.w, c.a) * d.edge.a, i.world);
    if (o.a < 0.004) { discard; }
    return out_spec(o, i.world, n, weight * d.size.w, rough);
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
// A model: a wall's attributes and its texture coordinate. Its maps are
// group 2: base colour `tex`, normal `aux`, metallic-roughness `aux2`.
struct MeshV {
    @builtin(position) pos: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
};
@vertex fn vs_mesh(@location(0) p: vec3f, @location(1) n: vec3f, @location(2) uv: vec2f) -> MeshV {
    let world = d.model * vec4f(p, 1.);
    return MeshV(g.view_proj * world, world.xyz, normalize((d.model * vec4f(n, 0.)).xyz), uv);
}
@fragment fn fs_mesh(i: MeshV) -> Out {
    // Derivatives first, in uniform control flow.
    let dp = array<vec3f, 2>(dpdx(i.world), dpdy(i.world));
    let duv = array<vec2f, 2>(dpdx(i.uv), dpdy(i.uv));
    let albedo = textureSample(tex, samp, i.uv);
    let bump = textureSample(aux, samp, i.uv).xyz * 2. - 1.;
    let mr = textureSample(aux2, samp, i.uv);
    var n = normalize(i.normal);
    if (dot(n, g.eye.xyz - i.world) < 0.) { n = -n; }
    if (d.size.x > 0.5) {
        // No tangents in the mesh: the frame from the screen derivatives
        // (Schüler's cotangent frame).
        let a = cross(dp[1], n);
        let b = cross(n, dp[0]);
        let t = a * duv[0].x + b * duv[1].x;
        let bt = a * duv[0].y + b * duv[1].y;
        let k = inverseSqrt(max(max(dot(t, t), dot(bt, bt)), 1e-20));
        let m = normalize(t * k * bump.x + bt * k * bump.y + n * bump.z);
        n = select(n, m, all(m == m));
    }
    let base = d.edge.rgb * albedo.rgb;
    let metal = d.flags.y * mr.b;
    let rough = d.flags.z * mr.g;
    let shine = 2. / max(rough * rough * rough * rough, 1e-3) - 2.;
    let f0 = mix(vec3f(0.04), base, metal);
    var c: vec3f;
    var weight = vec3f(0.);
    if (lit_shot()) {
        let s = shade(i.world, n, d.flags.x, shine);
        let v = normalize(g.eye.xyz - i.world);
        c = base * (1. - metal) * s.diffuse + f0 * s.spec * (shine + 8.) / 25.;
        // With no environment the ambient light stands in for what a metal
        // mirrors.
        weight = spec_weight(n, v, rough, f0);
        c += weight * spec_fallback(reflect(-v, n), rough);
    } else {
        let l = normalize(vec3f(-0.4, 0.6, 0.7));
        let v = normalize(g.eye.xyz - i.world);
        let h = normalize(l + v);
        let ndl = max(dot(n, l), 0.);
        c = base * (1. - metal) * (0.3 + 0.7 * ndl)
            + f0 * pow(max(dot(n, h), 0.), shine) * ndl * (shine + 8.) / 25.;
    }
    let o = mirrored(vec4f(c, 1.) * d.edge.a * albedo.a, i.world);
    if (o.a < 0.004) { discard; }
    return out_spec(o, i.world, n, weight, rough);
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
    o.pos = shadow_clip(k, c.world);
    o.uv = c.uv;
    return o;
}
// Where a caster's cover (a pixel's alpha times its opacity) falls below
// this, it casts nothing. Each beauty sample cuts at its own level, so the
// mean shadow fades with a fading face instead of dropping out at half.
fn shadow_cut() -> f32 { return select(0.5, g.fog_range.z, beauty()); }
// A face casts the shape of its pixels, not of its rectangle.
@fragment fn fs_shadow_face(i: ShadowCap) {
    if (textureSample(tex, samp, i.uv).a * d.edge.a * (1. - d.glass.x) <= shadow_cut()) { discard; }
}
@fragment fn fs_shadow_solid() {
    if (d.edge.a * (1. - d.glass.x) <= shadow_cut()) { discard; }
}
@vertex fn vs_shadow_solid(@location(0) p: vec3f, @location(1) n: vec3f, @builtin(instance_index) k: u32) -> @builtin(position) vec4f {
    return shadow_clip(k, (d.model * vec4f(p, 1.)).xyz);
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

// --- screen-space reflection and refraction -------------------------------

// The opaque frame, mip-chained for rough lookups (`aux` in these passes):
// colour, and in alpha the eye distance to the surface at each pixel.
@fragment fn fs_chain(i: Full) -> @location(0) vec4f {
    let px = vec2i(i.pos.xy);
    return vec4f(textureLoad(tex, px, 0).rgb, textureLoad(aux, px, 0).g);
}

// A world point on the screen: uv (y down) and its view depth.
fn screen(p: vec3f) -> vec3f {
    let c = g.view_proj * vec4f(p, 1.);
    return vec3f(c.x / c.w * 0.5 + 0.5, 0.5 - c.y / c.w * 0.5, c.w);
}
// The opaque surface's eye distance at `uv`.
fn scene_dist(uv: vec2f) -> f32 {
    let size = vec2i(g.time_res.yz);
    return textureLoad(aux, clamp(vec2i(uv * g.time_res.yz), vec2i(0), size - 1), 0).a;
}
// World units across one pixel at view depth `w`.
fn px_size(w: f32) -> f32 {
    return 2. * g.cam_up.w * max(w, 1.) / g.time_res.z;
}
// Two uniform numbers per pixel: the beauty sample's low-discrepancy pair,
// shifted by a per-pixel hash (Cranley-Patterson), so each pixel's samples
// stratify and neighbours decorrelate.
fn rand2(px: vec2f) -> vec2f {
    return fract(vec2f(hash2(px), hash2(px.yx + 17.31)) + g.fog_range.zw);
}
// A GGX microfacet normal about `n` for a view `v` (Walter et al. 2007).
fn ggx_normal(n: vec3f, rough: f32, u: vec2f) -> vec3f {
    let a = max(rough * rough, 1e-3);
    let phi = 6.2831853 * u.x;
    let ct = sqrt((1. - u.y) / (1. + (a * a - 1.) * u.y));
    let st = sqrt(max(1. - ct * ct, 0.));
    let up = select(vec3f(1., 0., 0.), vec3f(0., 1., 0.), abs(n.x) > 0.9);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    return normalize(t * (st * cos(phi)) + b * (st * sin(phi)) + n * ct);
}

const SSR_STEPS: i32 = 48;
const SSR_REACH: f32 = 4000.;
// March from `p` along unit `r` against the opaque depths: how far along
// it first passes behind a surface (within a thickness), bisected to the
// crossing; -1 if it leaves the frame or the reach first.
fn march(p: vec3f, r: vec3f, jitter: f32, reach: f32) -> f32 {
    var prev = 0.;
    var front = true;
    for (var k = 0; k < SSR_STEPS; k++) {
        var s = (f32(k) + jitter) / f32(SSR_STEPS);
        s = s * s;
        let t = reach * s;
        let q = p + r * t;
        let uv = screen(q);
        if (any(uv.xy < vec2f(0.)) || any(uv.xy > vec2f(1.))) { return -1.; }
        let ray = length(q - g.eye.xyz);
        let surf = scene_dist(uv.xy);
        let behind = ray - surf;
        // It went behind, from in front: however far a step overshoots a
        // surface seen edge-on. Behind something already (passing behind
        // a ball), it hits nothing until it is out in front again.
        let past = behind > 0.5 + surf * 0.002;
        if (past && front && surf < 50000.) {
            var lo = prev;
            var hi = t;
            for (var j = 0; j < 6; j++) {
                let m = 0.5 * (lo + hi);
                let qm = p + r * m;
                let um = screen(qm);
                if (length(qm - g.eye.xyz) > scene_dist(um.xy)) { hi = m; } else { lo = m; }
            }
            // A crossing closes on the surface; a ray that only slipped
            // behind a silhouette (from the void beside a card to behind
            // its edge) stays as far behind as it was: march on.
            let qh = p + r * hi;
            let gap = length(qh - g.eye.xyz) - scene_dist(screen(qh).xy);
            if (gap < max(3., (t - prev) * 0.125) + surf * 0.002) { return hi; }
        }
        prev = t;
        front = !past;
    }
    return -1.;
}
// Where a march's steps fall at pixel `px`: in the middle of each, so
// neighbours agree and a reflection's outline is a clean line; a beauty
// sample offsets them per pixel and per sample, which the mean smooths.
fn march_jitter(px: vec2f) -> f32 {
    if (!beauty()) { return 0.5; }
    let ign = fract(52.982918 * fract(dot(px, vec2f(0.06711056, 0.00583715))));
    return fract(ign + rand2(px).x);
}
// The furthest a ray from `p` along `r` is traced: the reach, or short of
// the eye's near side.
fn reach_of(p: vec3f, r: vec3f) -> f32 {
    let w0 = dot(p - g.eye.xyz, g.cam_fwd.xyz);
    let dw = dot(r, g.cam_fwd.xyz);
    var reach = SSR_REACH;
    if (dw < 0.) { reach = min(reach, (w0 - 2.) / -dw); }
    return reach;
}
// `march`, as a reflection sees it: the hit's uv, its confidence (0
// missed; fading toward the frame's edge and the reach) and the pixels
// travelled on screen.
fn trace(p: vec3f, r: vec3f, jitter: f32) -> vec4f {
    let reach = reach_of(p, r);
    if (reach <= 1.) { return vec4f(0.); }
    let t = march(p, r, jitter, reach);
    if (t < 0.) { return vec4f(0.); }
    let a = screen(p);
    let hit = screen(p + r * t).xy;
    let edge = max(abs(hit.x * 2. - 1.), abs(hit.y * 2. - 1.));
    let conf = (1. - smoothstep(0.85, 1., edge)) * (1. - smoothstep(0.7, 1., t / reach));
    return vec4f(hit, conf, length((hit - a.xy) * g.time_res.yz));
}
// The mip that blurs a lookup by a cone `rough` wide over `px` pixels.
fn rough_lod(rough: f32, px: f32) -> f32 {
    let spread = px * rough * rough * 1.2;
    var lod = log2(max(spread, 1.));
    if (beauty()) { lod *= 0.4; }
    return clamp(lod, 0., g.env2.w);
}
// What a surface at `p` with normal `n` mirrors along `r`: the frame where
// the ray meets it, else the environment.
fn reflection(p: vec3f, n: vec3f, r: vec3f, rough: f32, jitter: f32) -> vec3f {
    let fallback = spec_fallback(r, rough);
    let w = dot(p - g.eye.xyz, g.cam_fwd.xyz);
    let h = trace(p + n * (1. + px_size(w)), r, jitter);
    if (h.z <= 0.) { return fallback; }
    let c = textureSampleLevel(aux, samp, h.xy, rough_lod(rough, h.w)).rgb;
    return mix(fallback, c, h.z);
}

// Screen-space reflection over the opaque frame, added to it: each pixel's
// mirrored light (resolved `spec`: its weight and roughness) traced through
// the frame instead of the environment it was lit by. In a beauty sample
// the ray takes a GGX-sampled microfacet, so rough reflections converge.
// tex: the distances and normals; aux: the chain; aux2: `spec`.
@fragment fn fs_ssr(i: Full) -> @location(0) vec4f {
    let px = vec2i(i.pos.xy);
    let s = textureLoad(aux2, px, 0);
    if (max(s.r, max(s.g, s.b)) <= 1e-4) { return vec4f(0.); }
    let dn = textureLoad(tex, px, 0);
    let ray = eye_ray((vec2f(px) + 0.5) / g.time_res.yz);
    let p = g.eye.xyz + ray * dn.y;
    let n = oct_decode(dn.zw);
    let v = -ray;
    let rough = s.a;
    var m = n;
    let u = rand2(vec2f(px));
    if (beauty() && rough > 0.02) { m = ggx_normal(n, rough, u); }
    var r = reflect(ray, m);
    if (dot(r, n) <= 0.) { r = reflect(ray, n); }
    let c = reflection(p, n, r, rough, march_jitter(vec2f(px)));
    return vec4f(s.rgb * (c - spec_fallback(r, rough)), 0.);
}

// Refractive index at wavelength `nm` for glass of index `ior` at 587.6 nm
// and dispersion `k` (20 / Abbe number): Cauchy's A + B / l^2 through
// n_F - n_C = (ior - 1) / V.
fn ior_at(ior: f32, k: f32, nm: f32) -> f32 {
    let b = (ior - 1.) * k / 20. / (1. / (0.4861 * 0.4861) - 1. / (0.6563 * 0.6563));
    let l = nm * 0.001;
    return ior - b / (0.5876 * 0.5876) + b / (l * l);
}

// The opaque frame seen through glass at `p` (normal `n`, microfacet `m`,
// eye ray `ray`) for index `eta`: refract in, cross `thick` inside, leave
// (a slab sends it on parallel to how it came, a solid along the refracted
// ray) and find the opaque surface it meets. xy its uv, z the path inside,
// w the pixels of the blur footprint per unit of roughness; and the way
// it leaves.
struct Through {
    h: vec4f,
    dir: vec3f,
};
fn refracted(p: vec3f, n: vec3f, m: vec3f, ray: vec3f, eta: f32, thick: f32, slab: bool, jitter: f32) -> Through {
    var t = refract(ray, m, 1. / eta);
    if (dot(t, t) < 1e-6) { t = ray; }
    let path = thick / max(abs(dot(t, n)), 0.2);
    let exit = p + t * path;
    // A slab's far face is flat: through it the ray bends back, parallel
    // to how it came when the near face is smooth too.
    var dir = t;
    if (slab) {
        dir = refract(t, n, eta);
        if (dot(dir, dir) < 1e-6) { dir = ray; }
    }
    // March to the opaque surface the ray meets: the depths behind glass
    // jump (a letter, then the void round it), so no guess from the depth
    // straight behind holds. Missing everything, it meets the background
    // where the ray points (w -1), which the frame may hide behind a
    // nearer surface there.
    let along = march(exit, dir, jitter, reach_of(exit, dir));
    if (along < 0.) {
        return Through(vec4f(screen(exit + dir * 1e6).xy, path, -1.), dir);
    }
    let uv = screen(exit + dir * along);
    let behind = min(along, 3000.) / px_size(uv.z);
    return Through(vec4f(clamp(uv.xy, vec2f(0.), vec2f(1.)), path, behind), dir);
}
// What `refracted` found: the frame there, blurred by roughness, or the
// background. ponytail: a missed ray sees the background unblurred; blur
// it with the environment's mips if rough glass over the void bands.
fn seen_through(t: Through, rough: f32) -> vec3f {
    if (t.h.w < 0.) { return backdrop_along(t.dir, t.h.xy); }
    return textureSampleLevel(aux, samp, t.h.xy, rough_lod(rough, t.h.w)).rgb;
}

// A transmissive surface: Fresnel-weighted reflection of the frame (or
// the environment) over the refracted frame, tinted by the base colour
// and absorbed along the path inside, blurred by roughness; dispersion
// sends red, green and blue through their own indices (wavelengths
// jittered within each band per beauty sample). `base` is the surface
// colour; lit, its opaque share (1 - transmission) is diffuse, and every
// light leaves a highlight on the surface as on any polished dielectric.
fn glass(p: vec3f, n: vec3f, tilt: vec3f, base: vec3f, px: vec2f) -> vec3f {
    let trans = d.glass.x;
    let ior = max(d.glass.y, 1.);
    let thick = max(d.glass.z, 0.);
    let k = max(d.glass.w, 0.);
    let slab = d.tint.a > 0.5;
    let rough = d.flags.z;
    let metal = d.flags.y;
    let ray = normalize(p - g.eye.xyz);
    let v = -ray;
    let u = rand2(px);
    // A bevel tilts the face light enters by; it leaves by the flat back.
    let nb = normalize(n + tilt);
    var m = nb;
    if (beauty() && rough > 0.02) { m = ggx_normal(nb, rough, u); }
    let f0 = mix(vec3f(pow((ior - 1.) / (ior + 1.), 2.)), base, metal);
    let fr = f0 + (1. - f0) * pow(1. - clamp(dot(m, v), 0., 1.), 5.);
    var r = reflect(ray, m);
    if (dot(r, n) <= 0.) { r = reflect(ray, n); }
    var body = base;
    var glint = vec3f(0.);
    if (lit_shot()) {
        let shine = 2. / max(rough * rough * rough * rough, 1e-3) - 2.;
        let s = shade(p, n, d.flags.x, shine);
        body = base * s.diffuse;
        glint = f0 * s.spec * (shine + 8.) / 25.;
    }
    // Opaque, drawn after glass it stands in front of: nothing to see through.
    if (trans <= 0.) { return (1. - fr) * body + fr * spec_fallback(r, rough) + glint; }
    let jit = march_jitter(px);
    let refl = reflection(p, n, r, rough, jit);
    // Beauty: one wavelength per band, anywhere in it; else its middle.
    let w = select(vec3f(0.5), fract(vec3f(u.x, u.y, u.x + u.y) + g.jitter.zwz), beauty());
    let nm = vec3f(580., 490., 400.) + w * vec3f(120., 90., 90.);
    var seen = vec3f(0.);
    var path = vec3f(thick);
    if (k > 0.) {
        for (var c = 0; c < 3; c++) {
            let h = refracted(p, n, m, ray, ior_at(ior, k, nm[c]), thick, slab, jit);
            seen[c] = seen_through(h, rough)[c];
            path[c] = h.h.z;
        }
    } else {
        let h = refracted(p, n, m, ray, ior, thick, slab, jit);
        seen = seen_through(h, rough);
        path = vec3f(h.h.z);
    }
    // Beer-Lambert: `tint` is what is left after `thickness`.
    let absorb = select(d.tint.rgb, pow(max(d.tint.rgb, vec3f(1e-4)), path / max(thick, 1e-3)), thick > 0.);
    // Printed: the layer's dark is clear glass, its light marks ink on it.
    let print = d.glass2.x;
    let ink = print * smoothstep(0.02, 0.25, dot(base, vec3f(0.2126, 0.7152, 0.0722)));
    let through = seen * mix(base, vec3f(1.), print) * absorb * (1. - metal);
    let clear = (1. - fr) * mix(body, through, trans * (1. - ink)) + fr * refl + glint;
    // Barely glass is shaded as `fs_front` shades the opaque face, so
    // keying transmission up from 0 crossfades instead of popping.
    var opaque = base;
    if (lit_shot()) {
        let w = spec_weight(n, v, rough, mix(vec3f(0.04), base, metal));
        opaque = base * (1. - metal) * shade(p, n, d.flags.x, 0.).diffuse + w * spec_fallback(ray - 2. * dot(ray, n) * n, rough);
    }
    return mix(opaque, clear, smoothstep(0., 0.3, trans));
}

// The bevel's tilt of a face's normal at `uv`: out toward the nearer
// edges within `glass2.y` of them, a quarter round.
fn bevel_tilt(uv: vec2f) -> vec3f {
    let r = min(d.glass2.y, 0.5 * min(d.size.x, d.size.y));
    if (r <= 0.) { return vec3f(0.); }
    // From the middle, in the layer's y-down units.
    let q = ((uv - d.uv.xy) / (d.uv.zw - d.uv.xy) - 0.5) * d.size.xy;
    let t = 1. - clamp((d.size.xy * 0.5 - abs(q)) / r, vec2f(0.), vec2f(1.));
    let slope = t / sqrt(max(1. - t * t, vec2f(0.04)));
    let ax = normalize((d.model * vec4f(1., 0., 0., 0.)).xyz);
    let ay = normalize((d.model * vec4f(0., 1., 0., 0.)).xyz);
    return ax * sign(q.x) * slope.x - ay * sign(q.y) * slope.y;
}
// A glass face: its layer is the base colour and coverage.
@fragment fn fs_glass(i: Cap) -> Out {
    let c = textureSample(tex, samp, i.uv);
    let a = c.a * d.edge.a;
    if (a < 0.004) { discard; }
    let base = c.rgb / max(c.a, 1e-4);
    let n = face_normal(i.world);
    let rgb = glass(i.world, n, bevel_tilt(i.uv), base, i.pos.xy) * d.size.w;
    return out(vec4f(rgb * a, a), i.world);
}
// A glass wall or model: its colour is the base.
@fragment fn fs_glass_solid(i: Wall) -> Out {
    var n = normalize(i.normal);
    if (dot(n, g.eye.xyz - i.world) < 0.) { n = -n; }
    let rgb = glass(i.world, n, vec3f(0.), d.edge.rgb, i.pos.xy);
    return out(vec4f(rgb, 1.) * d.edge.a, i.world);
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
