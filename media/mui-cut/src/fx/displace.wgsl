struct Params {
    amount: f32,
    scale: f32,
    speed: f32,
    seed: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let q = pos.xy / (u.p.scale * u.scale);
    let z = u.time * u.p.speed + u.p.seed * 13.37;
    let n = vec2<f32>(noise(vec3<f32>(q, z)), noise(vec3<f32>(q + 71.3, z + 29.1)));
    return at(pos.xy + (n * 2.0 - 1.0) * u.p.amount * u.scale);
}
