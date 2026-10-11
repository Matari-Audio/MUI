// The environment's prefilter on the GPU: `fs_half` builds the source
// pyramid (each texel the mean of four), `fs_convolve` one level of the
// specular chain from it. The same maths as `env::prefilter` on the CPU,
// which stays as the reference the tests hold this to.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var samp: sampler;
// x: the level's roughness, y: the solid angle of a level-0 texel.
@group(0) @binding(2) var<uniform> level: vec4f;

const PI: f32 = 3.14159265;
const SAMPLES: u32 = 64u;

struct Full {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};
@vertex fn vs(@builtin(vertex_index) i: u32) -> Full {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Full;
    o.pos = vec4f(p * 2. - 1., 0., 1.);
    o.uv = vec2f(p.x, 1. - p.y);
    return o;
}

@fragment fn fs_half(i: Full) -> @location(0) vec4f {
    let p = vec2i(i.pos.xy) * 2;
    let hi = vec2i(textureDimensions(src)) - 1;
    var c = vec4f(0.);
    for (var k = 0; k < 4; k++) {
        c += textureLoad(src, min(p + vec2i(k & 1, k >> 1), hi), 0);
    }
    return vec4f(c.rgb * 0.25, 1.);
}

fn dir_of(uv: vec2f) -> vec3f {
    let phi = (uv.x - 0.5) * 2. * PI;
    let el = (0.5 - uv.y) * PI;
    return vec3f(cos(el) * cos(phi), sin(el), cos(el) * sin(phi));
}
fn uv_of(d: vec3f) -> vec2f {
    return vec2f(0.5 + atan2(d.z, d.x) / (2. * PI), 0.5 - asin(clamp(d.y, -1., 1.)) / PI);
}

// GGX-filtered with the level's roughness, reading the pyramid at each
// sample's footprint (filtered importance sampling), N = V.
@fragment fn fs_convolve(i: Full) -> @location(0) vec4f {
    let n = dir_of(i.uv);
    let a = max(level.x * level.x, 1e-4);
    let up = select(vec3f(1., 0., 0.), vec3f(0., 1., 0.), abs(n.y) < 0.999);
    let t = normalize(cross(up, n));
    let b = cross(n, t);
    var sum = vec3f(0.);
    var weight = 0.;
    for (var k = 0u; k < SAMPLES; k++) {
        let e1 = (f32(k) + 0.5) / f32(SAMPLES);
        let e2 = f32(reverseBits(k)) * 2.3283064e-10;
        let phi = 2. * PI * e2;
        let c = sqrt((1. - e1) / (1. + (a * a - 1.) * e1));
        let s = sqrt(max(1. - c * c, 0.));
        let dd = max(c * c * (a * a - 1.) + 1., 1e-7);
        let pdf = a * a / (PI * dd * dd) / 4.;
        let lod = max(0.5 * log2(1. / (f32(SAMPLES) * pdf) / level.y) + 1., 0.);
        let l = vec3f(2. * c * s * cos(phi), 2. * c * s * sin(phi), 2. * c * c - 1.);
        if (l.z <= 0.) { continue; }
        let d = t * l.x + b * l.y + n * l.z;
        sum += textureSampleLevel(src, samp, uv_of(d), lod).rgb * l.z;
        weight += l.z;
    }
    return vec4f(sum / weight, 1.);
}
