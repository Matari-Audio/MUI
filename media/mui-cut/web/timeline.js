import { getp, numPaths, scene, selectedLayers, tidy, tracks } from './doc.js';
import { begin, changed, edit, end } from './edit.js';
import { picking, refreshLists, select } from './lists.js';
import { snapOn } from './snap.js';
import { drawSound } from './sound.js';
import { $, C, KIND_ICON, S } from './state.js';
import { refresh, seek } from './transport.js';
import { isFolded, openComp, setFolded, treeRows } from './tree.js';

// ---------- timeline: drawn on one canvas the size of its panel; only
// the rows in sight are drawn, so a scene of hundreds of layers scrolls
// and scrubs at the display's rate. Rows follow the layer tree (folded
// branches left out); a selected layer opens onto a row per key list.
const tl = $('#timeline'), tctx = tl.getContext('2d'), wrap = $('#tl-wrap');
export const LABEL = 190, RULER = 24, ROW = 20;
const dur = () => S.R.scenes[S.si].duration;
const frame = () => 1 / S.R.fps;

// ---------- the time view, shared with the graph editor: `S.view.t0` at
// the track area's left edge, `t1` at its right, reset to the whole scene
// when the scene changes.
export const trackW = (w = tl.clientWidth) => Math.max(1, w - LABEL - 12);
export function view() {
  if (S.view.si !== S.si) Object.assign(S.view, { si: S.si, t0: 0, t1: dur() });
  return S.view;
}
export const xAt = (time, w) => { const v = view(); return LABEL + (time - v.t0) / (v.t1 - v.t0) * trackW(w); };
export const timeAt = (x, w) => { const v = view(); return v.t0 + (x - LABEL) / trackW(w) * (v.t1 - v.t0); };
export const tlX = time => xAt(time);
export const tlT = x => Math.max(0, Math.min(dur(), timeAt(x)));
// From four frames across to three scenes' length, a little past either end.
export function setView(t0, t1) {
  const d = dur(), span = Math.min(Math.max(t1 - t0, 4 * frame()), Math.max(d, 1) * 3), pad = span * 0.05;
  t0 = Math.max(-pad, Math.min(t0, Math.max(-pad, d + pad - span)));
  Object.assign(view(), { t0, t1: t0 + span });
  S.tlNeed = true;
}
export function zoomAt(time, k) { const v = view(); setView(time - (time - v.t0) * k, time + (v.t1 - time) * k); }
const panBy = dt => { const v = view(); setView(v.t0 + dt, v.t1 + dt); };
export const fitView = (t0 = 0, t1 = dur()) => { const pad = Math.max((t1 - t0) * 0.04, 2 * frame()); setView(t0 - pad, t1 + pad); };
// The wheel over a time axis: Ctrl (or a pinch) zooms about the pointer,
// Shift or a sideways swipe pans; a plain wheel goes to `scroll` (the
// timeline's rows), else pans too.
export function wheel(e, canvas, scroll) {
  e.preventDefault();
  const k = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? 400 : 1, dx = e.deltaX * k, dy = e.deltaY * k;
  const w = canvas.clientWidth, x = e.clientX - canvas.getBoundingClientRect().left, v = view();
  if (e.ctrlKey || e.metaKey) zoomAt(timeAt(x, w), Math.exp(dy * 0.002));
  else if (e.shiftKey || !scroll || Math.abs(dx) > Math.abs(dy)) panBy((e.shiftKey ? dy || dx : dx || dy) * (v.t1 - v.t0) / trackW(w));
  else scroll(dy);
}
// A middle-button drag pans the view (and, given `y`, scrolls).
export const startPan = (e, canvas, y = 0) => ({ pan: true, x0: e.clientX, y0: e.clientY, w: canvas.clientWidth, t0: view().t0, t1: view().t1, y });
export function pan(d, e) {
  const dt = -(e.clientX - d.x0) * (d.t1 - d.t0) / trackW(d.w);
  setView(d.t0 + dt, d.t1 + dt);
}
// Ruler ticks for canvas width `w`: a labelled one at least 64 px apart,
// minor ones between, on frames or round seconds.
export function ticks(w) {
  const v = view(), pps = trackW(w) / (v.t1 - v.t0), fr = frame();
  const steps = [1, 2, 5, 10, 15].map(n => n * fr).filter(s => s < 1 - 1e-9).concat([1, 2, 5, 10, 15, 30, 60, 120, 300, 600]);
  const major = steps.find(s => s * pps >= 64) ?? steps.at(-1);
  const whole = s => Math.abs(major / s - Math.round(major / s)) < 1e-6;
  const minor = steps.find(s => s * pps >= 7 && whole(s)) ?? major;
  const out = [];
  for (let n = Math.ceil(v.t0 / minor); n * minor <= v.t1; n++) {
    const t = n * minor;
    out.push({ t, major: Math.abs(t / major - Math.round(t / major)) < 1e-6 });
  }
  return out;
}
// `2s`, or `2s 12f` off the whole second.
const timecode = t => {
  const fps = S.R.fps, f = Math.round(t * fps), s = Math.floor(f / fps + 1e-9), r = f - Math.round(s * fps);
  return r ? `${s}s ${r}f` : `${s}s`;
};

