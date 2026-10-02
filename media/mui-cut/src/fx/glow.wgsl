// The glow composite: the source, and the pyramid's blur of what glows
// (`aux`, half size, linear half floats), in linear light. `mode` is the schema's order.
struct Params {
    color: vec4<f32>,
    threshold: f32,
    knee: f32,
    radius: f32,
    intensity: f32,
    tint: f32,
    falloff: f32,
    mode: f32,
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let s = lin(texel(vec2<i32>(pos.xy)));
    let m = u32(u.p.mode);
    // An edge glow's blur is half strength at the edge: twice it is whole.
    let edge = select(1.0, 2.0, m == 1u || m == 2u);
    var g = textureSampleLevel(aux, smp, pos.xy / u.res, 0.0) * u.p.intensity * edge;
    var o: vec4<f32>;
    if m == 1u {
        // Outer: behind the shape.
        g = g / max(g.a, 1.0);
        o = s + g * (1.0 - s.a);
    } else if m == 2u {
        // Inner: over the shape, inside it.
        g = g / max(g.a, 1.0) * s.a;
        o = vec4<f32>(s.rgb * (1.0 - g.a) + g.rgb, s.a);
    } else {
        // Bloom and neon add light; neon's tube burns white at its core.
        var c = s;
        if m == 3u {
            c = vec4<f32>(mix(c.rgb, vec3<f32>(c.a), 0.35 * min(u.p.intensity, 1.0)), c.a);
        }
        o = vec4<f32>(c.rgb + g.rgb, c.a + min(g.a, 1.0) * (1.0 - c.a));
    }
    // Past white, the excess spills into the other channels, so a hot
    // colour goes to white rather than clipping flat.
    let over = max(o.rgb - vec3<f32>(o.a), vec3<f32>(0.0));
    o = vec4<f32>(o.rgb + vec3<f32>(over.r + over.g + over.b) * 0.5, o.a);
    return store(o, pos.xy);
}
