// ---------- snapping: while a move or a scale drags, the selection's
// edges and middle catch on other layers' edges and middles and the
// canvas's, within a few screen pixels, and the overlay draws the guide
// lines they caught on. Ctrl held, or Snap off, drags freely.
import { scene } from './doc.js';
import { $, C, S } from './state.js';

let on = true;
$('#snap').onclick = () => { on = !on; $('#snap').setAttribute('aria-pressed', on); };
export const snapOn = () => on;
// The lines caught on last: project x's and y's.
export const guides = { x: [], y: [] };
const box = pts => {
  const xs = pts.map(p => p[0]), ys = pts.map(p => p[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
};
// The bounds of the quads of `ids` (layers or parts), or null.
export function boundsOf(ids) {
  const qs = S.quads.filter(q => ids.includes(q.id));
  return qs.length ? box(qs.flatMap(q => q.pts)) : null;
}
// What a drag of `ids` can catch on, `px` project pixels per screen pixel:
// every other outline (not theirs, their parts' or their children's) and
// the canvas, as x and y lines.
export function targets(ids, px) {
  const by = new Map(scene().layers.map(l => [l.id, l]));
  const moving = id => {
    const lid = id.split('#')[0];
    if (ids.includes(id) || ids.includes(lid)) return true;
    for (let p = by.get(lid)?.parent, n = 0; p && n < 64; p = by.get(p)?.parent, n++) if (ids.includes(p)) return true;
    return false;
  };
  const [W, H] = S.R.size, xs = [0, W / 2, W], ys = [0, H / 2, H];
  for (const q of S.quads) {
    if (moving(q.id)) continue;
    const [x0, y0, x1, y1] = box(q.pts);
    xs.push(x0, (x0 + x1) / 2, x1); ys.push(y0, (y0 + y1) / 2, y1);
  }
  return { xs, ys, reach: 6 * px };
}
// The nearest catch for any of `vals` among `lines`: [offset, line] or null.
function nearest(vals, lines, reach) {
  let best = null;
  for (const v of vals) for (const l of lines) if (Math.abs(l - v) <= reach && (!best || Math.abs(l - v) < Math.abs(best[0]))) best = [l - v, l];
  return best;
}
// A move by `d` of bounds `b` [x0, y0, x1, y1]: the delta snapped.
export function snapMove(t, b, d, free) {
  guides.x = []; guides.y = [];
  if (!on || free || !t || !b) return d;
  const out = [...d];
  const cx = nearest([b[0] + d[0], (b[0] + b[2]) / 2 + d[0], b[2] + d[0]], t.xs, t.reach);
  const cy = nearest([b[1] + d[1], (b[1] + b[3]) / 2 + d[1], b[3] + d[1]], t.ys, t.reach);
  if (cx) { out[0] += cx[0]; guides.x = [cx[1]]; }
  if (cy) { out[1] += cy[0]; guides.y = [cy[1]]; }
  return out;
}
// A point an edge follows (a scale handle): each axis that `axes` names
// snapped, where the edge is at `at` for the pointer at `p`.
export function snapPoint(t, p, at, axes, free) {
  guides.x = []; guides.y = [];
  if (!on || free || !t) return p;
  const out = [...p];
  for (const i of [0, 1]) {
    if (!axes[i]) continue;
    const c = nearest([at[i]], i ? t.ys : t.xs, t.reach);
    if (c) { out[i] += c[0]; (i ? guides.y : guides.x).push(c[1]); }
  }
  return out;
}
export function clearGuides() { guides.x = []; guides.y = []; }
// Dashed hairlines across the frame, with a dark halo.
export function drawGuides(c, k, dpr) {
  const [W, H] = S.R.size;
  c.setLineDash([5 * dpr, 4 * dpr]);
  for (const [w, color] of [[3, C.halo], [1, C.guide]]) {
    c.lineWidth = w * dpr; c.strokeStyle = color; c.beginPath();
    for (const x of guides.x) { c.moveTo(x * k, 0); c.lineTo(x * k, H * k); }
    for (const y of guides.y) { c.moveTo(0, y * k); c.lineTo(W * k, y * k); }
    c.stroke();
  }
  c.setLineDash([]);
}
