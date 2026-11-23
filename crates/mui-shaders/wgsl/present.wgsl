// The present: one triangle that copies the retained frame onto the
// window's view. The frame is premultiplied sRGB, as the quad pass and
// vello_cpu both write it; a surface that composites straight alpha
// (`PostMultiplied`) gets it unpremultiplied.

@group(0) @binding(0) var frame: texture_2d<f32>;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

// The frame's pixel under `p`, or clear past its edge: a view larger than
// the frame (a resize not yet seen) must not show stale or clamped pixels.
fn at(p: vec4<f32>) -> vec4<f32> {
    let xy = vec2<u32>(p.xy);
    if any(xy >= textureDimensions(frame)) {
        return vec4<f32>(0.0);
    }
    return textureLoad(frame, xy, 0);
}

@fragment
fn fs_present(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    return at(p);
}

@fragment
fn fs_present_straight(@builtin(position) p: vec4<f32>) -> @location(0) vec4<f32> {
    let c = at(p);
    if c.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(c.rgb / c.a, c.a);
}
