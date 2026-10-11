// A straight copy; as `over` it is drawn with premultiplied blending.
struct Params {
    unused: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(src, vec2<i32>(pos.xy), 0);
}
