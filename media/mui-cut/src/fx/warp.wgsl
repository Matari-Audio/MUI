// The scene behind a 3D layer, onto the layer's box in the atlas: `src` is
// the frame rendered without the layer and what is nearer, and `h` maps a
// pixel here to that frame's uv (homogeneous: the layer's plane seen
// through the camera), so a backdrop effect reads what is behind it.
struct Params {
    h0: vec4<f32>,
    h1: vec4<f32>,
    h2: vec4<f32>,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let q = vec3<f32>(pos.xy, 1.0);
    let p = vec3<f32>(dot(u.p.h0.xyz, q), dot(u.p.h1.xyz, q), dot(u.p.h2.xyz, q));
    if p.z <= 0.0 {
        return vec4<f32>(0.0);
    }
    return textureSampleLevel(src, smp, p.xy / p.z, 0.0);
}
