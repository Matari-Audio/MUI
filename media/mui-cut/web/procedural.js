// ---------- effector gizmo: each animator effector (`falloff`) of the
// selected layer, drawn where it sits (in the layer's own space, through
// its evaluated transform): its field's outline, a centre to drag and a
// radius handle. A drag writes `animators.N.falloff.x`/`y`/`radius` at the
// playhead, so a keyed effector gets a key there.
import { layer, now, round, scene, setValue } from './doc.js';
import { C, S, cut } from './state.js';

const rot = (a, [x, y]) => [x * Math.cos(a) - y * Math.sin(a), x * Math.sin(a) + y * Math.cos(a)];

// The selected layer's effectors at the playhead: `{ i, shape, c, r, soft,
// a, k, w }` with the centre `c` and radius `r` in project pixels, the
// layer's world turn `a` and scale `k`, and its world pivot `w`.
function effectors() {
  const l = layer();
  if (!l?.animators?.some(a => a.falloff) || scene()?.mode === '3d') return [];
  const frame = JSON.parse(cut.frame(S.si, S.t) || 'null');
  const d = frame?.layers.find(o => o.id === l.id);
  if (!d) return [];
  const a = d.rotation * Math.PI / 180, k = d.scale || 1e-9;
  return l.animators.flatMap((an, i) => {
    if (!an.falloff) return [];
    const v = n => now(l, `animators.${i}.falloff.${n}`);
    const [ox, oy] = rot(a, [v('x') * k, v('y') * k]);
    return [{ i, l, shape: an.falloff.shape ?? 'sphere', c: [d.x + ox, d.y + oy], r: v('radius') * k, soft: v('softness') * k, a, k, w: [d.x, d.y] }];
  });
}
// The radius handle: on the field's edge, along the layer's x (a wall
// has none: its radius is unused).
const knob = e => { const [dx, dy] = rot(e.a, [e.r, 0]); return [e.c[0] + dx, e.c[1] + dy]; };

export function drawEffectors(c, k, dpr) {
  for (const e of effectors()) {
    const P = ([x, y]) => [x * k, y * k];
    const [cx, cy] = P(e.c);
    const ring = (r, dash) => {
      c.beginPath();
      if (e.shape === 'sphere') c.arc(cx, cy, Math.max(0, r) * k, 0, 7);
      else if (e.shape === 'box') {
        const pts = [[-1, -1], [1, -1], [1, 1], [-1, 1]].map(([u, v]) => P([e.c[0], e.c[1]].map((o, j) => o + rot(e.a, [u * r, v * r])[j])));
        pts.forEach((p, j) => c[j ? 'lineTo' : 'moveTo'](...p)); c.closePath();
      } else {
        // A wall through the centre (and where the fade ends), across the frame.
        const off = rot(e.a, [r - e.r, 0]), up = rot(e.a, [0, 4000]);
        c.moveTo(...P([e.c[0] + off[0] - up[0], e.c[1] + off[1] - up[1]]));
        c.lineTo(...P([e.c[0] + off[0] + up[0], e.c[1] + off[1] + up[1]]));
      }
      c.setLineDash(dash); c.lineWidth = (dash.length ? 1 : 1.5) * dpr;
      c.strokeStyle = C.halo; c.lineWidth += 2 * dpr; c.stroke();
      c.lineWidth -= 2 * dpr; c.strokeStyle = C.guide; c.stroke(); c.setLineDash([]);
    };
    ring(e.r, []);
    if (e.soft > 0) ring(e.r + e.soft, [4 * dpr, 4 * dpr]);
    // The centre: a cross in a ring; the radius knob: a dot.
    c.beginPath(); c.arc(cx, cy, 6 * dpr, 0, 7); c.lineWidth = dpr; c.strokeStyle = C.sel; c.stroke();
    c.beginPath(); c.moveTo(cx - 9 * dpr, cy); c.lineTo(cx + 9 * dpr, cy); c.moveTo(cx, cy - 9 * dpr); c.lineTo(cx, cy + 9 * dpr); c.stroke();
    if (e.shape !== 'linear') {
      const [hx, hy] = P(knob(e));
      c.beginPath(); c.arc(hx, hy, 4.5 * dpr, 0, 7); c.fillStyle = C.halo; c.fill();
      c.beginPath(); c.arc(hx, hy, 3.5 * dpr, 0, 7); c.fillStyle = C.sel; c.fill();
    }
  }
}
// The effector handle under project point `p` (`px`: project pixels per
// CSS pixel): `{ e, radius }`, the knob before the centre.
export function effectorHit(p, px) {
  const near = q => Math.hypot(q[0] - p[0], q[1] - p[1]) < 8 * px;
  for (const e of effectors()) {
    if (e.shape !== 'linear' && near(knob(e))) return { e, radius: true };
    if (near(e.c)) return { e, radius: false, from: p, c0: e.c };
  }
  return null;
}
// Pointer at `p` during a drag of handle `h`.
export function moveEffector(h, p) {
  const { e } = h, path = n => `animators.${e.i}.falloff.${n}`;
  if (h.radius) {
    const r = Math.hypot(p[0] - e.c[0], p[1] - e.c[1]) / e.k;
    setValue(e.l, path('radius'), round(r));
    return;
  }
  const w = [h.c0[0] + p[0] - h.from[0], h.c0[1] + p[1] - h.from[1]];
  const [x, y] = rot(-e.a, [(w[0] - e.w[0]) / e.k, (w[1] - e.w[1]) / e.k]);
  setValue(e.l, path('x'), round(x)); setValue(e.l, path('y'), round(y));
}
// For the e2e: the selected layer's effector centres and radius knobs, in
// project pixels.
globalThis.cutEffectors = () => effectors().map(e => ({ i: e.i, c: e.c, knob: knob(e), r: e.r }));