// `animators.0.offset` as `a1 offset`, to fit the label column.
export const short = p => p.replace(/^animators\.(\d+)\./, (_, i) => `a${+i + 1} `).replace(/^deformers\.(\d+)\./, (_, i) => `d${+i + 1} `)
  .replace(/^effects\.(\d+)\./, (_, i) => `fx${+i + 1} `)
  .replace(/^parts\.([^.]+)\./, '$1 ').replace(/^params\.(\d+)\.value$/, (_, i) => `param ${+i + 1}`);

// ---------- rows: the tree's layer rows `{l, depth, kids}`, each selected
// layer followed by its key lists `{l, p, keys, depth}`.
let rows = [], scrollY = 0;
function buildRows() {
  const open = new Set(selectedLayers().map(l => l.id));
  if (S.sel) open.add(S.sel);
  const out = [];
  for (const r of treeRows()) {
    out.push(r);
    if (open.has(r.l.id)) for (const t of tracks(r.l)) out.push({ l: r.l, p: t.p, keys: t.keys, depth: r.depth + 1 });
  }
  return out;
}
// The keys a row shows, `{l, p, k}`: one list's, or all of the layer's.
const rowKeys = r => r.p ? r.keys.map(k => ({ l: r.l, p: r.p, k })) : tracks(r.l).flatMap(t => t.keys.map(k => ({ l: r.l, p: t.p, k })));
// The span a layer shows for: [start, end), the whole scene left out.
const barOf = l => [l.start ?? 0, l.end ?? dur()];

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
const EYE_X = LABEL - 16;
export function drawTimeline() {
  rows = buildRows();
  const dpr = devicePixelRatio, w = wrap.clientWidth, h = wrap.clientHeight;
  if (tl.width !== Math.round(w * dpr) || tl.height !== Math.round(h * dpr)) { tl.width = Math.round(w * dpr); tl.height = Math.round(h * dpr); }
  scrollY = Math.max(0, Math.min(scrollY, rows.length * ROW - (h - RULER) + 8));
  const c = tctx; c.setTransform(dpr, 0, 0, dpr, 0, 0); c.clearRect(0, 0, w, h);
  c.font = '11px Inter, sans-serif'; c.textBaseline = 'middle';
  const d = dur(), tk = ticks(w), picked = new Set(S.selKey ? [S.selKey.k] : []);
  const first = Math.floor(scrollY / ROW), last = Math.min(rows.length, Math.ceil((scrollY + h - RULER) / ROW));
  // The track area: rows, the scene's ends shaded past, a line a second.
  c.save(); c.beginPath(); c.rect(LABEL, RULER, w - LABEL, h - RULER); c.clip();
  for (let i = first; i < last; i++) {
    const r = rows[i], y = RULER + i * ROW - scrollY, on = r.p ? r.l.id === S.sel && r.p === S.prop : S.selection.includes(r.l.id);
    c.fillStyle = on ? C.rowOn : i % 2 ? C.rowAlt : C.row; c.fillRect(LABEL, y, w - LABEL, ROW);
  }
  c.fillStyle = C.grid;
  for (const t of tk) if (t.major) c.fillRect(Math.round(xAt(t.t, w)), RULER, 1, h - RULER);
  for (let i = first; i < last; i++) {
    const r = rows[i], y = RULER + i * ROW - scrollY;
    if (!r.p) {
      const [b0, b1] = barOf(r.l), x0 = Math.max(LABEL - 2, xAt(b0, w)), x1 = Math.min(w + 2, xAt(b1, w)), on = S.selection.includes(r.l.id);
      if (x1 > x0) {
        c.globalAlpha = r.l.hidden ? 0.4 : 1;
        c.fillStyle = on ? C.barOn : C.bar; c.fillRect(x0, y + 3, x1 - x0, ROW - 6);
        c.strokeStyle = C.barLine; c.lineWidth = 1; c.strokeRect(x0 + .5, y + 3.5, x1 - x0 - 1, ROW - 7);
        // Trimmed ends show as brighter edges.
        c.fillStyle = C.barEdge;
        if (r.l.start != null) c.fillRect(xAt(b0, w), y + 3, 2, ROW - 6);
        if (r.l.end != null) c.fillRect(xAt(b1, w) - 2, y + 3, 2, ROW - 6);
        c.globalAlpha = 1;
      }
      drawSound(c, r.l, y);
    }
    for (const key of rowKeys(r)) {
      const x = xAt(key.k.t, w);
      if (x < LABEL - 8 || x > w + 8) continue;
      const sel = picked.has(key.k);
      diamond(c, x, y + ROW / 2, sel ? 6 : r.p ? 5 : 4, sel ? C.picked : r.p ? C.key : C.layerKey, sel);
    }
  }
  c.fillStyle = C.outside;
  if (xAt(0, w) > LABEL) c.fillRect(LABEL, RULER, xAt(0, w) - LABEL, h - RULER);
  if (xAt(d, w) < w) c.fillRect(xAt(d, w), RULER, w - xAt(d, w), h - RULER);
  if (snapLine != null) { c.fillStyle = C.guide; c.fillRect(Math.round(xAt(snapLine, w)), RULER, 1, h - RULER); }
  c.restore();
  // The label column: fold triangle, kind, name, eye; key lists indented.
  c.save(); c.beginPath(); c.rect(0, RULER, LABEL, h - RULER); c.clip();
  for (let i = first; i < last; i++) {
    const r = rows[i], y = RULER + i * ROW - scrollY, on = r.p ? r.l.id === S.sel && r.p === S.prop : S.selection.includes(r.l.id);
    c.fillStyle = on ? C.rowOn : i % 2 ? C.rowAlt : C.row; c.fillRect(0, y, LABEL, ROW);
    if (on && r.p) { c.fillStyle = C.textOn; c.fillRect(0, y, 2, ROW); }
    const x = 8 + r.depth * 12;
    c.fillStyle = on ? C.textOn : r.l.hidden ? C.gridText : C.text;
    if (r.p) c.fillText(short(r.p), x + 12, y + ROW / 2);
    else {
      if (r.kids) c.fillText(isFolded(r.l.id) ? '▸' : '▾', x, y + ROW / 2);
      c.fillText(`${KIND_ICON[r.l.kind] ?? ''} ${r.l.name || r.l.id}`, x + 12, y + ROW / 2, EYE_X - x - 16);
      c.fillStyle = r.l.hidden ? C.text : C.tick;
      c.fillText(r.l.hidden ? '–' : '●', EYE_X, y + ROW / 2);
    }
  }
  c.restore();
  c.fillStyle = C.grid; c.fillRect(LABEL - 1, RULER, 1, h - RULER);
  // The ruler: ticks and times, the scene's span brighter.
  c.fillStyle = C.ruler; c.fillRect(0, 0, w, RULER);
  c.save(); c.beginPath(); c.rect(LABEL, 0, w - LABEL, RULER); c.clip();
  for (const t of tk) {
    const x = Math.round(xAt(t.t, w)), inside = t.t >= -1e-9 && t.t <= d + 1e-9;
    c.fillStyle = t.major ? (inside ? C.text : C.tick) : C.tick;
    c.fillRect(x, t.major ? 6 : 16, 1, t.major ? RULER - 6 : RULER - 16);
    if (t.major && t.t >= -1e-9) c.fillText(timecode(t.t), x + 3, 10);
  }
  c.fillStyle = C.barEdge;
  c.fillRect(Math.round(xAt(0, w)), RULER - 2, Math.max(0, xAt(d, w) - xAt(0, w)), 2);
  c.restore();
  c.fillStyle = C.gridText; c.fillText(`${timecode(S.t)}  ·  f${Math.round(S.t * S.R.fps)}`, 8, RULER / 2);
  c.save(); c.beginPath(); c.rect(LABEL, 0, w - LABEL, h); c.clip();
  playhead(c, xAt(S.t, w), h);
  c.restore();
}

