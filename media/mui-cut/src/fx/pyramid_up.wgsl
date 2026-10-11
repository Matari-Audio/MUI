// One level back up the pyramid: this level (`aux`) by its weight, plus the
// coarser sum (`src`) spread by a 3x3 tent. Linear half floats throughout.
struct Params {
    weight: f32,
    coarser: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    var c = textureLoad(aux, vec2<i32>(pos.xy), 0) * u.p.weight;
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
