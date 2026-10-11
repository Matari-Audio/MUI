struct Params {
    amount: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    // Radial: nothing at the centre, `amount` project pixels at a corner.
    let d = (pos.xy - 0.5 * u.res) / length(0.5 * u.res) * u.p.amount * u.scale;
    // Red magnified, blue shrunk: red fringes outside edges, blue inside.
    let r = at(pos.xy - d);
    let g = at(pos.xy);
    let b = at(pos.xy + d);
    return vec4<f32>(r.r, g.g, b.b, max(max(r.a, g.a), b.a));
}
