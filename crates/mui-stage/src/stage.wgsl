// The stage: MUI layers as textured slabs in a lit 3D scene, then bloom,
// aberration, vignette, tonemap and grain. All colour here is linear light
// until `fs_final` encodes it for display.

struct Globals {
    view_proj: mat4x4f,
    eye: vec4f,
    // x: time in seconds, yz: output size in pixels.
    time_res: vec4f,
};
@group(0) @binding(0) var<uniform> g: Globals;

struct Draw {
    model: mat4x4f,
    // xy: layer size in world units (logical px), z: slab depth, w: glow.
    size: vec4f,
    // rgb: edge colour (linear), a: opacity.
    edge: vec4f,
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
};
@group(2) @binding(2) var<uniform> post: Post;
@group(2) @binding(3) var bloom_tex: texture_2d<f32>;

// --- the background: `Stage::background` splices its function in below ---

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

@fragment fn fs_bg(i: Full) -> @location(0) vec4f {
    return vec4f(background(i.uv, g.time_res.x), 1.);
}

// --- slabs ----------------------------------------------------------------

struct Cap {
    @builtin(position) pos: vec4f,
    @location(0) uv: vec2f,
};
fn cap(i: u32, z: f32) -> Cap {
    var corners = array<vec2f, 6>(
        vec2f(0., 0.), vec2f(1., 0.), vec2f(0., 1.),
        vec2f(0., 1.), vec2f(1., 0.), vec2f(1., 1.));
    let uv = corners[i];
    // Layer space is y-down pixels; the world is y-up, centred on the layer.
    let local = vec4f((uv.x - 0.5) * d.size.x, (0.5 - uv.y) * d.size.y, z, 1.);
    var o: Cap;
    o.pos = g.view_proj * d.model * local;
    o.uv = uv;
    return o;
}
@vertex fn vs_front(@builtin(vertex_index) i: u32) -> Cap { return cap(i, 0.); }
@vertex fn vs_back(@builtin(vertex_index) i: u32) -> Cap { return cap(i, -d.size.z); }

@fragment fn fs_front(i: Cap) -> @location(0) vec4f {
    let c = textureSample(tex, samp, i.uv);
    if (c.a < 0.004) { discard; }
    return vec4f(c.rgb * d.size.w, c.a) * d.edge.a;
}
@fragment fn fs_back(i: Cap) -> @location(0) vec4f {
    let a = textureSample(tex, samp, i.uv).a;
    if (a < 0.004) { discard; }
    return vec4f(d.edge.rgb * 0.3, 1.) * a * d.edge.a;
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
@fragment fn fs_wall(i: Wall) -> @location(0) vec4f {
    let n = normalize(i.normal);
    let l = normalize(vec3f(-0.4, 0.6, 0.7));
    let v = normalize(g.eye.xyz - i.world);
    let h = normalize(l + v);
    let diffuse = 0.3 + 0.7 * max(dot(n, l), 0.);
    let spec = pow(max(dot(n, h), 0.), 48.) * 0.8;
    let rim = pow(1. - max(dot(n, v), 0.), 3.) * 0.35;
    return vec4f(d.edge.rgb * diffuse + vec3f(spec + rim), 1.) * d.edge.a;
}

// --- post -----------------------------------------------------------------

@fragment fn fs_copy(i: Full) -> @location(0) vec4f {
    return textureSample(tex, samp, i.uv);
}

@fragment fn fs_prefilter(i: Full) -> @location(0) vec4f {
    let c = textureSample(tex, samp, i.uv).rgb;
    let br = max(c.r, max(c.g, c.b));
    let knee = post.bloom.y;
    var soft = clamp(br - post.bloom.x + knee, 0., 2. * knee);
    soft = soft * soft / (4. * knee + 1e-5);
    let w = max(soft, br - post.bloom.x) / max(br, 1e-5);
    return vec4f(c * w, 1.);
}

@fragment fn fs_down(i: Full) -> @location(0) vec4f {
    let t = 1. / vec2f(textureDimensions(tex));
    var c = textureSample(tex, samp, i.uv) * 0.5;
    c += textureSample(tex, samp, i.uv + vec2f(-t.x, -t.y)) * 0.125;
    c += textureSample(tex, samp, i.uv + vec2f(t.x, -t.y)) * 0.125;
    c += textureSample(tex, samp, i.uv + vec2f(-t.x, t.y)) * 0.125;
    c += textureSample(tex, samp, i.uv + vec2f(t.x, t.y)) * 0.125;
    return vec4f(c.rgb, 1.);
}

@fragment fn fs_up(i: Full) -> @location(0) vec4f {
    let t = 1. / vec2f(textureDimensions(tex));
    var c = textureSample(tex, samp, i.uv) * 4.;
    c += textureSample(tex, samp, i.uv + vec2f(-t.x, 0.)) * 2.;
    c += textureSample(tex, samp, i.uv + vec2f(t.x, 0.)) * 2.;
    c += textureSample(tex, samp, i.uv + vec2f(0., -t.y)) * 2.;
    c += textureSample(tex, samp, i.uv + vec2f(0., t.y)) * 2.;
    c += textureSample(tex, samp, i.uv + vec2f(-t.x, -t.y));
    c += textureSample(tex, samp, i.uv + vec2f(t.x, -t.y));
    c += textureSample(tex, samp, i.uv + vec2f(-t.x, t.y));
    c += textureSample(tex, samp, i.uv + vec2f(t.x, t.y));
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
        textureSample(tex, samp, i.uv - ca).r,
        textureSample(tex, samp, i.uv).g,
        textureSample(tex, samp, i.uv + ca).b);
    c += textureSample(bloom_tex, samp, i.uv).rgb * post.bloom.z;
    c *= post.bloom.w;
    c *= mix(1., smoothstep(0.85, 0.2, length(dir * vec2f(1., 0.8))), post.lens.y);
    var o = srgb(select(c, aces(c), post.film.x > 0.5));
    let px = i.uv * g.time_res.yz;
    o += (hash2(px + fract(post.lens.w * 7.31) * 1000.) - 0.5) * post.lens.z;
    return vec4f(clamp(o, vec3f(0.), vec3f(1.)), 1.);
}
