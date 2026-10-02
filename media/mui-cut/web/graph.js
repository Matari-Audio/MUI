import { getp, insertKey, isKeys, keyAt, layer, numPaths, round, scene, setp, snap } from './doc.js';
import { begin, changed, edit, end } from './edit.js';
import { picking } from './lists.js';
import { $, C, S, cut } from './state.js';
import { LABEL, pan, playhead, short, startPan, ticks, timeAt, trackW, view, wheel, xAt } from './timeline.js';
import { refresh } from './transport.js';

// ---------- graph editor: the graphed property of the selected layer,
// and the curve of every key list a picked key is on, each in its own
// colour. One curve keeps its value scale (the numbers on the left);
// several are each fitted to the height, so their shapes compare.
const gr = $('#graph'), gctx = gr.getContext('2d');
let grDrag = null, shown = [];
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
// The curves to show, `{ l, p, keys }` (keys null for a plain value), the
// primary first; only numbers graph.
function curves() {
  const out = [], seen = new Set();
  const add = (l, p) => {
    const v = getp(l, p), id = `${l.id}\0${p}`;
    if (seen.has(id) || (isKeys(v) ? typeof v[0]?.v !== 'number' : typeof v === 'string')) return;
    seen.add(id); out.push({ l, p, keys: isKeys(v) ? v : null });
  };
  const l = layer();
  if (l) add(l, S.prop);
  for (const s of S.selKeys) if (scene().layers.includes(s.l)) add(s.l, s.p);
  return out.slice(0, C.curves.length);
}
const picked = k => S.selKeys.some(s => s.k === k);
function samples(cv, v, n) {
  try { return cut.sample(S.si, cv.l.id, cv.p, v.t0, v.t1, n); } catch { return []; }
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
  const v = view(), n = Math.max(2, Math.round(trackW(w) / 2)), was = shown;
  shown = curves();
  if (!shown.length) { c.fillStyle = C.text; c.fillText('Select a layer to see its curves.', LABEL, h / 2); gr._map = null; return; }
  const many = shown.length > 1;
  for (const [ci, cv] of shown.entries()) {
    cv.color = many ? C.curves[ci] : C.curve;
    cv.samples = samples(cv, v, n);
    // Value range: what is in view, held still while a key or handle drags.
    const old = was.find(o => o.l === cv.l && o.p === cv.p)?.range;
    if (grDrag && !grDrag.pan && old) { cv.range = old; continue; }
    let lo = Infinity, hi = -Infinity;
    const see = x => { lo = Math.min(lo, x); hi = Math.max(hi, x); };
    cv.samples.forEach(see);
    cv.keys?.forEach((k, i) => { if (k.t >= v.t0 && k.t <= v.t1) { see(k.v); handles(cv.keys, i).forEach(hd => see(hd.v)); } });
    if (!isFinite(lo)) { const x = cv.keys?.[0]?.v ?? 0; lo = hi = x; }
    if (!(hi - lo > 1e-6)) { lo -= 1; hi += 1; }
    const pad = (hi - lo) * 0.12; cv.range = [lo - pad, hi + pad];
  }
  for (const cv of shown) {
    const [lo, hi] = cv.range;
    cv.gy = x => 10 + (hi - x) / (hi - lo) * (h - 20);
    cv.vOf = y => hi - (y - 10) / (h - 20) * (hi - lo);
  }
  const main = shown[0], gx = time => xAt(time, w);
  S.range = main.range;
  // Grid: the ruler's labelled times, and four value lines with the
  // primary curve's numbers.
  c.fillStyle = C.grid;
  for (const t of ticks(w)) if (t.major) c.fillRect(Math.round(gx(t.t)), 0, 1, h);
  for (let i = 0; i <= 4; i++) {
    const x = main.range[0] + (main.range[1] - main.range[0]) * i / 4;
    c.fillStyle = C.grid; c.fillRect(LABEL, main.gy(x), w - LABEL - 12, 1);
    c.fillStyle = many ? main.color : C.gridText; c.fillText(Math.abs(x) >= 100 ? x.toFixed(0) : x.toFixed(2), 8, main.gy(x));
  }
  c.save(); c.beginPath(); c.rect(LABEL, 0, w - LABEL, h); c.clip();
  // Back to front, so the primary is on top.
  for (const cv of [...shown].reverse()) {
    c.beginPath();
    if (cv.samples.length) cv.samples.forEach((s, i) => { const x = gx(v.t0 + i / (n - 1) * (v.t1 - v.t0)); i ? c.lineTo(x, cv.gy(s)) : c.moveTo(x, cv.gy(s)); });
    else cv.keys?.forEach((k, i) => i ? c.lineTo(gx(k.t), cv.gy(k.v)) : c.moveTo(gx(k.t), cv.gy(k.v)));
    c.strokeStyle = cv.color; c.lineWidth = cv === main ? 1.5 : 1.25; c.stroke();
    // Handles: every key's with one curve, only the picked keys' with several.
    cv.keys?.forEach((k, i) => {
      const on = picked(k);
      if (many && !on) return;
      for (const hd of handles(cv.keys, i)) {
        c.beginPath(); c.moveTo(gx(k.t), cv.gy(k.v)); c.lineTo(gx(hd.t), cv.gy(hd.v));
        c.strokeStyle = on ? C.pickedLine : C.handleLine; c.lineWidth = 1; c.stroke();
        // Handles are round, keys square: shape, not colour, tells them apart.
        c.beginPath(); c.arc(gx(hd.t), cv.gy(hd.v), on ? 4 : 3, 0, 7); c.fillStyle = on ? C.picked : C.handle; c.fill();
      }
    });
    cv.keys?.forEach(k => {
      const on = picked(k), s = on ? 5 : 4;
      c.fillStyle = C.halo; c.fillRect(gx(k.t) - s - 1, cv.gy(k.v) - s - 1, 2 * s + 2, 2 * s + 2);
      c.fillStyle = on ? C.picked : many ? cv.color : C.key; c.fillRect(gx(k.t) - s, cv.gy(k.v) - s, 2 * s, 2 * s);
    });
  }
  playhead(c, gx(S.t), h);
  c.restore();
  if (!main.keys && !many) { c.fillStyle = C.text; c.fillText(`${S.prop} is a plain value: press ◆ in the inspector to animate it.`, LABEL + 8, 14); }
  // A legend when there are several: each curve's colour and name.
  if (many) {
    shown.forEach((cv, i) => {
      const y = 14 + i * 15;
      c.fillStyle = cv.color; c.fillRect(LABEL + 10, y - 1, 12, 2);
      c.fillStyle = C.textOn; c.fillText(`${cv.l.name || cv.l.id} · ${short(cv.p)}`, LABEL + 28, y);
    });
  }
  gr._map = { gx, gy: main.gy, tOf: x => timeAt(x, w), vOf: main.vOf };
}
function grHit(e) {
  const r = gr.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top, m = gr._map;
  if (!m) return { x, y };
  const many = shown.length > 1;
  for (const cv of shown) for (let i = 0; i < (cv.keys?.length ?? 0); i++) {
    if (many && !picked(cv.keys[i])) continue;
    const hd = handles(cv.keys, i).find(hd => Math.hypot(m.gx(hd.t) - x, cv.gy(hd.v) - y) < 7);
    if (hd) return { x, y, cv, i, side: hd.side };
  }
  for (const cv of shown) {
    const i = cv.keys?.findIndex(k => Math.hypot(m.gx(k.t) - x, cv.gy(k.v) - y) < 7) ?? -1;
    if (i >= 0) return { x, y, cv, i };
  }
  return { x, y };
}
gr.onpointerdown = e => {
  if (e.button === 1) { e.preventDefault(); gr.setPointerCapture(e.pointerId); grDrag = startPan(e, gr); return; }
  const h = grHit(e);
  if (!h.cv) return;
  gr.setPointerCapture(e.pointerId);
  const k = h.cv.keys[h.i], ref = { l: h.cv.l, p: h.cv.p, k };
  // A click picks the key; Shift/Ctrl toggles it in the picked set. Picked
  // keys keep the set (the one clicked becomes the primary).
  const rest = S.selKeys.filter(s => s.k !== k);
  if (picking(e) && picked(k)) { S.selKeys = rest; S.need = true; return; }
  S.selKeys = picking(e) || picked(k) ? [...rest, ref] : [ref];
  grDrag = { cv: h.cv, k, side: h.side };
  begin(); S.need = true;
};
gr.onpointermove = e => {
  if (!grDrag) return;
  if (grDrag.pan) { pan(grDrag, e); return; }
  const r = gr.getBoundingClientRect(), cv = grDrag.cv;
  const time = gr._map.tOf(e.clientX - r.left), v = cv.vOf(e.clientY - r.top);
  const keys = getp(cv.l, cv.p), k = grDrag.k, i = keys.indexOf(k);
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
// Double-click adds a key on the graphed property.
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
// Hold / Linear / Bezier / Reset handles: every picked key.
document.querySelectorAll('[data-interp]').forEach(b => b.onclick = () => { if (S.selKeys.length) edit(() => { for (const s of S.selKeys) s.k.interp = b.dataset.interp; }); });
$('#reset-handles').onclick = () => { if (S.selKeys.length) edit(() => { for (const s of S.selKeys) { delete s.k.in; delete s.k.out; } }); };
