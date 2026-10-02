// ---------- transform gizmo: scale handles on the selection's box (its
// own turned outline for one layer, the bounds of all for several) and a
// rotate handle above it. A drag works in project pixels and writes each
// layer's local x, y, rotation, scale (or width and height) at the
// playhead, through its parent's evaluated transform, so parented layers
// stay put under their parents.
import { now, roots, round, scene, selectedLayers, setValue } from './doc.js';
import { C, S, cut } from './state.js';

const SIZED = ['rect', 'ellipse', 'image', 'duplicator'];
const rad = d => d * Math.PI / 180;
const rot = (a, [x, y]) => [x * Math.cos(a) - y * Math.sin(a), x * Math.sin(a) + y * Math.cos(a)];
// The layers a gizmo acts on: selected whole layers no selected ancestor
// carries, in a 2D scene.
const targets = () => scene()?.mode === '3d' ? [] : roots(selectedLayers());

// The box: four corners (top left, top right, bottom right, bottom left
// in its own turn), its angle, and the pivot a rotation turns about (one
// layer's own position, else the box's middle).
export function gizmoBox() {
  const ls = targets();
  const qs = selectedLayers().map(l => S.quads.find(q => q.id === l.id)).filter(Boolean);
  if (!ls.length || !qs.length) return null;
  if (qs.length === 1 && ls.length === 1) {
    const pts = qs[0].pts, a = Math.atan2(pts[1][1] - pts[0][1], pts[1][0] - pts[0][0]);
    return { pts, a, one: ls[0] };
  }
  const xs = qs.flatMap(q => q.pts.map(p => p[0])), ys = qs.flatMap(q => q.pts.map(p => p[1]));
  const [x0, x1, y0, y1] = [Math.min(...xs), Math.max(...xs), Math.min(...ys), Math.max(...ys)];
  return { pts: [[x0, y0], [x1, y0], [x1, y1], [x0, y1]], a: 0 };
}
const lerp = ([ax, ay], [bx, by], k) => [ax + (bx - ax) * k, ay + (by - ay) * k];
// A box point by its place in the box, (0, 0) its top left, (1, 1) its
// bottom right.
const boxAt = (b, u, v) => lerp(lerp(b.pts[0], b.pts[1], u), lerp(b.pts[3], b.pts[2], u), v);
// Handles: corners and edge middles (u, v), and the rotate knob `px`
// project pixels above the top edge.
function handles(b, px) {
  const hs = [[0, 0], [1, 0], [1, 1], [0, 1], [0.5, 0], [1, 0.5], [0.5, 1], [0, 0.5]].map(([u, v]) => ({ u, v, at: boxAt(b, u, v) }));
  const top = boxAt(b, 0.5, 0), up = rot(b.a, [0, -px * 24]);
  hs.push({ turn: true, at: [top[0] + up[0], top[1] + up[1]], from: top });
  return hs;
}
// The handle under project point `p` (`px`: project pixels per CSS pixel).
export function gizmoHit(p, px) {
  const b = gizmoBox();
  if (!b) return null;
  return handles(b, px).find(h => Math.hypot(h.at[0] - p[0], h.at[1] - p[1]) < 7 * px) ?? null;
}
// The cursor over a handle: by its direction on screen.
export function gizmoCursor(h, b = gizmoBox()) {
  if (h.turn) return 'grab';
  const deg = ((Math.atan2(h.v - 0.5, h.u - 0.5) + b.a) * 180 / Math.PI + 360) % 180;
  return ['ew-resize', 'nwse-resize', 'ns-resize', 'nesw-resize'][Math.round(deg / 45) % 4];
}

