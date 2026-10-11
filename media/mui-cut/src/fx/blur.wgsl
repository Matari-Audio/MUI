struct Params {
    radius: f32,
}

// Separable: pass 0 across, pass 1 down. Edges repeat.
@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let at0 = vec2<i32>(pos.xy);
    let sigma = u.p.radius * u.scale;
    if sigma < 0.3 {
        return texel(at0);
    }
    let dir = select(vec2<i32>(0, 1), vec2<i32>(1, 0), u.pass_index == 0u);
    let n = min(i32(ceil(3.0 * sigma)), 192);
    let k = -0.5 / (sigma * sigma);
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var i = -n; i <= n; i++) {
        let w = exp(k * f32(i * i));
        sum += texel(at0 + dir * i) * w;
        total += w;
    }
    return sum / total;
}
