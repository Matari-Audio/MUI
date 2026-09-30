struct Params {
    length: f32,
    // Degrees, clockwise from +x.
    angle: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let len = u.p.length * u.scale;
    let n = clamp(i32(ceil(len)), 1, 256);
    let a = radians(u.p.angle);
    let step = vec2<f32>(cos(a), sin(a)) * len / f32(n);
    var sum = vec4<f32>(0.0);
    for (var i = 0; i <= n; i++) {
        sum += at(pos.xy + step * (f32(i) - 0.5 * f32(n)));
    }
    return sum / f32(n + 1);
}
