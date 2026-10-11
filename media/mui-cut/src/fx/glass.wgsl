// Backdrop glass. `src` is the layer (its coverage is the pane), `aux` what
// is composited under it, `aux2` that frosted (the pyramid, half size),
// `aux3` the shape's coverage blurred over the bevel (half size): its
// gradient is the bevel's slope, pointing out of the shape.
struct Params {
    tint: vec4<f32>,
    frost: f32,
    refraction: f32,
    bevel: f32,
    dispersion: f32,
    tint_amount: f32,
    saturation: f32,
    highlight: f32,
    light_angle: f32,
    shadow: f32,
    grain: f32,
}

fn edge(p: vec2<f32>) -> f32 {
    return textureSampleLevel(aux3, smp, p / u.res, 0.0).a;
}

// The backdrop at `p`, linear and premultiplied, frosted by `f`.
fn behind(p: vec2<f32>, f: f32) -> vec4<f32> {
    let uv = p / u.res;
    let sharp = lin(textureSampleLevel(aux, smp, uv, 0.0));
    let frosted = textureSampleLevel(aux2, smp, uv, 0.0);
    return mix(sharp, frosted, f);
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let p = pos.xy;
    let cover = texel(vec2<i32>(p)).a;
    if cover <= 0.0 {
        return vec4<f32>(0.0);
    }
    // The bevel: 0 deep inside the pane, 1 at its edge, and which way is out.
    let d = max(u.p.bevel * u.scale * 0.25, 1.0);
    let grad = vec2<f32>(edge(p + vec2(d, 0.0)) - edge(p - vec2(d, 0.0)), edge(p + vec2(0.0, d)) - edge(p - vec2(0.0, d)));
    let out = select(vec2<f32>(0.0), -normalize(grad), length(grad) > 1e-5);
    let slope = 1.0 - smoothstep(0.0, 1.0, clamp((edge(p) - 0.5) * 2.0, 0.0, 1.0));

    // Refraction: the edge, tilted, shows what lies beyond it, drawn in.
    // By the field's own gradient, so it eases to nothing in the middle of
    // a narrow pane instead of folding over (`refraction` pixels at a
    // straight edge, where the gradient is about 0.4 a bevel).
    let off = -grad / (2.0 * d) * max(u.p.bevel * u.scale, 1.0) * 2.5 * u.p.refraction * u.scale;
    let f = clamp(u.p.frost * u.scale / 3.0, 0.0, 1.0);
    var b = behind(p + off, f);
    if u.p.dispersion > 0.0 {
        let k = 0.5 * u.p.dispersion;
        let r = behind(p + off * (1.0 + k), f);
        let bl = behind(p + off * (1.0 - k), f);
        b = vec4<f32>(r.r, b.g, bl.b, max(max(r.a, b.a), bl.a));
    }
    var c = unpremul(b).rgb;
    c = max(mix(vec3<f32>(luma(c)), c, u.p.saturation), vec3<f32>(0.0));
    c = mix(c, to_lin(u.p.tint.rgb), u.p.tint_amount);

    // Lit from `light_angle` (degrees, y down): a rim where the edge faces
    // the light, a fainter one opposite, an inner shadow on the far side.
    let a = radians(u.p.light_angle);
    let l = vec2<f32>(cos(a), sin(a));
    let facing = dot(out, l);
    c *= 1.0 - u.p.shadow * slope * slope * (0.55 + 0.45 * max(-facing, 0.0));
    let spec = pow(max(facing, 0.0), 3.0) + 0.4 * pow(max(-facing, 0.0), 3.0);
    // A hairline just inside the edge (the shape's own coverage a step
    // out), and a broad sheen over the bevel.
    let line = 1.0 - textureSampleLevel(src, smp, (p + out * 1.5 * u.scale) / u.res, 0.0).a;
    let rim = u.p.highlight * (line * (0.25 + 0.75 * spec) + 0.35 * pow(slope, 3.0) * spec);
    c += vec3<f32>(rim);

    let alpha = cover * clamp(max(b.a, max(u.p.tint_amount, rim)), 0.0, 1.0);
    var o = store(vec4<f32>(c * alpha, alpha), p);
    let n = dither(p, 0x6a5u) * 255.0 * u.p.grain * 0.25;
    o = vec4<f32>(clamp(o.rgb + vec3<f32>(n * o.a), vec3<f32>(0.0), vec3<f32>(o.a)), o.a);
    return o;
}
