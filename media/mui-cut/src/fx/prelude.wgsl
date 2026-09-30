// Shared by every effect: each effect file is this prelude followed by its
// own `struct Params` (the effect's uniform, fields in the schema's order,
// colours first as vec4) and `fn fs`. Input and output are what MUI's
// present pass writes: sRGB-encoded, premultiplied alpha.

struct U {
    // Output pixels.
    res: vec2<f32>,
    // Seconds into the scene.
    time: f32,
    // Output pixels per project pixel: lengths in Params are project pixels.
    scale: f32,
    // The output frame: per-frame noise, shared by a frame's subframes.
    seed: u32,
    // Which of the effect's passes this is (a separable blur has two).
    pass_index: u32,
    _pad0: u32,
    _pad1: u32,
    p: Params,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
@group(0) @binding(2) var<uniform> u: U;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// The source at a point in output pixels, filtered; outside is transparent.
fn at(p: vec2<f32>) -> vec4<f32> {
    let c = textureSampleLevel(src, smp, p / u.res, 0.0);
    let inside = all(p >= vec2<f32>(0.0)) && all(p <= u.res);
    return select(vec4<f32>(0.0), c, inside);
}

// The source pixel under `p`, edges repeated.
fn texel(p: vec2<i32>) -> vec4<f32> {
    return textureLoad(src, clamp(p, vec2<i32>(0), vec2<i32>(u.res) - 1), 0);
}

fn unpremul(c: vec4<f32>) -> vec4<f32> {
    return select(vec4<f32>(0.0), vec4<f32>(c.rgb / c.a, c.a), c.a > 0.0);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

// PCG-style integer hash: the same bits on every GPU.
fn hash3(v: vec3<u32>) -> vec3<u32> {
    var x = v * 1664525u + 1013904223u;
    x.x += x.y * x.z;
    x.y += x.z * x.x;
    x.z += x.x * x.y;
    x ^= x >> vec3<u32>(16u);
    x.x += x.y * x.z;
    x.y += x.z * x.x;
    x.z += x.x * x.y;
    return x;
}

// Uniform in [0, 1).
fn rand(v: vec3<u32>) -> f32 {
    return f32(hash3(v).x >> 8u) / 16777216.0;
}

// Smooth value noise in [0, 1] over a 3D lattice.
fn noise(p: vec3<f32>) -> f32 {
    let i = floor(p);
    let f = p - i;
    let w = f * f * (3.0 - 2.0 * f);
    let b = vec3<u32>(vec3<i32>(i) + vec3<i32>(1 << 20));
    var n = 0.0;
    for (var k = 0u; k < 8u; k++) {
        let o = vec3<u32>(k & 1u, (k >> 1u) & 1u, (k >> 2u) & 1u);
        let wk = mix(1.0 - w, w, vec3<f32>(o));
        n += rand(b + o) * wk.x * wk.y * wk.z;
    }
    return n;
}