// A gesture on handle `h` from project point `p`: every target's world
// and local transform where it starts.
export function startGizmo(h, p) {
  const b = gizmoBox(), frame = JSON.parse(cut.frame(S.si, S.t) || 'null');
  const world = id => frame?.layers.find(d => d.id === id);
  const items = targets().map(l => {
    const w = world(l.id) ?? { x: now(l, 'x'), y: now(l, 'y'), rotation: now(l, 'rotation'), scale: now(l, 'scale') };
    const pw = l.parent ? world(l.parent) : null;
    const it = { l, w: [w.x, w.y], wr: w.rotation, parent: pw && { x: pw.x, y: pw.y, a: rad(pw.rotation), k: pw.scale || 1e-9 },
      rot0: now(l, 'rotation'), scale0: now(l, 'scale') };
    if (SIZED.includes(l.kind)) Object.assign(it, { w0: now(l, 'width'), h0: now(l, 'height') });
    return it;
  });
  const W = Math.hypot(b.pts[1][0] - b.pts[0][0], b.pts[1][1] - b.pts[0][1]);
  const H = Math.hypot(b.pts[3][0] - b.pts[0][0], b.pts[3][1] - b.pts[0][1]);
  const pivot = b.one ? items[0].w : boxAt(b, 0.5, 0.5);
  return { h, p0: p, box: b, W, H, items, pivot };
}
// A world point back into the item's parent's frame: its local x, y.
function place(it, [x, y]) {
  if (!it.parent) return [x, y];
  const { x: px, y: py, a, k } = it.parent;
  const [lx, ly] = rot(-a, [(x - px) / k, (y - py) / k]);
  return [lx, ly];
}
function setPos(it, w) {
  const [x, y] = place(it, w);
  setValue(it.l, 'x', round(x)); setValue(it.l, 'y', round(y));
}
// Pointer at project point `p`: `shift` keeps proportions (and snaps a
// turn to 15°), `alt` scales about the middle.
export function moveGizmo(g, p, { shift, alt }) {
  const { h, box: b, items } = g;
  if (h.turn) {
    const [cx, cy] = g.pivot;
    let d = Math.atan2(p[1] - cy, p[0] - cx) - Math.atan2(g.p0[1] - cy, g.p0[0] - cx);
    if (shift) d = rad(15) * Math.round(d / rad(15));
    for (const it of items) {
      const [x, y] = rot(d, [it.w[0] - cx, it.w[1] - cy]);
      if (!b.one) setPos(it, [cx + x, cy + y]);
      setValue(it.l, 'rotation', round(it.rot0 + d * 180 / Math.PI));
    }
    return;
  }
  // In the box's own frame, from its top left corner.
  const o = b.pts[0], local = q => rot(-b.a, [q[0] - o[0], q[1] - o[1]]);
  const au = alt ? 0.5 : 1 - h.u, av = alt ? 0.5 : 1 - h.v;
  const A = [au * g.W, av * g.H], q = local(p), q0 = local(g.p0);
  const factor = (i, on) => {
    const span = q0[i] - A[i];
    return !on || Math.abs(span) < 1e-6 ? 1 : Math.max(0.01, (q[i] - A[i]) / span);
  };
  let fx = factor(0, h.u !== 0.5), fy = factor(1, h.v !== 0.5);
  const corner = h.u !== 0.5 && h.v !== 0.5;
  if (shift) { const f = !corner ? (h.u !== 0.5 ? fx : fy) : Math.abs(fx - 1) > Math.abs(fy - 1) ? fx : fy; fx = fy = f; }
  // A layer that has no width and height to stretch scales evenly.
  // ponytail: no scale_x/scale_y in the engine, so text, SVG and the like
  // cannot stretch one way; add them there to stretch everything.
  const even = corner ? Math.sqrt(fx * fy) : h.u !== 0.5 ? fx : fy;
  if (b.one && items[0].w0 === undefined) fx = fy = even;
  for (const it of items) {
    const w = local(it.w);
    const nw = rot(b.a, [A[0] + (w[0] - A[0]) * fx, A[1] + (w[1] - A[1]) * fy]);
    setPos(it, [o[0] + nw[0], o[1] + nw[1]]);
    if (it.w0 !== undefined) {
      // Its own axes against the box's: a quarter turn swaps them.
      const turn = (((it.wr - b.a * 180 / Math.PI) % 180) + 180) % 180;
      const [sx, sy] = turn < 10 || turn > 170 ? [fx, fy] : Math.abs(turn - 90) < 10 ? [fy, fx] : [even, even];
      setValue(it.l, 'width', round(it.w0 * sx)); setValue(it.l, 'height', round(it.h0 * sy));
    } else setValue(it.l, 'scale', round(it.scale0 * even));
  }
}

// The box, its handles and the rotate knob, over the selection outlines.
export function drawGizmo(c, k, dpr, px) {
  const b = gizmoBox();
  if (!b) return;
  const P = ([x, y]) => [x * k, y * k];
  c.beginPath(); b.pts.forEach((pt, i) => c[i ? 'lineTo' : 'moveTo'](...P(pt))); c.closePath();
  c.lineWidth = dpr; c.strokeStyle = C.sel; c.setLineDash(b.one ? [] : [4 * dpr, 3 * dpr]); c.stroke(); c.setLineDash([]);
  for (const h of handles(b, px)) {
    const [x, y] = P(h.at);
    if (h.turn) {
      c.beginPath(); c.moveTo(...P(h.from)); c.lineTo(x, y); c.lineWidth = dpr; c.strokeStyle = C.sel; c.stroke();
      c.beginPath(); c.arc(x, y, 4.5 * dpr, 0, 7); c.fillStyle = C.halo; c.fill();
      c.beginPath(); c.arc(x, y, 3.5 * dpr, 0, 7); c.fillStyle = C.sel; c.fill();
    } else {
      c.fillStyle = C.halo; c.fillRect(x - 4 * dpr, y - 4 * dpr, 8 * dpr, 8 * dpr);
      c.fillStyle = C.sel; c.fillRect(x - 3 * dpr, y - 3 * dpr, 6 * dpr, 6 * dpr);
    }
  }
}
