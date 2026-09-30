// The shutter's float sum straight to the encoder's pixels: BT.709
// limited-range 4:2:0, as NV12 (8-bit) or P010 (10 bits in the top of 16),
// so the readback is 1.5 or 3 bytes a pixel and ffmpeg converts nothing.
// Chroma is the 2x2 average of the sRGB-encoded colours. Alpha is dropped,
// as ffmpeg drops it converting rgba.

struct Cfg {
    size: vec2<u32>,
    // Bytes per row of either plane, a multiple of 4.
    stride: u32,
    _pad: u32,
}

@group(0) @binding(0) var acc: texture_2d<f32>;
@group(0) @binding(1) var<storage, read_write> out: array<u32>;
@group(0) @binding(2) var<uniform> cfg: Cfg;

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(c, vec3<f32>(1.0 / 2.4)) - 0.055, c * 12.92, c <= vec3<f32>(0.0031308));
}

// What `resolve` in shutter.wgsl writes, before its rounding to bytes.
fn rgb(x: u32, y: u32) -> vec3<f32> {
    let p = min(vec2<u32>(x, y), cfg.size - 1u);
    let s = textureLoad(acc, vec2<i32>(p), 0);
    if s.a <= 0.0 {
        return vec3<f32>(0.0);
    }
    return to_srgb(clamp(s.rgb / s.a, vec3<f32>(0.0), vec3<f32>(1.0)));
}

fn luma(c: vec3<f32>) -> f32 {
    return (16.0 + 219.0 * dot(c, vec3<f32>(0.2126, 0.7152, 0.0722))) / 255.0;
}

// Cb, Cr of the 2x2 block at (x, y), 0..1 of full scale.
fn chroma(x: u32, y: u32) -> vec2<f32> {
    let c = 0.25 * (rgb(x, y) + rgb(x + 1u, y) + rgb(x, y + 1u) + rgb(x + 1u, y + 1u));
    let l = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    return (vec2<f32>(128.0) + 224.0 * vec2<f32>((c.b - l) / 1.8556, (c.r - l) / 1.5748)) / 255.0;
}

fn q8(v: f32) -> u32 {
    return u32(clamp(v * 255.0 + 0.5, 0.0, 255.0));
}

// 8-bit: four pixels across, two down.
@compute @workgroup_size(8, 8)
fn nv12(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x * 4u;
    let y = id.y * 2u;
    if x >= cfg.size.x || y >= cfg.size.y {
        return;
    }
    for (var r = 0u; r < 2u; r++) {
        var w = 0u;
        for (var i = 0u; i < 4u; i++) {
            w |= q8(luma(rgb(x + i, y + r))) << (8u * i);
        }
        out[((y + r) * cfg.stride + x) / 4u] = w;
    }
    let a = chroma(x, y);
    let b = chroma(x + 2u, y);
    let uv = q8(a.x) | (q8(a.y) << 8u) | (q8(b.x) << 16u) | (q8(b.y) << 24u);
    out[((cfg.size.y + id.y) * cfg.stride + x) / 4u] = uv;
}

fn q10(v: f32) -> u32 {
    return u32(clamp(v * 1023.0 + 0.5, 0.0, 1023.0)) << 6u;
}

// P010: two pixels across, two down; a sample is 16 bits, value in the top 10.
@compute @workgroup_size(8, 8)
fn p010(@builtin(global_invocation_id) id: vec3<u32>) {
    let x = id.x * 2u;
    let y = id.y * 2u;
    if x >= cfg.size.x || y >= cfg.size.y {
        return;
    }
    for (var r = 0u; r < 2u; r++) {
        out[((y + r) * cfg.stride + x * 2u) / 4u] =
            q10(luma(rgb(x, y + r))) | (q10(luma(rgb(x + 1u, y + r))) << 16u);
    }
    let c = chroma(x, y);
    out[((cfg.size.y + id.y) * cfg.stride + x * 2u) / 4u] = q10(c.x) | (q10(c.y) << 16u);
}
