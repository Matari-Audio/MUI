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
    } else if m == 3u {
        // Neon: the tube's own colour, hotter where its tight halo piles up
        // on it (the core spills to white); round it, the halo stays its
        // colour, saturated: light past 1 there is kept in hue, not spilled.
        let ga = min(max(max(g.r, g.g), g.b), 1.0);
        var rgb = s.rgb + g.rgb * max(s.a, 0.0);
        let over = max(rgb - vec3<f32>(s.a), vec3<f32>(0.0));
        rgb += vec3<f32>(over.r + over.g + over.b) * 0.5;
        let halo = g.rgb * (1.0 - s.a);
        let peak = max(max(max(halo.r, halo.g), halo.b), 1.0);
        o = vec4<f32>(rgb + halo / peak, s.a + ga * (1.0 - s.a));
        return store(o, pos.xy);
    } else {
        // Bloom adds light: over clear, as little coverage as the light needs.
        let ga = min(max(max(g.r, g.g), g.b), 1.0);
        o = vec4<f32>(s.rgb + g.rgb, s.a + ga * (1.0 - s.a));
    }
    return store(o, pos.xy);
}
