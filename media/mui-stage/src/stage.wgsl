// The stage: MUI layers as textured slabs in a lit 3D scene over a floor that
// reflects them, then depth of field, bloom, aberration, vignette, tonemap
// and grain. All colour here is linear light until `fs_final` encodes it.

struct Globals {
    view_proj: mat4x4f,
    eye: vec4f,
    // x: time in seconds, yz: output size in pixels.
    time_res: vec4f,
    // The floor's colour, linear.
    floor: vec4f,
};
@group(0) @binding(0) var<uniform> g: Globals;

struct Draw {
    model: mat4x4f,
    // xy: layer size in world units (logical px), z: slab depth, w: glow.
    size: vec4f,
    // rgb: edge colour (linear), a: opacity.
    edge: vec4f,
    // Reflected draws: x floor height, y strength, z height falloff,
    // w floor radius (0 = not a reflection).
    mirror: vec4f,
};
@group(1) @binding(0) var<uniform> d: Draw;

@group(2) @binding(0) var tex: texture_2d<f32>;
@group(2) @binding(1) var samp: sampler;

struct Post {
    // threshold, knee, bloom strength, exposure
    bloom: vec4f,
    // aberration, vignette, grain, time
    lens: vec4f,
    // x: tonemap on
    film: vec4f,
    // focus distance, aperture (px of blur at infinity), max blur px
    dof: vec4f,
};
@group(2) @binding(2) var<uniform> post: Post;
// Bloom for `fs_final`, view distance for `fs_dof`, raw Vello pixels for
// `fs_linearize`.
@group(2) @binding(3) var aux: texture_2d<f32>;

// Colour, and the distance from the eye for depth of field.
struct Out {
    @location(0) color: vec4f,
    @location(1) dist: vec4f,
};
fn out(c: vec4f, world: vec3f) -> Out {
    var o: Out;
    o.color = c;
    o.dist = vec4f(length(world - g.eye.xyz), 0., 0., 1.);
    return o;
}

fn hash2(p: vec2f) -> f32 {
    let q = fract(p * vec2f(123.34, 456.21));
    let r = q + dot(q, q + 45.32);
    return fract(r.x * r.y);
}
fn noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3. - 2. * f);
    return mix(mix(hash2(i), hash2(i + vec2f(1., 0.)), u.x),
               mix(hash2(i + vec2f(0., 1.)), hash2(i + vec2f(1., 1.)), u.x), u.y);
}
fn fbm(p: vec2f) -> f32 {
    var v = 0.;
    var a = 0.5;
    var q = p;
    for (var i = 0; i < 5; i++) {
        v += a * noise(q);
        q = q * 2.03 + vec2f(1.7, 9.2);
        a *= 0.5;
    }
    return v;
}

// --- the background: `Stage::background` splices its function in below ---

{{BACKGROUND}}

// --- full-screen passes ---------------------------------------------------

struct Full {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};
@vertex fn vs_full(@builtin(vertex_index) i: u32) -> Full {
    let p = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var o: Full;
    o.pos = vec4f(p * 2. - 1., 0., 1.);
    o.uv = vec2f(p.x, 1. - p.y);
    return o;
}

@fragment fn fs_bg(i: Full) -> Out {
    var o: Out;
    o.color = vec4f(background(i.uv, g.time_res.x), 1.);
    o.dist = vec4f(60000., 0., 0., 1.);
    return o;
}

// --- slabs ----------------------------------------------------------------

struct Cap {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
    @location(1) world: vec3f,
};
fn cap(i: u32, z: f32) -> Cap {
    var corners = array<vec2f, 6>(
        vec2f(0., 0.), vec2f(1., 0.), vec2f(0., 1.),
        vec2f(0., 1.), vec2f(1., 0.), vec2f(1., 1.));
    let uv = corners[i];
    // Layer space is y-down pixels; the world is y-up, centred on the layer.
    let local = vec4f((uv.x - 0.5) * d.size.x, (0.5 - uv.y) * d.size.y, z, 1.);
    let world = d.model * local;
    var o: Cap;
    o.pos = g.view_proj * world;
    o.uv = uv;
    o.world = world.xyz;
    return o;
}
@vertex fn vs_front(@builtin(vertex_index) i: u32) -> Cap { return cap(i, 0.); }
@vertex fn vs_back(@builtin(vertex_index) i: u32) -> Cap { return cap(i, -d.size.z); }

