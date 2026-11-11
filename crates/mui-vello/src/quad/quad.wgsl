// One instanced quad per paint entry, or per glyph: a rounded rect filled,
// stroked or blurred, or a glyph's atlas cell, painted solid, with a
// gradient or with an image, clipped by at most one device rectangle and
// one rounded rect. Shape coverage is analytic box-filter area; a glyph's is
// its atlas cell, read one texel a pixel.
//
// Colours arrive premultiplied, in sRGB-encoded space: `vello_cpu` blends
// encoded values, and so does this pass (the target is a non-sRGB format).

struct View {
    size: vec2<f32>,
    pad: vec2<f32>,
}

@group(0) @binding(0) var<uniform> view: View;
// Glyph coverage, one layer a page.
@group(0) @binding(1) var atlas: texture_2d_array<f32>;
@group(0) @binding(2) var linear: sampler;
// Premultiplied RGBA.
@group(1) @binding(0) var image: texture_2d<f32>;

// See `Instance` in build.rs for what each slot holds.
struct Instance {
    @location(0) xf: vec4<f32>,
    @location(1) xt: vec4<f32>,
    @location(2) rect: vec4<f32>,
    @location(3) kind: vec4<f32>,
    @location(4) c0: vec4<f32>,
    @location(5) c1: vec4<f32>,
    @location(6) c2: vec4<f32>,
    @location(7) c3: vec4<f32>,
    @location(8) offs: vec4<f32>,
    @location(9) geom: vec4<f32>,
    @location(10) dclip: vec4<f32>,
    @location(11) cxf: vec4<f32>,
    @location(12) cxt: vec4<f32>,
    @location(13) crect: vec4<f32>,
}

struct Varying {
    @builtin(position) pos: vec4<f32>,
    @location(0) q: vec2<f32>,
    @location(1) @interpolate(flat) ji: vec4<f32>,
    @location(2) @interpolate(flat) rect: vec4<f32>,
    @location(3) @interpolate(flat) kind: vec4<f32>,
    @location(4) @interpolate(flat) c0: vec4<f32>,
    @location(5) @interpolate(flat) c1: vec4<f32>,
    @location(6) @interpolate(flat) c2: vec4<f32>,
    @location(7) @interpolate(flat) c3: vec4<f32>,
    @location(8) @interpolate(flat) offs: vec4<f32>,
    @location(9) @interpolate(flat) geom: vec4<f32>,
    @location(10) @interpolate(flat) dclip: vec4<f32>,
    @location(11) @interpolate(flat) cxf: vec4<f32>,
    @location(12) @interpolate(flat) cxt: vec4<f32>,
    @location(13) @interpolate(flat) crect: vec4<f32>,
}

const SHAPE_STROKE: f32 = 1.0;
const SHAPE_BLUR: f32 = 2.0;
const SHAPE_BLUR_INVERT: f32 = 3.0;
const SHAPE_GLYPH: f32 = 4.0;
const PAINT_IMAGE: f32 = 4.0;

// The instance code, `page * 64 + shape * 8 + paint`, as (page, shape, paint).
fn decode(code: f32) -> vec3<f32> {
    let page = floor(code / 64.0);
    let rest = code - 64.0 * page;
    let shape = floor(rest / 8.0);
    return vec3<f32>(page, shape, rest - 8.0 * shape);
}

@vertex
fn vs(@builtin(vertex_index) vi: u32, i: Instance) -> Varying {
    // kurbo order: x' = a x + c y + e, y' = b x + d y + f.
    let a = i.xf.x;
    let b = i.xf.y;
    let c = i.xf.z;
    let d = i.xf.w;
    let det = a * d - b * c;
    // One device pixel of margin in local units, for the antialiased edge:
    // the reciprocal of the smallest singular value of the linear part.
    let e = 0.5 * (a * a + b * b + c * c + d * d);
    let smax = sqrt(e + sqrt(max(e * e - det * det, 0.0)));
    let margin = smax / abs(det);
    let shape = decode(i.kind.x).y;
    var grow = margin;
    if shape == SHAPE_STROKE {
        grow += 0.5 * i.kind.w;
    }
    let corner = vec2<f32>(f32(vi & 1u), f32(vi >> 1u));
    let lo = i.rect.xy - vec2<f32>(grow);
    let hi = i.rect.zw + vec2<f32>(grow);
    var q = mix(lo, hi, corner);
    var p = vec2<f32>(a * q.x + c * q.y + i.xt.x, b * q.x + d * q.y + i.xt.y);
    if shape == SHAPE_GLYPH {
        // The cell, in device pixels; `q` maps back to local for the paint.
        p = mix(i.rect.xy, i.rect.zw, corner);
        let r = p - i.xt.xy;
        q = vec2<f32>(d * r.x - c * r.y, a * r.y - b * r.x) / det;
    }
    var o: Varying;
    o.pos = vec4<f32>(p.x / view.size.x * 2.0 - 1.0, 1.0 - p.y / view.size.y * 2.0, 0.0, 1.0);
    o.q = q;
    // Inverse of the linear part, rows (x, y) and (z, w): local = ji * device.
    o.ji = vec4<f32>(d, -c, -b, a) / det;
    o.rect = i.rect;
    o.kind = i.kind;
    o.c0 = i.c0;
    o.c1 = i.c1;
    o.c2 = i.c2;
    o.c3 = i.c3;
    o.offs = i.offs;
    o.geom = i.geom;
    o.dclip = i.dclip;
    o.cxf = i.cxf;
    o.cxt = i.cxt;
    o.crect = i.crect;
    return o;
}