// ---------- snapping: a dragged time catches, within 6 px, on the
// playhead, the scene's ends, other keys and other layers' in and out
// points; Ctrl held (or Snap off) keeps to the frame grid only.
let snapLine = null;
function snapTargets(skipKeys = new Set(), skipLayers = new Set(), withPlayhead = true) {
  const ts = [0, dur()];
  if (withPlayhead) ts.push(S.t);
  for (const l of scene().layers) {
    if (!skipLayers.has(l)) { if (l.start != null) ts.push(l.start); if (l.end != null) ts.push(l.end); }
    for (const t of tracks(l)) for (const k of t.keys) if (!skipKeys.has(k)) ts.push(k.t);
  }
  return ts;
}
// A move by `d` of times `ps`: `d` on the frame grid, or onto the target
// nearest any of them. Sets the snap line.
function snapDelta(ps, d, targets, free) {
  const fps = S.R.fps;
  let best = Math.round(d * fps) / fps, gap = Infinity;
  snapLine = null;
  if (free || !snapOn()) return best;
  const reach = 6 * (view().t1 - view().t0) / trackW();
  for (const p of ps) for (const t of targets) {
    const g = Math.abs(t - (p + d));
    if (g <= reach && g < gap) { gap = g; best = t - p; snapLine = t; }
  }
  return best;
}