// Seen in the floor, a surface keeps its coverage, so a nearer reflection
// hides a farther one, but its light fades into the floor's colour with its
// depth below the floor; the whole reflection thins out toward the floor's
// rim. Anything the mirror put above the floor is not seen in it.
fn mirrored(c: vec4f, world: vec3f) -> vec4f {
    if (d.mirror.w <= 0.) { return c; }
    let below = d.mirror.x - world.y;
    if (below < -0.5) { return vec4f(0.); }
    let r = length(world.xz) / d.mirror.w;
    let f = d.mirror.y * exp(-max(below, 0.) / d.mirror.z);
    return vec4f(mix(g.floor.rgb * c.a, c.rgb, f), c.a) * exp(-r * r);
}

@fragment fn fs_front(i: Cap) -> Out {
    let c = textureSample(tex, samp, i.uv);
    let o = mirrored(vec4f(c.rgb * d.size.w, c.a) * d.edge.a, i.world);
    if (o.a < 0.004) { discard; }
    return out(o, i.world);
}
@fragment fn fs_back(i: Cap) -> Out {
    let a = textureSample(tex, samp, i.uv).a;
    let o = mirrored(vec4f(d.edge.rgb * 0.3, 1.) * a * d.edge.a, i.world);
    if (o.a < 0.004) { discard; }
    return out(o, i.world);
}

struct Wall {
    @builtin(position) pos: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
};
@vertex fn vs_wall(@location(0) p: vec3f, @location(1) n: vec3f) -> Wall {
    let world = d.model * vec4f(p, 1.);
    var o: Wall;
    o.pos = g.view_proj * world;
    o.world = world.xyz;
    o.normal = normalize((d.model * vec4f(n, 0.)).xyz);
    return o;
}
@fragment fn fs_wall(i: Wall) -> Out {
    let n = normalize(i.normal);
    let l = normalize(vec3f(-0.4, 0.6, 0.7));
    let v = normalize(g.eye.xyz - i.world);
    let h = normalize(l + v);
    let diffuse = 0.3 + 0.7 * max(dot(n, l), 0.);
    let spec = pow(max(dot(n, h), 0.), 48.) * 0.8;
    let rim = pow(1. - max(dot(n, v), 0.), 3.) * 0.35;
    let o = mirrored(vec4f(d.edge.rgb * diffuse + vec3f(spec + rim), 1.) * d.edge.a, i.world);
    if (o.a < 0.004) { discard; }
    return out(o, i.world);
}

// The floor: `size.x` radius, `edge.rgb` colour, `mirror.x` height. It fades
// out radially so it melts into the background instead of ending.
@vertex fn vs_floor(@builtin(vertex_index) i: u32) -> Cap {
    var corners = array<vec2f, 6>(
        vec2f(-1., -1.), vec2f(1., -1.), vec2f(-1., 1.),
        vec2f(-1., 1.), vec2f(1., -1.), vec2f(1., 1.));
    let c = corners[i] * d.size.x * 3.;
    let world = vec3f(c.x, d.mirror.x, c.y);
    var o: Cap;
    o.pos = g.view_proj * vec4f(world, 1.);
    o.uv = c;
    o.world = world;
    return o;
}
@fragment fn fs_floor(i: Cap) -> Out {
    let r = length(i.world.xz) / d.size.x;
    let a = exp(-r * r) * d.edge.a;
    return out(vec4f(d.edge.rgb * a, a), i.world);
}

// --- layers ---------------------------------------------------------------

// Vello's premultiplied sRGB bytes to premultiplied linear light, so an
// antialiased edge composites at its true brightness and mips average light.
@fragment fn fs_linearize(i: Full) -> @location(0) vec4f {
    let c = textureLoad(aux, vec2i(i.pos.xy), 0);
    if (c.a <= 0.) { return vec4f(0.); }
    let s = clamp(c.rgb / c.a, vec3f(0.), vec3f(1.));
    let lin = select(pow((s + 0.055) / 1.055, vec3f(2.4)), s / 12.92, s <= vec3f(0.04045));
    return vec4f(lin * c.a, c.a);
}

// --- post -----------------------------------------------------------------

@fragment fn fs_copy(i: Full) -> @location(0) vec4f {
    return textureSampleLevel(tex, samp, i.uv, 0.);
}