// Box-filter coverage of a half-plane over a unit pixel: `s` is the signed
// distance of the pixel centre inside the edge, `n` the unit normal. The
// pixel projects onto `n` as the convolution of two boxes, |n.x| and |n.y|
// wide: a trapezoid, whose integral is this.
fn half_plane(s: f32, n: vec2<f32>) -> f32 {
    let lo = max(min(abs(n.x), abs(n.y)), 1e-5);
    let hi = max(abs(n.x), abs(n.y));
    let h = 0.5 * (lo + hi);
    let k = 0.5 * (hi - lo);
    if s >= h {
        return 1.0;
    }
    if s <= -h {
        return 0.0;
    }
    if s < -k {
        let u = s + h;
        return u * u / (2.0 * lo * hi);
    }
    if s > k {
        let u = h - s;
        return 1.0 - u * u / (2.0 * lo * hi);
    }
    return 0.5 + s / hi;
}

// Coverage of the edge whose local inward distance is `s` and whose local
// normal is `g`, under the inverse linear map `ji` (rows xy and zw).
fn edge(s: f32, g: vec2<f32>, ji: vec4<f32>) -> f32 {
    // The device gradient of a local distance is ji^T g.
    let m = vec2<f32>(ji.x * g.x + ji.z * g.y, ji.y * g.x + ji.w * g.y);
    let len = length(m);
    return half_plane(s / len, m / len);
}

// Coverage of a rounded rect (`r` = x0 y0 x1 y1, local) at local point `q`.
// The corner is the exact circle. `vello_cpu` covers the polygon it
// flattens the arc to (0.25 px tolerance), so corner pixels differ by up to
// a quarter of a pixel's area; the pixel test states that tolerance.
fn rounded(q: vec2<f32>, r: vec4<f32>, radius: f32, ji: vec4<f32>) -> f32 {
    if r.z <= r.x || r.w <= r.y {
        return 0.0;
    }
    // Most fragments sit a pixel or more inside every edge, where each
    // `edge` below would return exactly 1: a pixel reaches at most `reach`
    // local units from its centre (0.71 of the inverse map's Frobenius norm
    // bounds half its diagonal), so skip the edge maths there.
    let reach = 0.71 * sqrt(dot(ji, ji));
    let d = min(q - r.xy, r.zw - q);
    let centre = clamp(q, r.xy + vec2<f32>(radius), r.zw - vec2<f32>(radius));
    let v = q - centre;
    if min(d.x, d.y) >= reach && (radius <= 0.0 || v.x == 0.0 || v.y == 0.0 || length(v) <= radius - reach) {
        return 1.0;
    }
    let sx = clamp(edge(q.x - r.x, vec2<f32>(1.0, 0.0), ji) + edge(r.z - q.x, vec2<f32>(1.0, 0.0), ji) - 1.0, 0.0, 1.0);
    let sy = clamp(edge(q.y - r.y, vec2<f32>(0.0, 1.0), ji) + edge(r.w - q.y, vec2<f32>(0.0, 1.0), ji) - 1.0, 0.0, 1.0);
    let box = sx * sy;
    if radius <= 0.0 {
        return box;
    }
    if v.x == 0.0 || v.y == 0.0 {
        return box;
    }
    let len = length(v);
    return min(box, edge(radius - len, v / len, ji));
}

// `vello_common::math::compute_erf7`, term for term.
fn erf7(x0: f32) -> f32 {
    let x1 = clamp(x0, -10.0, 10.0) * 1.1283791670955126;
    let xx = x1 * x1;
    let x = x1 + (0.24295 + (0.03395 + 0.0104 * xx) * xx) * (x1 * xx);
    return x / sqrt(1.0 + x * x);
}

fn power(x: f32, e: f32) -> f32 {
    if x <= 0.0 {
        return 0.0;
    }
    return pow(x, e);
}

// `vello_cpu`'s blurred rounded rect (after raph's blurrr distfield), with
// its encoded parameters: c1 = (1/std_dev, exponent, scale, r1),
// c2 = (w, h), c3 = the unblurred rect.
fn blurred(q: vec2<f32>, c1: vec4<f32>, c2: vec4<f32>, r: vec4<f32>) -> f32 {
    let width = r.z - r.x;
    let height = r.w - r.y;
    let ij = q - r.xy;
    let y = ij.y - 0.5 * height;
    let y0 = c1.w - (0.5 * c2.y - abs(y));
    let y1 = max(y0, 0.0);
    let x = ij.x - 0.5 * width;
    let x0 = c1.w - (0.5 * c2.x - abs(x));
    let x1 = max(x0, 0.0);
    let d_pos = power(power(x1, c1.y) + power(y1, c1.y), 1.0 / c1.y);
    let d_neg = min(max(x0, y0), 0.0);
    let d = d_pos + d_neg - c1.w;
    let min_edge = min(width, height);
    return c1.z * (erf7(c1.x * (min_edge + d)) - erf7(c1.x * d));
}

