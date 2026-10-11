struct Params {
    tint: vec4<f32>,
    black: f32,
    white: f32,
    gamma: f32,
    saturation: f32,
    tint_amount: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let s = unpremul(textureLoad(src, vec2<i32>(pos.xy), 0));
    var rgb = clamp((s.rgb - u.p.black) / max(u.p.white - u.p.black, 1e-4), vec3<f32>(0.0), vec3<f32>(1.0));
    rgb = pow(rgb, vec3<f32>(1.0 / u.p.gamma));
    let l = luma(rgb);
    rgb = mix(vec3<f32>(l), rgb, u.p.saturation);
    // Tint by luminance: black stays black, white becomes the tint.
    rgb = mix(rgb, l * u.p.tint.rgb, u.p.tint_amount);
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)) * s.a, s.a);
}
