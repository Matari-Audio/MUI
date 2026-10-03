// One level down the blur pyramid, whose levels are linear light,
// premultiplied, in half floats: half the size, a 13-tap filter (the "Call
// of Duty" downsample: four overlapping boxes round a centre one, no
// aliasing as the picture moves). The first level reads a stored picture
// and says what is blurred (`mode`), per tap:
//   0 a pyramid level (already linear), 1 what is past a soft `threshold`
//   (bloom), 2 the shape in its colour (outer glow), 3 the outside of the
//   shape (inner glow), 4 the shape in its colour, saturated (neon), 5 the
//   backdrop (`aux`) outside the shape (light wrap), 6 the shape's coverage
//   alone, 7 the picture.
struct Params {
    color: vec4<f32>,
    mode: f32,
    threshold: f32,
    knee: f32,
    tint: f32,
}

fn pick(uv: vec2<f32>) -> vec4<f32> {
    let s = textureSampleLevel(src, smp, uv, 0.0);
    let m = u32(u.p.mode);
    if m == 0u {
        return s;
    }
    if m == 7u {
        return lin(s);
    }
    let tinted = to_lin(u.p.color.rgb);
    if m == 1u {
        let c = lin(s);
        // Soft knee: a quadratic ease into the threshold, then linear.
        let br = max(max(c.r, c.g), c.b);
        let k = max(u.p.knee * u.p.threshold, 1e-4);
        var q = clamp(br - u.p.threshold + k, 0.0, 2.0 * k);
        q = q * q / (4.0 * k);
        let w = max(q, br - u.p.threshold) / max(br, 1e-4);
        let rgb = mix(c.rgb, luma(c.rgb) * tinted, u.p.tint);
        // Coverage is what glows, not how much: the way up reads it to tell
        // a small source from a big one (`pyramid_up.wgsl`'s `sparse`).
        return vec4<f32>(rgb * w, select(0.0, c.a, w > 0.0));
    }
    if m == 3u {
        return vec4<f32>(tinted, 1.0) * (1.0 - s.a);
    }
    if m == 5u {
        let b = lin(textureSampleLevel(aux, smp, uv, 0.0));
        return vec4<f32>(b.rgb, 1.0) * (1.0 - s.a);
    }
    if m == 6u {
        return vec4<f32>(s.a);
    }
    var own = unpremul(lin(s)).rgb;
    if m == 4u {
        own = max(mix(vec3<f32>(luma(own)), own, 1.6), vec3<f32>(0.0));
    }
    return vec4<f32>(mix(own, tinted, u.p.tint), 1.0) * s.a;
}

// A box of four taps; for bloom, weighted down by its brightness (Karis'
// average) so one hot pixel does not flicker as a big blob.
fn box(uv: vec2<f32>, ts: vec2<f32>, a: vec2<f32>, b: vec2<f32>, c: vec2<f32>, d: vec2<f32>) -> vec4<f32> {
    let s = (pick(uv + a * ts) + pick(uv + b * ts) + pick(uv + c * ts) + pick(uv + d * ts)) * 0.25;
    if u32(u.p.mode) == 1u {
        return s / (1.0 + luma(s.rgb));
    }
    return s;
}

@fragment
fn fs(@builtin(position) pos: vec4<f32>) -> @location(0) vec4<f32> {
    let uv = pos.xy / u.res;
    let ts = 1.0 / vec2<f32>(textureDimensions(src));
    let mid = box(uv, ts, vec2(-1.0, -1.0), vec2(1.0, -1.0), vec2(-1.0, 1.0), vec2(1.0, 1.0));
    let tl = box(uv, ts, vec2(-2.0, -2.0), vec2(0.0, -2.0), vec2(-2.0, 0.0), vec2(0.0, 0.0));
    let tr = box(uv, ts, vec2(0.0, -2.0), vec2(2.0, -2.0), vec2(0.0, 0.0), vec2(2.0, 0.0));
    let bl = box(uv, ts, vec2(-2.0, 0.0), vec2(0.0, 0.0), vec2(-2.0, 2.0), vec2(0.0, 2.0));
    let br = box(uv, ts, vec2(0.0, 0.0), vec2(2.0, 0.0), vec2(0.0, 2.0), vec2(2.0, 2.0));
    var c = mid * 0.5 + (tl + tr + bl + br) * 0.125;
    if u32(u.p.mode) == 1u {
        // Undo the Karis weighting's overall darkening.
        c = c * (1.0 + luma(c.rgb));
    }
    return c;
}
