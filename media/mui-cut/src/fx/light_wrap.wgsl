// Light wrap: the backdrop's blur outside the shape (`aux`, half size, from
// the pyramid) screened over the layer, strongest at its edges.
struct Params {
    radius: f32,
    intensity: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let s = lin(texel(vec2<i32>(pos.xy)));
    if s.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let g = textureSampleLevel(aux, smp, pos.xy / u.res, 0.0);
    // At the very edge half the blur is outside: twice it is the backdrop.
    let w = clamp(g.rgb * 2.0 * u.p.intensity, vec3<f32>(0.0), vec3<f32>(1.0));
    let c = unpremul(s).rgb;
    let lit = 1.0 - (1.0 - c) * (1.0 - w);
    return store(vec4<f32>(lit * s.a, s.a), pos.xy);
}
