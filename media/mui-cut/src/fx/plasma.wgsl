// A generator in the ISF/Shadertoy manner: it ignores the colours under it
// and draws a field from position and time, keeping the layer's coverage
// (its shape, antialiasing and opacity) as a mask.
struct Params {
    color_a: vec4<f32>,
    color_b: vec4<f32>,
    scale: f32,
    speed: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let mask = textureLoad(src, vec2<i32>(pos.xy), 0).a;
    let p = pos.xy / (u.p.scale * u.scale);
    let t = u.time * u.p.speed;
    var v = sin(p.x + t);
    v += sin(0.7 * p.y - 1.3 * t);
    v += sin(0.5 * (p.x + p.y) + 0.8 * t);
    let c = p + vec2<f32>(3.0 * sin(0.33 * t), 3.0 * cos(0.5 * t));
    v += sin(length(c) + t);
    let k = 0.5 + 0.5 * sin(0.785398 * v);
    let col = mix(u.p.color_a, u.p.color_b, k);
    let a = mask * col.a;
    return vec4<f32>(col.rgb * a, a);
}
