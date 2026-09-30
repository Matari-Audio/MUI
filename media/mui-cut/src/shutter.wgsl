// The GPU shutter: `accumulate` adds one rendered subframe, weighted 1/N, to a
// float target as premultiplied linear light; `resolve` turns the sum back
// into straight-alpha sRGB bytes. The same maths as mui-reel's CPU
// accumulate/resolve, so a GPU and a CPU render blur alike.

@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var<uniform> weight: vec4<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    return select(pow((c + 0.055) / 1.055, vec3<f32>(2.4)), c / 12.92, c <= vec3<f32>(0.04045));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

// The source is what MUI's present pass writes: sRGB-encoded, premultiplied.
@fragment
fn accumulate(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let s = textureLoad(src, vec2<i32>(p.xy), 0);
    var rgb = vec3<f32>(0.0);
    if s.a > 0.0 {
        rgb = to_linear(clamp(s.rgb / s.a, vec3<f32>(0.0), vec3<f32>(1.0))) * s.a;
    }
    return vec4<f32>(rgb, s.a) * weight.x;
}

@fragment
fn resolve(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let s = textureLoad(src, vec2<i32>(p.xy), 0);
    var rgb = vec3<f32>(0.0);
    if s.a > 0.0 {
        rgb = to_srgb(clamp(s.rgb / s.a, vec3<f32>(0.0), vec3<f32>(1.0)));
    }
    return vec4<f32>(rgb, s.a);
}
