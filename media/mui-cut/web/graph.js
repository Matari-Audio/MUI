import { getp, insertKey, isKeys, keyAt, layer, numPaths, round, setp, snap } from './doc.js';
import { begin, changed, edit, end } from './edit.js';
import { $, C, S, cut } from './state.js';
import { LABEL, pan, playhead, short, startPan, ticks, timeAt, trackW, view, wheel, xAt } from './timeline.js';
import { refresh } from './transport.js';

// ---------- graph editor
const gr = $('#graph'), gctx = gr.getContext('2d');
let grDrag = null;
function gKeys() { const l = layer(), k = l && getp(l, S.prop); return isKeys(k) ? k : null; }
// Where a key's handles sit, in (time, value), clamped into their segment
// like the evaluator clamps them.
function handles(keys, i) {
  const k = keys[i], out = [];
  const next = keys[i + 1], prev = keys[i - 1];
  if (next && k.interp === 'bezier') {
    const [dt, dv] = k.out ?? [(next.t - k.t) / 3, 0];
    out.push({ side: 'out', t: Math.min(next.t, k.t + Math.max(0, dt)), v: k.v + dv });
  }
  if (prev && prev.interp === 'bezier') {
    const [dt, dv] = k.in ?? [-(k.t - prev.t) / 3, 0];
    out.push({ side: 'in', t: Math.max(prev.t, k.t + Math.min(0, dt)), v: k.v + dv });
  }
  return out;
}
export function drawGraph() {
  const sel0 = $('#graph-prop');
  const l = layer();
  const dpr = devicePixelRatio, w = gr.clientWidth, h = gr.clientHeight;
  if (gr.width !== Math.round(w * dpr) || gr.height !== Math.round(h * dpr)) { gr.width = Math.round(w * dpr); gr.height = Math.round(h * dpr); }
  const c = gctx; c.setTransform(dpr, 0, 0, dpr, 0, 0); c.clearRect(0, 0, w, h);
  c.font = '11px Inter, sans-serif'; c.textBaseline = 'middle';
  const nums = l ? numPaths(l) : [];
  if (sel0.dataset.for !== (l?.id ?? '') + S.prop + nums.length) {
    sel0.dataset.for = (l?.id ?? '') + S.prop + nums.length;
    sel0.replaceChildren(...nums.map(p => new Option(short(p) + (isKeys(getp(l, p)) ? ' ◆' : ''), p, false, p === S.prop)));
  }
  if (!l) { c.fillStyle = C.text; c.fillText('Select a layer to see its curves.', LABEL, h / 2); return; }
  const v = view(), n = Math.max(2, Math.round(trackW(w) / 2));
  const samples = cut.sample(S.si, l.id, S.prop, v.t0, v.t1, n);
  const keys = gKeys();
  if (!grDrag || grDrag.pan || !S.range) {
    let lo = Infinity, hi = -Infinity;
    const see = v => { lo = Math.min(lo, v); hi = Math.max(hi, v); };
    samples.forEach(see);
    keys?.forEach((k, i) => { see(k.v); handles(keys, i).forEach(hd => see(hd.v)); });
    if (!(hi - lo > 1e-6)) { lo -= 1; hi += 1; }
    const pad = (hi - lo) * 0.12; S.range = [lo - pad, hi + pad];
  }
  // The same time view as the timeline above it, so keys line up.
  const gx = time => xAt(time, w);
  const gy = v => 10 + (S.range[1] - v) / (S.range[1] - S.range[0]) * (h - 20);
  // Grid: the ruler's labelled times, and four value lines with their numbers.
  c.fillStyle = C.grid;
  for (const t of ticks(w)) if (t.major) c.fillRect(Math.round(gx(t.t)), 0, 1, h);
  for (let i = 0; i <= 4; i++) {
    const v = S.range[0] + (S.range[1] - S.range[0]) * i / 4;
    c.fillStyle = C.grid; c.fillRect(LABEL, gy(v), w - LABEL - 12, 1);
    c.fillStyle = C.gridText; c.fillText(Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(2), 8, gy(v));
  }
  c.beginPath();
  samples.forEach((s, i) => { const x = gx(v.t0 + i / (n - 1) * (v.t1 - v.t0)); i ? c.lineTo(x, gy(s)) : c.moveTo(x, gy(s)); });
  c.strokeStyle = C.curve; c.lineWidth = 1.5; c.stroke();
  if (!keys) { c.fillStyle = C.text; c.fillText(`${S.prop} is a plain value: press ◆ in the inspector to animate it.`, LABEL + 8, 14); }
  keys?.forEach((k, i) => {
    const picked = S.selKey && S.selKey.k === k;
    for (const hd of handles(keys, i)) {
      c.beginPath(); c.moveTo(gx(k.t), gy(k.v)); c.lineTo(gx(hd.t), gy(hd.v));
      c.strokeStyle = picked ? C.pickedLine : C.handleLine; c.lineWidth = 1; c.stroke();
      // Handles are round, keys square: shape, not colour, tells them apart.
      c.beginPath(); c.arc(gx(hd.t), gy(hd.v), picked ? 4 : 3, 0, 7); c.fillStyle = picked ? C.picked : C.handle; c.fill();
    }
  });
  keys?.forEach(k => {
    const picked = S.selKey && S.selKey.k === k;
    const s = picked ? 5 : 4;
    c.fillStyle = C.halo; c.fillRect(gx(k.t) - s - 1, gy(k.v) - s - 1, 2 * s + 2, 2 * s + 2);
    c.fillStyle = picked ? C.picked : C.key; c.fillRect(gx(k.t) - s, gy(k.v) - s, 2 * s, 2 * s);
  });
  c.save(); c.beginPath(); c.rect(LABEL, 0, w - LABEL, h); c.clip();
  playhead(c, gx(S.t), h);
  c.restore();
  gr._map = { gx, gy, tOf: x => timeAt(x, w), vOf: y => S.range[1] - (y - 10) / (h - 20) * (S.range[1] - S.range[0]) };
}
function grHit(e) {
  const r = gr.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top, m = gr._map, keys = gKeys();
  if (!m || !keys) return { x, y };
  for (let i = 0; i < keys.length; i++) {
    const hd = handles(keys, i).find(hd => Math.hypot(m.gx(hd.t) - x, m.gy(hd.v) - y) < 7);
    if (hd) return { x, y, i, side: hd.side };
  }
  const i = keys.findIndex(k => Math.hypot(m.gx(k.t) - x, m.gy(k.v) - y) < 7);
  return i >= 0 ? { x, y, i } : { x, y };
}
gr.onpointerdown = e => {
  if (e.button === 1) { e.preventDefault(); gr.setPointerCapture(e.pointerId); grDrag = startPan(e, gr); return; }
  const h = grHit(e), keys = gKeys();
  if (h.i === undefined) return;
  gr.setPointerCapture(e.pointerId);
  S.selKey = { l: layer(), p: S.prop, k: keys[h.i] };
  grDrag = { k: keys[h.i], side: h.side };
  begin(); S.need = true;
};
gr.onpointermove = e => {
  if (!grDrag) return;
  if (grDrag.pan) { pan(grDrag, e); return; }
  const m = gr._map, r = gr.getBoundingClientRect();
  const time = m.tOf(e.clientX - r.left), v = m.vOf(e.clientY - r.top);
  const keys = gKeys(), k = grDrag.k, i = keys.indexOf(k);
  if (!grDrag.side) {
    // A key moves between its neighbours, snapped to frames.
    const lo = i > 0 ? keys[i - 1].t + 1 / S.R.fps : 0, hi = i < keys.length - 1 ? keys[i + 1].t - 1 / S.R.fps : S.R.scenes[S.si].duration;
    k.t = snap(Math.max(lo, Math.min(hi, time)));
    k.v = round(v);
  } else {
    const out = grDrag.side === 'out';
    const dt = out ? Math.max(0, time - k.t) : Math.min(0, time - k.t), dv = v - k.v;
    k[out ? 'out' : 'in'] = [round(dt), round(dv)];
    // Smooth tangents: the other handle keeps its length and follows the
    // slope, unless Alt breaks them.
    const other = handles(keys, i).find(hd => hd.side !== grDrag.side);
    if (!e.altKey && other && Math.abs(dt) > 1e-6) {
      const len = Math.abs(other.t - k.t), slope = dv / dt;
      k[out ? 'in' : 'out'] = [round(out ? -len : len), round((out ? -len : len) * slope)];
    }
  }
  changed();
};
gr.onpointerup = () => { const was = grDrag; grDrag = null; if (was && !was.pan) end(); };
gr.addEventListener('wheel', e => wheel(e, gr), { passive: false });
gr.ondblclick = e => {
  const l = layer(), m = gr._map;
  if (!l || !m) return;
  const r = gr.getBoundingClientRect();
  const time = snap(Math.max(0, Math.min(S.R.scenes[S.si].duration, m.tOf(e.clientX - r.left))));
  const v = round(m.vOf(e.clientY - r.top));
  edit(() => {
    if (!isKeys(getp(l, S.prop))) setp(l, S.prop, []);
    const keys = getp(l, S.prop), i = keyAt(keys, time);
    if (i >= 0) keys[i].v = v; else S.selKey = { l, p: S.prop, k: insertKey(keys, { t: time, v, interp: 'bezier' }) };
  });
};
$('#graph-prop').onchange = e => { S.prop = e.target.value; S.selKey = null; S.range = null; refresh(); };
document.querySelectorAll('[data-interp]').forEach(b => b.onclick = () => { if (S.selKey) edit(() => { S.selKey.k.interp = b.dataset.interp; }); });
$('#reset-handles').onclick = () => { if (S.selKey) edit(() => { delete S.selKey.k.in; delete S.selKey.k.out; }); };

