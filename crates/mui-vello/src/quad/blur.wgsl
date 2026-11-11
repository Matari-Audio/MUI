// A backdrop's blur: its snapshot averaged down by a whole factor (Rune's
// area prefilter, `layer_blur.wgsl`, at an integer ratio), then a separable
// Gaussian across the small target, each pass one fragment a texel. Taps
// past the snapshot repeat its edge, as `EdgeMode::Duplicate` does.

struct Blur {
    // Texels of the source the snapshot fills.
    used: vec2<f32>,
    // Down: (factor, factor). Blur: the axis, (1, 0) or (0, 1).
    step: vec2<f32>,
    sigma: f32,
    radius: f32,
    pad: vec2<f32>,
}

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> u: Blur;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One triangle over the whole viewport.
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn load(p: vec2<i32>) -> vec4<f32> {
    return textureLoad(src, clamp(p, vec2<i32>(0), vec2<i32>(u.used) - 1), 0);
}

@fragment
fn fs_down(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let n = i32(u.step.x);
    let base = vec2<i32>(floor(p.xy)) * n;
    var sum = vec4<f32>(0.0);
    for (var y = 0; y < n; y++) {
        for (var x = 0; x < n; x++) {
            sum += load(base + vec2<i32>(x, y));
        }
    }
    return sum / f32(n * n);
}

@fragment
fn fs_blur(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let at = vec2<i32>(floor(p.xy));
    let dir = vec2<i32>(u.step);
    let r = i32(u.radius);
    let k = -0.5 / (u.sigma * u.sigma);
    var sum = vec4<f32>(0.0);
    var weight = 0.0;
    for (var i = -r; i <= r; i++) {
        let w = exp(k * f32(i * i));
        sum += load(at + dir * i) * w;
        weight += w;
    }
    return sum / weight;
}