// The gradient parameter at `q`; `paint` 1 linear, 2 radial, 3 sweep.
fn ramp_t(q: vec2<f32>, paint: f32, g: vec4<f32>) -> f32 {
    if paint == 1.0 {
        let dir = g.zw - g.xy;
        return dot(q - g.xy, dir) / dot(dir, dir);
    }
    if paint == 2.0 {
        return length(q - g.xy) / g.z;
    }
    let v = q - g.xy;
    var angle = atan2(v.y, v.x);
    if angle < 0.0 {
        angle += 6.283185307179586;
    }
    return (angle - g.z) * g.w;
}

fn ramp(t0: f32, scale: f32, v: Varying) -> vec4<f32> {
    // Pad, then snap to the lookup table `vello_cpu` samples.
    let t = floor(clamp(t0, 0.0, 1.0) * scale) / scale;
    var c: vec4<f32>;
    if t < v.offs.x {
        c = v.c0;
    } else if t < v.offs.y {
        c = mix(v.c0, v.c1, (t - v.offs.x) / max(v.offs.y - v.offs.x, 1e-6));
    } else if t < v.offs.z {
        c = mix(v.c1, v.c2, (t - v.offs.y) / max(v.offs.z - v.offs.y, 1e-6));
    } else if t < v.offs.w {
        c = mix(v.c2, v.c3, (t - v.offs.z) / max(v.offs.w - v.offs.z, 1e-6));
    } else {
        c = v.c3;
    }
    let a = clamp(c.a, 0.0, 1.0);
    return vec4<f32>(min(clamp(c.rgb, vec3<f32>(0.0), vec3<f32>(1.0)), vec3<f32>(a)), a);
}

fn span(lo: f32, hi: f32, p: f32) -> f32 {
    return clamp(min(p + 0.5, hi) - max(p - 0.5, lo), 0.0, 1.0);
}

fn clip(p: vec2<f32>, v: Varying) -> f32 {
    var cov = span(v.dclip.x, v.dclip.z, p.x) * span(v.dclip.y, v.dclip.w, p.y);
    if v.cxt.w > 0.0 && cov > 0.0 {
        let q = vec2<f32>(
            v.cxf.x * p.x + v.cxf.z * p.y + v.cxt.x,
            v.cxf.y * p.x + v.cxf.w * p.y + v.cxt.y,
        );
        // Rows of the clip's inverse linear part, as `edge` wants them.
        let ji = vec4<f32>(v.cxf.x, v.cxf.z, v.cxf.y, v.cxf.w);
        cov *= rounded(q, v.crect, v.cxt.z, ji);
    }
    return cov;
}

@fragment
fn fs(v: Varying) -> @location(0) vec4<f32> {
    let code = decode(v.kind.x);
    let shape = code.y;
    let paint = code.z;
    let radius = v.kind.z;
    let width = v.kind.w;
    let q = v.q;
    var cov: f32;
    if shape == SHAPE_GLYPH {
        let texel = vec2<i32>(floor(v.pos.xy - v.rect.xy + v.kind.zw));
        cov = textureLoad(atlas, texel, i32(code.x), 0).r;
    } else if shape == SHAPE_STROKE {
        let h = 0.5 * width;
        let outer = rounded(q, v.rect + vec4<f32>(-h, -h, h, h), radius + h, v.ji);
        let inner = rounded(q, v.rect + vec4<f32>(h, h, -h, -h), max(radius - h, 0.0), v.ji);
        cov = max(outer - inner, 0.0);
    } else {
        // A blurred rect paints its inflated box, sharp-edged.
        cov = rounded(q, v.rect, select(radius, 0.0, shape == SHAPE_BLUR || shape == SHAPE_BLUR_INVERT), v.ji);
    }
    cov *= clip(v.pos.xy, v);
    if cov <= 0.0 {
        discard;
    }
    var colour: vec4<f32>;
    if shape == SHAPE_BLUR || shape == SHAPE_BLUR_INVERT {
        var z = blurred(q, v.c1, v.c2, v.c3);
        if shape == SHAPE_BLUR_INVERT {
            z = 1.0 - z;
        }
        colour = v.c0 * z;
    } else if paint == 0.0 {
        colour = v.c0;
    } else if paint == PAINT_IMAGE {
        // Image pixels from the box's local space; bilinear, edges clamped.
        let at = (q - v.geom.xy) / v.geom.zw;
        colour = textureSampleLevel(image, linear, at / vec2<f32>(textureDimensions(image)), 0.0);
    } else {
        colour = ramp(ramp_t(q, paint, v.geom), v.kind.y, v);
    }
    // `vello_cpu` holds its paint as premultiplied u8 before covering it.
    return floor(colour * 255.0 + 0.5) / 255.0 * cov;
}
