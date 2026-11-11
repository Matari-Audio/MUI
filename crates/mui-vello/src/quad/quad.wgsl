// One instanced quad per paint entry, or per glyph: a rounded rect filled,
// stroked or blurred, a path filled, stroked or blurred, or a glyph's atlas
// cell, painted solid, with a gradient or with an image, clipped by at most
// one device rectangle and one rounded rect or path. Shape coverage is
// analytic box-filter area; a path's is read from its record (path.rs) and
// a glyph's from its atlas cell, one texel a pixel.
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
// Path records, read by linear texel index (path.rs has the layout).
@group(0) @binding(3) var paths: texture_2d<f32>;
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
    @location(14) pclip: vec4<f32>,
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
    @location(14) @interpolate(flat) pclip: vec4<f32>,
}

const SHAPE_STROKE: f32 = 1.0;
const SHAPE_BLUR: f32 = 2.0;
const SHAPE_BLUR_INVERT: f32 = 3.0;
const SHAPE_GLYPH: f32 = 4.0;
const SHAPE_PATH: f32 = 5.0;
const SHAPE_PATH_STROKE: f32 = 6.0;
const SHAPE_PATH_BLUR: f32 = 7.0;
const PAINT_IMAGE: f32 = 4.0;
// Texels a row of `paths`: `path::WIDTH`.
const PATH_WIDTH: u32 = 256u;

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
    if shape == SHAPE_STROKE || shape == SHAPE_PATH_STROKE {
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
    if shape >= SHAPE_PATH {
        // A path's record is in device orientation, translation-free.
        q = p - i.xt.xy;
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
    o.pclip = i.pclip;
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
    let hi = c1.x * (min_edge + d);
    let lo = c1.x * d;
    // Deep inside, both terms are within 2e-5 of +-1, far below a u8 step:
    // skip them, as Rune's `shadow` skips its interior.
    if hi >= 3.5 && lo <= -3.5 {
        return 2.0 * c1.z;
    }
    return c1.z * (erf7(hi) - erf7(lo));
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

fn path_texel(i: u32) -> vec4<f32> {
    return textureLoad(paths, vec2<i32>(i32(i % PATH_WIDTH), i32(i / PATH_WIDTH)), 0);
}

// The (first texel, count) of the cell of record `rec` that holds `r`
// (device, translation-free); (0, 0) outside its grid, where nothing is
// covered.
fn path_cell(rec: u32, r: vec2<f32>) -> vec2<u32> {
    let h = path_texel(rec);
    let n = path_texel(rec + 1u).xy;
    let c = floor((r - h.xy) * h.zw);
    if any(c < vec2<f32>(0.0)) || any(c >= n) {
        return vec2<u32>(0u);
    }
    let e = path_texel(rec + 2u + u32(c.y * n.x + c.x));
    return vec2<u32>(u32(e.x), u32(e.y));
}

// The integral of clamp(u, 0, 1) from 0 to u.
fn ramp_area(u: f32) -> f32 {
    if u <= 0.0 {
        return 0.0;
    }
    if u >= 1.0 {
        return u - 0.5;
    }
    return 0.5 * u * u;
}

// The signed area segment `a b` (relative to the pixel centre) sweeps over
// the unit pixel towards +x: its share of the box-filtered winding. Exact
// for a line: the part of the pixel left of the segment, integrated over
// the rows it crosses.
fn segment_area(a: vec2<f32>, b: vec2<f32>) -> f32 {
    let dy = b.y - a.y;
    if dy == 0.0 {
        return 0.0;
    }
    let t0 = clamp((-0.5 - a.y) / dy, 0.0, 1.0);
    let t1 = clamp((0.5 - a.y) / dy, 0.0, 1.0);
    let lo = min(t0, t1);
    let hi = max(t0, t1);
    let rows = (hi - lo) * dy;
    let ua = a.x + (b.x - a.x) * lo + 0.5;
    let ub = a.x + (b.x - a.x) * hi + 0.5;
    if min(ua, ub) >= 1.0 {
        return rows;
    }
    if max(ua, ub) <= 0.0 {
        return 0.0;
    }
    if abs(ub - ua) < 1e-4 {
        return rows * clamp(0.5 * (ua + ub), 0.0, 1.0);
    }
    return rows * (ramp_area(ub) - ramp_area(ua)) / (ub - ua);
}

// Non-zero box-filter coverage of the pixel at `r` by fill record `rec`.
fn path_fill(rec: u32, r: vec2<f32>) -> f32 {
    let cell = path_cell(rec, r);
    var acc = 0.0;
    for (var k = 0u; k < cell.y; k++) {
        let s = path_texel(cell.x + k);
        acc += segment_area(s.xy - r, s.zw - r);
    }
    return min(abs(acc), 1.0);
}

// Coverage of the pixel at `r` by stroke record `rec`: the union of
// capsules `hw` wide either side of its segments (local), measured as the
// distance to the nearest one, round joins and caps exact.
fn path_stroke(rec: u32, r: vec2<f32>, hw: f32, ji: vec4<f32>) -> f32 {
    let cell = path_cell(rec, r);
    let q = vec2<f32>(ji.x * r.x + ji.y * r.y, ji.z * r.x + ji.w * r.y);
    var best = 3.0e38;
    var near = vec2<f32>(0.0);
    var along = vec2<f32>(1.0, 0.0);
    for (var k = 0u; k < cell.y; k++) {
        let s = path_texel(cell.x + k);
        let a = s.xy - q;
        let ab = s.zw - s.xy;
        let t = clamp(-dot(a, ab) / max(dot(ab, ab), 1e-12), 0.0, 1.0);
        let c = a + ab * t;
        let d2 = dot(c, c);
        if d2 < best {
            best = d2;
            near = c;
            along = ab;
        }
    }
    if cell.y == 0u {
        return 0.0;
    }
    let d = sqrt(best);
    // The direction away from the nearest point; across the segment on it.
    var n = vec2<f32>(-along.y, along.x);
    if d > 1e-6 {
        n = near;
    }
    // The slab either side of the segment: its two edges, box-filtered.
    return clamp(edge(hw - d, n, ji) - edge(-hw - d, n, ji), 0.0, 1.0);
}

// The pixel at `r` under fill record `rec` blurred by a Gaussian of
// `sigma` device pixels: rows sampled across three deviations either side,
// each blurred across exactly from where it crosses the outline (after
// Evan Wallace's separable rounded-rect shadow).
fn path_blur(rec: u32, r: vec2<f32>, sigma: f32) -> f32 {
    let h = path_texel(rec);
    let n = path_texel(rec + 1u).xy;
    let rows = 16;
    var total = 0.0;
    var weights = 0.0;
    for (var k = 0; k < rows; k++) {
        let u = -3.0 + 6.0 * (f32(k) + 0.5) / f32(rows);
        let y = r.y + sigma * u;
        let w = exp(-0.5 * u * u);
        weights += w;
        let band = clamp(floor((y - h.y) * h.w), 0.0, n.y - 1.0);
        let e = path_texel(rec + 2u + u32(band));
        var across = 0.0;
        for (var j = 0u; j < u32(e.y); j++) {
            let s = path_texel(u32(e.x) + j);
            if (s.y <= y) != (s.w <= y) {
                let x = mix(s.x, s.z, (y - s.y) / (s.w - s.y));
                let rise = sign(s.w - s.y);
                across += rise * (0.5 + 0.5 * erf7((x - r.x) / (sigma * 1.4142135)));
            }
        }
        total += w * min(abs(across), 1.0);
    }
    return total / weights;
}

fn span(lo: f32, hi: f32, p: f32) -> f32 {
    return clamp(min(p + 0.5, hi) - max(p - 0.5, lo), 0.0, 1.0);
}

fn clip(p: vec2<f32>, v: Varying) -> f32 {
    var cov = span(v.dclip.x, v.dclip.z, p.x) * span(v.dclip.y, v.dclip.w, p.y);
    if v.pclip.w > 0.0 && cov > 0.0 {
        cov *= path_fill(u32(v.pclip.x), p - v.pclip.yz);
    }
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
    var q = v.q;
    if shape >= SHAPE_PATH {
        // The paint lives in local space.
        q = vec2<f32>(v.ji.x * v.q.x + v.ji.y * v.q.y, v.ji.z * v.q.x + v.ji.w * v.q.y);
    }
    var cov: f32;
    if shape == SHAPE_PATH {
        cov = path_fill(u32(v.kind.z), v.q);
    } else if shape == SHAPE_PATH_STROKE {
        cov = path_stroke(u32(v.kind.z), v.q, 0.5 * width, v.ji);
    } else if shape == SHAPE_PATH_BLUR {
        // Its quad is the blur's reach, sharp-edged, as `blurred_path`
        // clips it.
        cov = rounded(q, v.rect, 0.0, v.ji);
    } else if shape == SHAPE_GLYPH {
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
    if shape == SHAPE_PATH_BLUR {
        // The deviation in device pixels: local, scaled by the area scale.
        let sigma = v.c1.x / sqrt(abs(v.ji.x * v.ji.w - v.ji.y * v.ji.z));
        colour = v.c0 * path_blur(u32(v.kind.z), v.q, sigma);
    } else if shape == SHAPE_BLUR || shape == SHAPE_BLUR_INVERT {
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
