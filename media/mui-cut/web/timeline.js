import { getp, isKeys, keyPaths, numPaths, scene, snap } from './doc.js';
import { begin, changed, end } from './edit.js';
import { select } from './lists.js';
import { drawSound } from './sound.js';
import { $, C, KIND_ICON, S } from './state.js';
import { refresh, seek } from './transport.js';

// ---------- timeline
const tl = $('#timeline'), tctx = tl.getContext('2d');
export const LABEL = 150, RULER = 22, ROW = 20;
let tlRows = [], tlDrag = null;
export const tlX = time => LABEL + time / S.R.scenes[S.si].duration * (tl.clientWidth - LABEL - 12);
export const tlT = x => Math.max(0, Math.min(S.R.scenes[S.si].duration, (x - LABEL) / (tl.clientWidth - LABEL - 12) * S.R.scenes[S.si].duration));
// `animators.0.offset` as `a1 offset`, to fit the label column.
export const short = p => p.replace(/^animators\.(\d+)\./, (_, i) => `a${+i + 1} `).replace(/^deformers\.(\d+)\./, (_, i) => `d${+i + 1} `)
  .replace(/^parts\.([^.]+)\./, '$1 ').replace(/^params\.(\d+)\.value$/, (_, i) => `param ${+i + 1}`);
function rows() {
  const out = [];
  for (const l of [...scene().layers].reverse()) {
    out.push({ l });
    if (l.id === S.sel) for (const p of keyPaths(l)) if (isKeys(getp(l, p))) out.push({ l, p });
  }
  return out;
}
// The keys a row shows: one prop's, or every key of the layer.
function rowKeys(r) {
  const ps = r.p ? [r.p] : keyPaths(r.l).filter(p => isKeys(getp(r.l, p)));
  return ps.flatMap(p => getp(r.l, p).map(k => ({ l: r.l, p, k })));
}
function diamond(c, x, y, s, fill, ring = false) {
  c.beginPath(); c.moveTo(x, y - s); c.lineTo(x + s, y); c.lineTo(x, y + s); c.lineTo(x - s, y); c.closePath();
  c.fillStyle = fill; c.fill();
  if (ring) { c.lineWidth = 1; c.strokeStyle = C.halo; c.stroke(); }
}
// Playhead: a hairline with a flag on the ruler, like Blender's.
export function playhead(c, x, h) {
  c.fillStyle = C.playhead; c.fillRect(Math.round(x), 0, 1, h);
  c.beginPath(); c.moveTo(x - 5, 0); c.lineTo(x + 6, 0); c.lineTo(x + 6, 6); c.lineTo(x + .5, 11); c.lineTo(x - 5, 6); c.closePath(); c.fill();
}
export function drawTimeline() {
  tlRows = rows();
  const dpr = devicePixelRatio, w = tl.clientWidth, h = RULER + tlRows.length * ROW + 8;
  tl.style.height = h + 'px';
  if (tl.width !== Math.round(w * dpr) || tl.height !== Math.round(h * dpr)) { tl.width = Math.round(w * dpr); tl.height = Math.round(h * dpr); }
  const c = tctx; c.setTransform(dpr, 0, 0, dpr, 0, 0); c.clearRect(0, 0, w, h);
  c.font = '11px Inter, sans-serif'; c.textBaseline = 'middle';
  const dur = S.R.scenes[S.si].duration;
  // Ruler: a tick every tenth, a label every second.
  for (let i = 0; i <= Math.round(dur * 10); i++) {
    const x = tlX(i / 10), whole = i % 10 === 0;
    c.fillStyle = whole ? C.text : C.tick;
    c.fillRect(x, whole ? 4 : 14, 1, whole ? 18 : 8);
    if (whole) c.fillText(`${i / 10}s`, x + 3, 9);
  }
  tlRows.forEach((r, i) => {
    const y = RULER + i * ROW;
    const on = r.l.id === S.sel && (r.p ? r.p === S.prop : false);
    c.fillStyle = on ? C.rowOn : i % 2 ? C.rowAlt : C.row; c.fillRect(0, y, w, ROW);
    if (on) { c.fillStyle = C.textOn; c.fillRect(0, y, 2, ROW); }
    c.fillStyle = r.l.id === S.sel && (!r.p || on) ? C.textOn : C.text;
    if (!r.p) drawSound(c, r.l, y);
    c.fillText(r.p ? `   ${short(r.p)}` : `${KIND_ICON[r.l.kind] ?? ''} ${r.l.name || r.l.id}`, 8, y + ROW / 2);
    for (const key of rowKeys(r)) {
      const picked = S.selKey && S.selKey.k === key.k;
      diamond(c, tlX(key.k.t), y + ROW / 2, picked ? 6 : r.p ? 5 : 4, picked ? C.picked : r.p ? C.key : C.layerKey, picked);
    }
  });
  playhead(c, tlX(S.t), h);
}
function tlHit(e) {
  const r = tl.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
  const row = y < RULER ? null : tlRows[Math.floor((y - RULER) / ROW)] ?? null;
  const key = row && x > LABEL ? rowKeys(row).find(k => Math.abs(tlX(k.k.t) - x) < 6) : null;
  return { x, y, row, key };
}
tl.onpointerdown = e => {
  const h = tlHit(e);
  tl.setPointerCapture(e.pointerId);
  if (h.row && h.x < LABEL) { select(h.row.l.id); if (h.row.p && numPaths(h.row.l).includes(h.row.p)) S.prop = h.row.p; refresh(); return; }
  if (h.key) {
    // A layer-row diamond carries every key of the layer at that time.
    const group = h.row.p ? [h.key] : rowKeys(h.row).filter(k => Math.abs(k.k.t - h.key.k.t) < 1e-9);
    select(h.row.l.id);
    S.selKey = h.key; if (numPaths(h.row.l).includes(h.key.p)) S.prop = h.key.p;
    tlDrag = { group, t0: h.key.k.t, x0: h.x };
    begin(); refresh(); return;
  }
  tlDrag = { scrub: true }; seek(snap(tlT(h.x)));
};
tl.onpointermove = e => {
  if (!tlDrag) return;
  const h = tlHit(e);
  if (tlDrag.scrub) { seek(snap(tlT(h.x))); return; }
  const nt = snap(tlT(tlX(tlDrag.t0) + h.x - tlDrag.x0));
  for (const { l, p, k } of tlDrag.group) { k.t = nt; getp(l, p).sort((a, b) => a.t - b.t); }
  changed();
};
tl.onpointerup = () => { if (tlDrag && !tlDrag.scrub) end(); tlDrag = null; };

