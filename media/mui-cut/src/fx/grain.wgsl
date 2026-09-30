struct Params {
    amount: f32,
    size: f32,
    seed: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let c = textureLoad(src, vec2<i32>(pos.xy), 0);
    let cell = vec2<u32>(floor(pos.xy / max(u.p.size * u.scale, 1.0)));
    let s = u.seed * 7919u + u32(u.p.seed);
    // Two uniforms summed: a triangle distribution, closer to real grain.
    let n = rand(vec3<u32>(cell, s)) + rand(vec3<u32>(cell, s ^ 0x9e3779b9u)) - 1.0;
    // Strongest in the midtones, scaled by coverage so clear stays clear.
    let l = luma(unpremul(c).rgb);
    let k = u.p.amount * (0.35 + 2.6 * l * (1.0 - l));
    return vec4<f32>(clamp(c.rgb + n * k * c.a, vec3<f32>(0.0), vec3<f32>(c.a)), c.a);
}
