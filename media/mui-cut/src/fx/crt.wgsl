struct Params {
    curvature: f32,
    scanlines: f32,
    // Scanline period, project pixels.
    line: f32,
    vignette: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var c = pos.xy / u.res * 2.0 - 1.0;
    // Barrel: points move out with the square of the other axis.
    c += c * (c.yx * c.yx) * u.p.curvature;
    let uv = c * 0.5 + 0.5;
    if any(uv < vec2<f32>(0.0)) || any(uv > vec2<f32>(1.0)) {
        return vec4<f32>(0.0);
    }
    let p = uv * u.res;
    let wave = 0.5 + 0.5 * cos(6.2831853 * p.y / max(u.p.line * u.scale, 1.0));
    let scan = 1.0 - u.p.scanlines * wave;
    let vig = 1.0 - u.p.vignette * 0.5 * dot(c, c);
    let s = at(p);
    return vec4<f32>(s.rgb * clamp(scan * vig, 0.0, 1.0), s.a);
}