fn coc(dist: f32) -> f32 {
    return clamp(post.dof.y * abs(dist - post.dof.x) / max(dist, 1.), 0., post.dof.z);
}
// Gather depth of field: 64 taps on a golden-angle spiral. A tap counts where
// its own circle of confusion reaches this pixel; one behind a sharper pixel
// is held to this pixel's circle, so a blurred background does not bleed over
// a sharp foreground edge.
@fragment fn fs_dof(i: Full) -> @location(0) vec4f {
    let px = 1. / g.time_res.yz;
    let d0 = textureSampleLevel(aux, samp, i.uv, 0.).r;
    let c0 = coc(d0);
    var sum = textureSampleLevel(tex, samp, i.uv, 0.).rgb;
    var w = 1.;
    for (var k = 1; k < 64; k++) {
        let r = sqrt(f32(k) / 64.) * post.dof.z;
        let a = f32(k) * 2.39996;
        let uv = i.uv + vec2f(cos(a), sin(a)) * r * px;
        let dk = textureSampleLevel(aux, samp, uv, 0.).r;
        let ck = select(coc(dk), min(coc(dk), c0), dk > d0);
        let wk = smoothstep(r - 1., r + 1., ck);
        sum += textureSampleLevel(tex, samp, uv, 0.).rgb * wk;
        w += wk;
    }
    return vec4f(sum / w, 1.);
}

@fragment fn fs_prefilter(i: Full) -> @location(0) vec4f {
    let c = textureSampleLevel(tex, samp, i.uv, 0.).rgb;
    let br = max(c.r, max(c.g, c.b));
    let knee = post.bloom.y;
    var soft = clamp(br - post.bloom.x + knee, 0., 2. * knee);
    soft = soft * soft / (4. * knee + 1e-5);
    let w = max(soft, br - post.bloom.x) / max(br, 1e-5);
    return vec4f(c * w, 1.);
}

@fragment fn fs_down(i: Full) -> @location(0) vec4f {
    let t = 1. / vec2f(textureDimensions(tex));
    var c = textureSampleLevel(tex, samp, i.uv, 0.) * 0.5;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, -t.y), 0.) * 0.125;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, -t.y), 0.) * 0.125;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, t.y), 0.) * 0.125;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, t.y), 0.) * 0.125;
    return vec4f(c.rgb, 1.);
}

@fragment fn fs_up(i: Full) -> @location(0) vec4f {
    let t = 1. / vec2f(textureDimensions(tex));
    var c = textureSampleLevel(tex, samp, i.uv, 0.) * 4.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, 0.), 0.) * 2.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, 0.), 0.) * 2.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(0., -t.y), 0.) * 2.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(0., t.y), 0.) * 2.;
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, -t.y), 0.);
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, -t.y), 0.);
    c += textureSampleLevel(tex, samp, i.uv + vec2f(-t.x, t.y), 0.);
    c += textureSampleLevel(tex, samp, i.uv + vec2f(t.x, t.y), 0.);
    return vec4f(c.rgb / 16., 1.);
}

fn aces(x: vec3f) -> vec3f {
    return clamp((x * (2.51 * x + 0.03)) / (x * (2.43 * x + 0.59) + 0.14), vec3f(0.), vec3f(1.));
}
fn srgb(c: vec3f) -> vec3f {
    return select(1.055 * pow(c, vec3f(1. / 2.4)) - 0.055, c * 12.92, c <= vec3f(0.0031308));
}

@fragment fn fs_final(i: Full) -> @location(0) vec4f {
    let dir = i.uv - 0.5;
    let ca = post.lens.x * dir;
    var c = vec3f(
        textureSampleLevel(tex, samp, i.uv - ca, 0.).r,
        textureSampleLevel(tex, samp, i.uv, 0.).g,
        textureSampleLevel(tex, samp, i.uv + ca, 0.).b);
    c += textureSampleLevel(aux, samp, i.uv, 0.).rgb * post.bloom.z;
    c *= post.bloom.w;
    c *= mix(1., smoothstep(0.85, 0.2, length(dir * vec2f(1., 0.8))), post.lens.y);
    var o = srgb(select(c, aces(c), post.film.x > 0.5));
    let px = i.uv * g.time_res.yz;
    o += (hash2(px + fract(post.lens.w * 7.31) * 1000.) - 0.5) * post.lens.z;
    return vec4f(clamp(o, vec3f(0.), vec3f(1.)), 1.);
}
