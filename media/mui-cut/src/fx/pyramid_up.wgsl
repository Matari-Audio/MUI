// One level back up the pyramid: this level (`aux`) by its weight, plus the
// coarser sum (`src`) spread by a 3x3 tent. Linear half floats throughout.
// `sparse` > 0 (bloom, neon): where a source covers less than a quarter of a
// level's pixel, its light is divided by that shortfall to this power, so it
// spreads as if it filled more of the pixel. A big source is unchanged; a
// small one keeps a halo at any radius instead of averaging away. Never
// brighter than the source's own colour (light <= coverage).
struct Params {
    weight: f32,
    coarser: f32,
    sparse: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let l = textureLoad(aux, vec2<i32>(pos.xy), 0);
    var c = l * u.p.weight;
    if u.p.sparse > 0.0 && l.a > 0.0 {
        c = vec4<f32>(c.rgb * pow(min(l.a * 4.0, 1.0), -u.p.sparse), c.a);
    }
    if u.p.coarser > 0.5 {
        let uv = pos.xy / u.res;
        let ts = 1.0 / vec2<f32>(textureDimensions(src));
        var t = vec4<f32>(0.0);
        for (var y = -1; y <= 1; y++) {
            for (var x = -1; x <= 1; x++) {
                let w = f32((2 - abs(x)) * (2 - abs(y)));
                t += textureSampleLevel(src, smp, uv + vec2<f32>(f32(x), f32(y)) * ts, 0.0) * w;
            }
        }
        c += t / 16.0;
    }
    return c;
}