// ---------- pointer
let drag = null;
function hitAt(e) {
  const r = tl.getBoundingClientRect(), x = e.clientX - r.left, y = e.clientY - r.top;
  if (y < RULER) return { x, y, zone: x < LABEL ? 'corner' : 'ruler' };
  const row = rows[Math.floor((y - RULER + scrollY) / ROW)];
  if (!row) return { x, y, zone: 'empty' };
  if (x < LABEL) {
    const tx = 8 + row.depth * 12;
    return { x, y, row, zone: 'label', twist: !row.p && row.kids && x >= tx - 4 && x < tx + 10, eye: !row.p && x >= EYE_X - 4 };
  }
  let key = null, best = 7;
  for (const k of rowKeys(row)) { const g = Math.abs(xAt(k.k.t) - x); if (g < best) { best = g; key = k; } }
  if (key) return { x, y, row, zone: 'key', key };
  if (!row.p) {
    const [b0, b1] = barOf(row.l), x0 = xAt(b0), x1 = xAt(b1);
    if (Math.abs(x - x0) <= 4) return { x, y, row, zone: 'start' };
    if (Math.abs(x - x1) <= 4) return { x, y, row, zone: 'end' };
    if (x > x0 && x < x1) return { x, y, row, zone: 'bar' };
  }
  return { x, y, row, zone: 'track' };
}
// The layers a bar gesture acts on: the selection when it holds this one.
function barLayers(l, e) {
  if (picking(e)) { select(l.id, 'toggle'); return S.selection.includes(l.id) ? selectedLayers() : null; }
  if (!S.selection.includes(l.id)) select(l.id);
  return selectedLayers().length ? selectedLayers() : [l];
}
// Every time a slide carries: the layer's keys and a plugin's notes.
const timed = l => [...tracks(l).flatMap(t => t.keys), ...(Array.isArray(l.notes) ? l.notes : [])];
tl.onpointerdown = e => {
  const h = hitAt(e);
  tl.setPointerCapture(e.pointerId);
  if (e.button === 1) { e.preventDefault(); drag = startPan(e, tl, scrollY); return; }
  if (e.button !== 0) return;
  if (h.zone === 'ruler') { drag = { scrub: true, targets: snapTargets(new Set(), new Set(), false) }; scrub(h.x, e); return; }
  if (h.zone === 'label') {
    const l = h.row.l;
    if (h.twist) { setFolded(l.id, !isFolded(l.id)); refreshLists(); return; }
    if (h.eye) { edit(() => { if (l.hidden) delete l.hidden; else l.hidden = true; }); return; }
    select(l.id, h.row.p ? undefined : picking(e));
    if (h.row.p && numPaths(l).includes(h.row.p)) S.prop = h.row.p;
    refresh(); return;
  }
  if (h.zone === 'key') {
    // A layer-row diamond carries every key of the layer at that time.
    const group = h.row.p ? [h.key] : rowKeys(h.row).filter(k => Math.abs(k.k.t - h.key.k.t) < 1e-9);
    select(h.row.l.id);
    S.selKey = h.key; if (numPaths(h.row.l).includes(h.key.p)) S.prop = h.key.p;
    const skip = new Set(group.map(k => k.k));
    drag = { keys: group.map(k => ({ ...k, t0: k.k.t })), grab: h.key.k.t, x0: h.x, targets: snapTargets(skip) };
    begin(); refresh(); return;
  }
  if (h.zone === 'start' || h.zone === 'end' || h.zone === 'bar') {
    const ls = barLayers(h.row.l, e);
    if (!ls) return;
    const all = new Set(ls), skip = new Set(ls.flatMap(timed));
    drag = { [h.zone === 'bar' ? 'slide' : 'trim']: h.zone, x0: h.x, moved: false, collapse: !picking(e) && ls.length > 1 ? h.row.l.id : null,
      ls: ls.map(l => ({ l, s: l.start, e: l.end, keys: timed(l).map(k => [k, k.t]) })),
      targets: snapTargets(h.zone === 'bar' ? skip : new Set(), all) };
    begin(); refresh(); return;
  }
  if (h.zone === 'track' || h.zone === 'empty') { drag = { scrub: true, targets: snapTargets(new Set(), new Set(), false) }; scrub(h.x, e); }
};
function scrub(x, e) {
  const t = snapDelta([0], timeAt(x), drag.targets, e.ctrlKey || e.metaKey);
  seek(Math.max(0, Math.min(dur(), t)));
}
tl.onpointermove = e => {
  if (!drag) {
    const z = hitAt(e).zone;
    tl.style.cursor = z === 'start' || z === 'end' ? 'ew-resize' : z === 'bar' ? 'grab' : z === 'key' ? 'pointer' : z === 'ruler' ? 'col-resize' : 'default';
    return;
  }
  const x = e.clientX - tl.getBoundingClientRect().left, free = e.ctrlKey || e.metaKey;
  if (drag.pan) { pan(drag, e); scrollY = drag.y - (e.clientY - drag.y0); return; }
  if (drag.scrub) { scrub(x, e); return; }
  if (drag.keys) {
    const d = snapDelta([drag.grab], timeAt(x) - timeAt(drag.x0), drag.targets, free);
    const lists = new Set();
    for (const { l, p, k, t0 } of drag.keys) { k.t = tidy(Math.max(0, t0 + d)); lists.add(getp(l, p)); }
    for (const ks of lists) ks.sort((a, b) => a.t - b.t);
    changed(); return;
  }
  drag.moved = true;
  if (drag.trim) {
    const t = snapDelta([0], timeAt(x), drag.targets, free);
    for (const { l } of drag.ls) {
      if (drag.trim === 'start') {
        const s = Math.min(t, (l.end ?? dur()) - frame());
        if (Math.abs(s) < 1e-9) delete l.start; else l.start = tidy(s);
      } else {
        const en = Math.max(t, (l.start ?? 0) + frame());
        if (en >= dur() - 1e-9) delete l.end; else l.end = tidy(en);
      }
    }
    changed(); return;
  }
  if (drag.slide) {
    // A slide moves the in and out points and every key and note with
    // them (keys are in scene time), so the layer plays the same, later.
    const edges = drag.ls.flatMap(o => [o.s ?? 0, o.e ?? dur()]);
    const d = snapDelta(edges, timeAt(x) - timeAt(drag.x0), drag.targets, free);
    for (const o of drag.ls) {
      if (o.s == null && d === 0) delete o.l.start; else o.l.start = tidy((o.s ?? 0) + d);
      const en = (o.e ?? dur()) + d;
      if (o.e == null && en >= dur() - 1e-9) delete o.l.end; else o.l.end = tidy(en);
      for (const [k, t] of o.keys) k.t = tidy(t + d);
    }
    changed();
  }
};
tl.onpointerup = () => {
  const was = drag;
  drag = null; snapLine = null; S.tlNeed = true;
  if (!was || was.pan || was.scrub) return;
  end();
  if (was.collapse && !was.moved) select(was.collapse);
};
tl.ondblclick = e => {
  const h = hitAt(e);
  if (h.row && !h.row.p && (h.zone === 'label' || h.zone === 'bar') && openComp(h.row.l)) refresh();
};
tl.addEventListener('wheel', e => wheel(e, tl, dy => { scrollY += dy; S.tlNeed = true; }), { passive: false });
// Alt+[ / Alt+]: the selected layers' in or out point to the playhead.
export function trimToPlayhead(side) {
  const ls = selectedLayers(), t = Math.round(S.t * S.R.fps) / S.R.fps;
  if (!ls.length) return;
  edit(() => {
    for (const l of ls) {
      if (side === 'start' && t < (l.end ?? dur())) { if (t === 0) delete l.start; else l.start = t; }
      if (side === 'end' && t > (l.start ?? 0)) { if (t >= dur()) delete l.end; else l.end = t; }
    }
  });
}
new ResizeObserver(() => { S.tlNeed = true; }).observe(wrap);
