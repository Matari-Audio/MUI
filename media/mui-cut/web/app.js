// mui-cut web editor. The project lives here as plain JSON; every edit goes
// through the WASM engine (`Cut.load`), which also draws the viewport and
// samples the graph editor's curves, so what you see is what `mui-cut render`
// writes. ponytail: the engine runs on the main thread, not a worker; move it
// to a worker if big projects make scrubbing stutter.
import init, { Cut } from './pkg/mui_cut.js';

const $ = s => document.querySelector(s);
const PROPS = ['x', 'y', 'scale', 'rotation', 'opacity', 'width', 'height', 'radius', 'font_size', 'weight'];
const KEYABLE = [...PROPS, 'fill'];
const DEFAULTS = { x: 0, y: 0, scale: 1, rotation: 0, opacity: 1, width: 100, height: 100, radius: 0, font_size: 64, weight: 600, fill: '#ffffff' };
const KIND_ICON = { rect: '▭', ellipse: '◯', text: 'T', image: '▣' };

await init();
const cut = new Cut();

let doc = null;          // the project, as JSON data
let saved = '';          // the file's text as last loaded or saved
let si = 0;              // scene index
let t = 0;               // playhead, seconds into the scene
let sel = null;          // selected layer id
let prop = 'x';          // property shown in the graph editor
let selKey = null;       // { l, p, k }: the selected key object
let frame = null;        // evaluated frame at the playhead
let quads = [];          // layer outlines from the last render, project px
let playing = false;
let need = true;
const undo = [], redo = [];
let base = null;         // the document before the gesture in progress
const images = new Set();

// ---------- document helpers
const scene = () => doc.scenes[si];
const layer = () => scene()?.layers.find(l => l.id === sel) ?? null;
const isKeys = v => Array.isArray(v);
const snap = x => Math.round(x * doc.fps) / doc.fps;
const round = v => Math.round(v * 1000) / 1000;
const keyAt = (keys, time) => keys.findIndex(k => Math.abs(k.t - time) < 0.25 / doc.fps);
function now(l, p) {
  const d = frame?.layers.find(d => d.id === l.id);
  return d && p in d ? d[p] : (l[p] ?? DEFAULTS[p]);
}
function insertKey(keys, k) { keys.push(k); keys.sort((a, b) => a.t - b.t); return k; }
// Write a value at the playhead: into the key there (or a new one) when the
// property is animated, else as its plain value.
function setValue(l, p, v) {
  const cur = l[p];
  if (!isKeys(cur)) { l[p] = v; return; }
  const i = keyAt(cur, snap(t));
  if (i >= 0) cur[i].v = v; else insertKey(cur, { t: snap(t), v, interp: 'bezier' });
}
// The ◆ button: add a key at the playhead, or remove the one there; the last
// key removed leaves the property a plain value.
function toggleKey(l, p) {
  const cur = l[p], v = now(l, p);
  if (!isKeys(cur)) { l[p] = [{ t: snap(t), v, interp: 'bezier' }]; return; }
  const i = keyAt(cur, snap(t));
  if (i < 0) selKey = { l, p, k: insertKey(cur, { t: snap(t), v, interp: 'bezier' }) };
  else if (cur.length === 1) l[p] = v;
  else cur.splice(i, 1);
}
function deleteKey({ l, p, k }) {
  const keys = l[p];
  if (!isKeys(keys)) return;
  if (keys.length === 1) l[p] = k.v; else keys.splice(keys.indexOf(k), 1);
  selKey = null;
}

// ---------- edits, undo, save
function begin() { if (base === null) base = JSON.stringify(doc); }
function changed() {
  try { cut.load(JSON.stringify(doc)); showError(''); } catch (e) { showError(String(e)); }
  need = true;
}
function end() {
  if (base === null) return;
  if (JSON.stringify(doc) !== base) { undo.push(base); redo.length = 0; save(); }
  base = null;
  refresh();
}
function edit(fn) { begin(); fn(); changed(); end(); }
function restore(text) {
  doc = JSON.parse(text); selKey = null;
  changed(); save(); refresh();
}
$('#undo').onclick = () => { if (undo.length) { redo.push(JSON.stringify(doc)); restore(undo.pop()); } };
$('#redo').onclick = () => { if (redo.length) { undo.push(JSON.stringify(doc)); restore(redo.pop()); } };

let saveTimer = 0;
function save() {
  clearTimeout(saveTimer);
  status('unsaved');
  saveTimer = setTimeout(async () => {
    const r = await fetch('/project', { method: 'PUT', body: JSON.stringify(doc) });
    const text = await r.text();
    if (!r.ok) { status('not saved: ' + text, true); return; }
    saved = text;
    status('saved');
  }, 200);
}
function status(s, bad = false) { $('#status').textContent = s; $('#status').classList.toggle('bad', bad); }
function showError(s) { $('#error').hidden = !s; $('#error').textContent = s; }

// The file changed: an agent (or you, in a text editor) saved it.
async function pull(why) {
  const text = await (await fetch('/project')).text();
  if (text === saved) return;
  try { cut.load(text); } catch (e) { status('the file on disk has an error: ' + e, true); return; }
  if (doc) { undo.push(JSON.stringify(doc)); redo.length = 0; }
  doc = JSON.parse(text); saved = text; selKey = null;
  si = Math.min(si, doc.scenes.length - 1);
  if (!layer()) sel = null;
  await loadImages();
  status(why); refresh();
}
async function loadImages() {
  for (const l of doc.scenes.flatMap(s => s.layers)) {
    if (l.kind !== 'image' || images.has(l.path)) continue;
    images.add(l.path);
    try {
      const r = await fetch('/asset/' + l.path);
      if (r.ok) cut.add_png(l.path, new Uint8Array(await r.arrayBuffer()));
    } catch (e) { console.warn(l.path, e); }
  }
}

// ---------- scene and layer lists
function refreshLists() {
  $('#scenes').replaceChildren(...doc.scenes.map((s, i) => {
    const b = document.createElement('button');
    b.textContent = `${s.name}  ·  ${s.duration}s`;
    b.className = i === si ? 'on' : '';
    b.onclick = () => { si = i; sel = null; selKey = null; t = Math.min(t, s.duration); refresh(); };
    return b;
  }));
  $('#layers').replaceChildren(...[...scene().layers].reverse().map(l => {
    const b = document.createElement('button');
    b.innerHTML = `<span class="kind">${KIND_ICON[l.kind] ?? '?'}</span>`;
    b.append(l.name || l.id);
    b.className = l.id === sel ? 'on' : '';
    b.onclick = () => select(l.id);
    return b;
  }));
}
function select(id) {
  if (sel !== id) selKey = null;
  sel = id;
  const l = layer();
  if (l && !PROPS.includes(prop)) prop = 'x';
  if (l && !isKeys(l[prop])) prop = PROPS.find(p => isKeys(l[p])) ?? prop;
  refresh();
}
$('#add-scene').onclick = () => edit(() => {
  doc.scenes.push({ name: `scene ${doc.scenes.length + 1}`, duration: 2, layers: [] });
  si = doc.scenes.length - 1; sel = null;
});
document.querySelectorAll('[data-add]').forEach(b => b.onclick = () => edit(() => {
  const kind = b.dataset.add, ls = scene().layers;
  let n = 1; while (ls.some(l => l.id === `${kind}${n}`)) n++;
  const [w, h] = doc.size;
  const l = { id: `${kind}${n}`, kind, x: w / 2, y: h / 2 };
  if (kind === 'text') Object.assign(l, { text: 'Text' });
  else Object.assign(l, { width: 200, height: 200, fill: '#8b7cff' });
  ls.push(l); sel = l.id;
}));
function moveLayer(d) {
  const ls = scene().layers, i = ls.findIndex(l => l.id === sel), j = i + d;
  if (i < 0 || j < 0 || j >= ls.length) return;
  edit(() => { [ls[i], ls[j]] = [ls[j], ls[i]]; });
}
$('#layer-up').onclick = () => moveLayer(1);
$('#layer-down').onclick = () => moveLayer(-1);
$('#layer-del').onclick = () => { if (layer()) edit(() => { scene().layers = scene().layers.filter(l => l.id !== sel); sel = null; selKey = null; }); };

// ---------- inspector
function field(label, input, keyBtn) {
  const l = document.createElement('label'); l.textContent = label;
  const box = $('#inspector');
  box.append(l, input);
  if (keyBtn) box.append(keyBtn); else input.classList.add('wide');
}
function input(value, onchange, type = 'text') {
  const i = document.createElement('input');
  i.type = type; i.value = value;
  if (type === 'number') i.step = 'any';
  i.onchange = () => onchange(i.value);
  return i;
}
function refreshInspector() {
  const box = $('#inspector'); box.replaceChildren();
  const l = layer();
  if (!l) {
    const s = scene();
    $('#insp-title').textContent = 'Scene';
    field('name', input(s.name, v => edit(() => { s.name = v; })));
    field('duration', input(s.duration, v => edit(() => { s.duration = Math.max(0.05, Number(v) || 1); }), 'number'));
    field('background', input(s.background ?? '#101014', v => edit(() => { s.background = v; })));
    field('project', input(`${doc.size[0]}×${doc.size[1]} @ ${doc.fps} fps`, () => {}));
    return;
  }
  $('#insp-title').textContent = `Layer · ${l.kind}`;
  field('id', input(l.id, v => { if (v && !scene().layers.some(o => o.id === v)) edit(() => { l.id = v; sel = v; }); }));
  if (l.kind === 'text') field('text', input(l.text, v => edit(() => { l.text = v; })));
  if (l.kind === 'image') field('path', input(l.path, v => edit(() => { l.path = v; loadImages(); })));
  for (const p of KEYABLE) {
    if (l.kind === 'text' && (p === 'width' || p === 'height' || p === 'radius')) continue;
    if (l.kind !== 'text' && (p === 'font_size' || p === 'weight')) continue;
    const i = input(p === 'fill' ? now(l, p) : round(now(l, p)), v => edit(() => setValue(l, p, p === 'fill' ? v : Number(v))), p === 'fill' ? 'text' : 'number');
    i.dataset.prop = p;
    const k = document.createElement('button');
    k.className = 'key'; k.textContent = '◆'; k.dataset.key = p;
    k.title = 'Add or remove a key at the playhead';
    k.onclick = () => { edit(() => toggleKey(l, p)); if (PROPS.includes(p)) prop = p; refresh(); };
    field(p, i, k);
  }
  updateInspector();
}
// Values follow the playhead without rebuilding the panel.
function updateInspector() {
  const l = layer();
  if (!l) return;
  for (const i of document.querySelectorAll('#inspector input[data-prop]')) {
    if (document.activeElement === i) continue;
    const p = i.dataset.prop;
    i.value = p === 'fill' ? now(l, p) : round(now(l, p));
  }
  for (const b of document.querySelectorAll('#inspector [data-key]')) {
    const v = l[b.dataset.key];
    b.className = 'key' + (isKeys(v) ? (keyAt(v, snap(t)) >= 0 ? ' here' : ' animated') : '');
  }
}

// ---------- viewport
const view = $('#view'), over = $('#overlay');
const vctx = view.getContext('2d'), octx = over.getContext('2d');
let hover = null, drag = null;
function layoutViewport() {
  const box = $('#stage').getBoundingClientRect(), [pw, ph] = doc.size;
  const k = Math.min((box.width - 32) / pw, (box.height - 32) / ph);
  const cw = Math.max(1, Math.floor(pw * k)), ch = Math.max(1, Math.floor(ph * k));
  $('#frame').style.width = cw + 'px'; $('#frame').style.height = ch + 'px';
  const w = Math.max(2, Math.round(Math.min(pw, cw * devicePixelRatio))), h = Math.max(2, Math.round(w * ph / pw));
  if (view.width !== w || view.height !== h) { view.width = w; view.height = h; }
  const ow = Math.round(cw * devicePixelRatio), oh = Math.round(ch * devicePixelRatio);
  if (over.width !== ow || over.height !== oh) { over.width = ow; over.height = oh; }
}
function drawViewport() {
  layoutViewport();
  try {
    const px = cut.render(si, t, view.width, view.height);
    vctx.putImageData(new ImageData(new Uint8ClampedArray(px.buffer, px.byteOffset, px.length), view.width, view.height), 0, 0);
    quads = JSON.parse(cut.quads());
  } catch (e) { showError(String(e)); }
  const k = over.width / doc.size[0];
  octx.clearRect(0, 0, over.width, over.height);
  for (const [id, width, color] of [[hover, 1, '#ffffff88'], [sel, 2, '#8b7cff']]) {
    const q = quads.find(q => q.id === id);
    if (!q) continue;
    octx.beginPath();
    q.pts.forEach(([x, y], i) => i ? octx.lineTo(x * k, y * k) : octx.moveTo(x * k, y * k));
    octx.closePath(); octx.lineWidth = width * devicePixelRatio; octx.strokeStyle = color; octx.stroke();
  }
}
function toProject(e) {
  const r = over.getBoundingClientRect();
  return [(e.clientX - r.left) / r.width * doc.size[0], (e.clientY - r.top) / r.height * doc.size[1]];
}
// Inside a convex quad: the point is on the same side of all four edges.
function inside([x, y], pts) {
  let pos = 0, neg = 0;
  for (let i = 0; i < 4; i++) {
    const [ax, ay] = pts[i], [bx, by] = pts[(i + 1) % 4];
    const c = (bx - ax) * (y - ay) - (by - ay) * (x - ax);
    if (c > 0) pos++; else if (c < 0) neg++;
  }
  return !(pos && neg);
}
const hit = p => [...quads].reverse().find(q => inside(p, q.pts))?.id ?? null;
over.onpointerdown = e => {
  const p = toProject(e), id = hit(p);
  select(id);
  if (!id) return;
  const l = layer();
  drag = { p, l, x0: now(l, 'x'), y0: now(l, 'y') };
  over.setPointerCapture(e.pointerId);
  begin();
};
over.onpointermove = e => {
  const p = toProject(e);
  if (!drag) { const h = hit(p); if (h !== hover) { hover = h; need = true; } over.style.cursor = h ? 'move' : 'default'; return; }
  setValue(drag.l, 'x', round(drag.x0 + p[0] - drag.p[0]));
  setValue(drag.l, 'y', round(drag.y0 + p[1] - drag.p[1]));
  changed();
};
over.onpointerup = () => { drag = null; end(); };

// ---------- timeline
const tl = $('#timeline'), tctx = tl.getContext('2d');
const LABEL = 150, RULER = 22, ROW = 20;
let tlRows = [], tlDrag = null;
const tlX = time => LABEL + time / scene().duration * (tl.clientWidth - LABEL - 12);
const tlT = x => Math.max(0, Math.min(scene().duration, (x - LABEL) / (tl.clientWidth - LABEL - 12) * scene().duration));
function rows() {
  const out = [];
  for (const l of [...scene().layers].reverse()) {
    out.push({ l });
    if (l.id === sel) for (const p of KEYABLE) if (isKeys(l[p])) out.push({ l, p });
  }
  return out;
}
// The keys a row shows: one prop's, or every key of the layer.
function rowKeys(r) {
  const ps = r.p ? [r.p] : KEYABLE.filter(p => isKeys(r.l[p]));
  return ps.flatMap(p => r.l[p].map(k => ({ l: r.l, p, k })));
}
function diamond(c, x, y, s, fill) {
  c.beginPath(); c.moveTo(x, y - s); c.lineTo(x + s, y); c.lineTo(x, y + s); c.lineTo(x - s, y); c.closePath();
  c.fillStyle = fill; c.fill();
}
function drawTimeline() {
  tlRows = rows();
  const dpr = devicePixelRatio, w = tl.clientWidth, h = RULER + tlRows.length * ROW + 8;
  tl.style.height = h + 'px';
  if (tl.width !== Math.round(w * dpr) || tl.height !== Math.round(h * dpr)) { tl.width = Math.round(w * dpr); tl.height = Math.round(h * dpr); }
  const c = tctx; c.setTransform(dpr, 0, 0, dpr, 0, 0); c.clearRect(0, 0, w, h);
  c.font = '11px Inter, sans-serif'; c.textBaseline = 'middle';
  const dur = scene().duration;
  // Ruler: a tick every tenth, a label every second.
  for (let i = 0; i <= Math.round(dur * 10); i++) {
    const x = tlX(i / 10), whole = i % 10 === 0;
    c.fillStyle = whole ? '#8d8aa3' : '#3a3d4a';
    c.fillRect(x, whole ? 4 : 14, 1, whole ? 18 : 8);
    if (whole) c.fillText(`${i / 10}s`, x + 3, 9);
  }
  tlRows.forEach((r, i) => {
    const y = RULER + i * ROW;
    const on = r.l.id === sel && (r.p ? r.p === prop : false);
    c.fillStyle = i % 2 ? '#17181f' : '#1a1b23'; c.fillRect(0, y, w, ROW);
    if (on) { c.fillStyle = '#8b7cff22'; c.fillRect(0, y, w, ROW); }
    c.fillStyle = r.l.id === sel && !r.p ? '#eceaf6' : '#8d8aa3';
    c.fillText(r.p ? `   ${r.p}` : `${KIND_ICON[r.l.kind] ?? ''} ${r.l.name || r.l.id}`, 8, y + ROW / 2);
    for (const key of rowKeys(r)) {
      const picked = selKey && selKey.k === key.k;
      diamond(c, tlX(key.k.t), y + ROW / 2, r.p ? 5 : 4, picked ? '#ffffff' : r.p ? '#ffcf5a' : '#b8a86a');
    }
  });
  c.fillStyle = '#ff5a7a'; c.fillRect(tlX(t), 0, 1.5, h);
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
  if (h.row && h.x < LABEL) { select(h.row.l.id); if (h.row.p && PROPS.includes(h.row.p)) prop = h.row.p; refresh(); return; }
  if (h.key) {
    // A layer-row diamond carries every key of the layer at that time.
    const group = h.row.p ? [h.key] : rowKeys(h.row).filter(k => Math.abs(k.k.t - h.key.k.t) < 1e-9);
    select(h.row.l.id);
    selKey = h.key; if (PROPS.includes(h.key.p)) prop = h.key.p;
    tlDrag = { group, t0: h.key.k.t, x0: h.x };
    begin(); refresh(); return;
  }
  tlDrag = { scrub: true }; t = snap(tlT(h.x)); need = true;
};
tl.onpointermove = e => {
  if (!tlDrag) return;
  const h = tlHit(e);
  if (tlDrag.scrub) { t = snap(tlT(h.x)); need = true; return; }
  const nt = snap(tlT(tlX(tlDrag.t0) + h.x - tlDrag.x0));
  for (const { l, p, k } of tlDrag.group) { k.t = nt; l[p].sort((a, b) => a.t - b.t); }
  changed();
};
tl.onpointerup = () => { if (tlDrag && !tlDrag.scrub) end(); tlDrag = null; };

// ---------- graph editor
const gr = $('#graph'), gctx = gr.getContext('2d');
let range = null, grDrag = null;
function gKeys() { const l = layer(); return l && isKeys(l[prop]) ? l[prop] : null; }
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
function drawGraph() {
  const sel0 = $('#graph-prop');
  const l = layer();
  const dpr = devicePixelRatio, w = gr.clientWidth, h = gr.clientHeight;
  if (gr.width !== Math.round(w * dpr) || gr.height !== Math.round(h * dpr)) { gr.width = Math.round(w * dpr); gr.height = Math.round(h * dpr); }
  const c = gctx; c.setTransform(dpr, 0, 0, dpr, 0, 0); c.clearRect(0, 0, w, h);
  c.font = '11px Inter, sans-serif'; c.textBaseline = 'middle';
  if (sel0.dataset.for !== (l?.id ?? '') + prop) {
    sel0.dataset.for = (l?.id ?? '') + prop;
    sel0.replaceChildren(...PROPS.map(p => new Option(p + (l && isKeys(l[p]) ? ' ◆' : ''), p, false, p === prop)));
  }
  if (!l) { c.fillStyle = '#8d8aa3'; c.fillText('Select a layer to see its curves.', LABEL, h / 2); return; }
  const dur = scene().duration, n = Math.max(2, Math.round(w - LABEL - 12));
  const samples = cut.sample(si, l.id, prop, 0, dur, n);
  const keys = gKeys();
  if (!grDrag || !range) {
    let lo = Infinity, hi = -Infinity;
    const see = v => { lo = Math.min(lo, v); hi = Math.max(hi, v); };
    samples.forEach(see);
    keys?.forEach((k, i) => { see(k.v); handles(keys, i).forEach(hd => see(hd.v)); });
    if (!(hi - lo > 1e-6)) { lo -= 1; hi += 1; }
    const pad = (hi - lo) * 0.12; range = [lo - pad, hi + pad];
  }
  // The same time axis as the timeline above it, so keys line up.
  const gx = time => LABEL + time / dur * (w - LABEL - 12);
  const gy = v => 10 + (range[1] - v) / (range[1] - range[0]) * (h - 20);
  // Grid: seconds, and four value lines with their numbers.
  c.fillStyle = '#23252f';
  for (let s = 0; s <= dur + 1e-9; s += 0.5) c.fillRect(gx(s), 0, 1, h);
  for (let i = 0; i <= 4; i++) {
    const v = range[0] + (range[1] - range[0]) * i / 4;
    c.fillStyle = '#23252f'; c.fillRect(LABEL, gy(v), w - LABEL - 12, 1);
    c.fillStyle = '#6d6a83'; c.fillText(Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(2), 8, gy(v));
  }
  c.beginPath();
  samples.forEach((v, i) => { const x = gx(i / (n - 1) * dur); i ? c.lineTo(x, gy(v)) : c.moveTo(x, gy(v)); });
  c.strokeStyle = '#8b7cff'; c.lineWidth = 2; c.stroke();
  if (!keys) { c.fillStyle = '#8d8aa3'; c.fillText(`${prop} is a plain value: press ◆ in the inspector to animate it.`, LABEL + 8, 14); }
  keys?.forEach((k, i) => {
    const picked = selKey && selKey.k === k;
    for (const hd of handles(keys, i)) {
      c.beginPath(); c.moveTo(gx(k.t), gy(k.v)); c.lineTo(gx(hd.t), gy(hd.v));
      c.strokeStyle = picked ? '#ffffffaa' : '#ffcf5a66'; c.lineWidth = 1; c.stroke();
      c.beginPath(); c.arc(gx(hd.t), gy(hd.v), 4, 0, 7); c.fillStyle = picked ? '#fff' : '#ffcf5a'; c.fill();
    }
  });
  keys?.forEach(k => {
    const picked = selKey && selKey.k === k;
    c.fillStyle = picked ? '#ffffff' : '#ffcf5a';
    c.fillRect(gx(k.t) - 4, gy(k.v) - 4, 8, 8);
  });
  c.fillStyle = '#ff5a7a'; c.fillRect(gx(t), 0, 1.5, h);
  gr._map = { gx, gy, tOf: x => (x - LABEL) / (w - LABEL - 12) * dur, vOf: y => range[1] - (y - 10) / (h - 20) * (range[1] - range[0]) };
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
  const h = grHit(e), keys = gKeys();
  if (h.i === undefined) return;
  gr.setPointerCapture(e.pointerId);
  selKey = { l: layer(), p: prop, k: keys[h.i] };
  grDrag = { k: keys[h.i], side: h.side };
  begin(); need = true;
};
gr.onpointermove = e => {
  if (!grDrag) return;
  const m = gr._map, r = gr.getBoundingClientRect();
  const time = m.tOf(e.clientX - r.left), v = m.vOf(e.clientY - r.top);
  const keys = gKeys(), k = grDrag.k, i = keys.indexOf(k);
  if (!grDrag.side) {
    // A key moves between its neighbours, snapped to frames.
    const lo = i > 0 ? keys[i - 1].t + 1 / doc.fps : 0, hi = i < keys.length - 1 ? keys[i + 1].t - 1 / doc.fps : scene().duration;
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
gr.onpointerup = () => { if (grDrag) { grDrag = null; end(); } };
gr.ondblclick = e => {
  const l = layer(), m = gr._map;
  if (!l || !m) return;
  const r = gr.getBoundingClientRect();
  const time = snap(Math.max(0, Math.min(scene().duration, m.tOf(e.clientX - r.left))));
  const v = round(m.vOf(e.clientY - r.top));
  edit(() => {
    if (!isKeys(l[prop])) l[prop] = [];
    const i = keyAt(l[prop], time);
    if (i >= 0) l[prop][i].v = v; else selKey = { l, p: prop, k: insertKey(l[prop], { t: time, v, interp: 'bezier' }) };
  });
};
$('#graph-prop').onchange = e => { prop = e.target.value; selKey = null; range = null; refresh(); };
document.querySelectorAll('[data-interp]').forEach(b => b.onclick = () => { if (selKey) edit(() => { selKey.k.interp = b.dataset.interp; }); });
$('#reset-handles').onclick = () => { if (selKey) edit(() => { delete selKey.k.in; delete selKey.k.out; }); };

// ---------- transport, keys, loop
function setPlaying(on) { playing = on; $('#play').textContent = on ? '❚❚' : '▶'; }
$('#play').onclick = () => setPlaying(!playing);
addEventListener('keydown', e => {
  if (e.target.closest('input, select')) return;
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key.toLowerCase() === 'z') { e.preventDefault(); $(e.shiftKey ? '#redo' : '#undo').click(); }
  else if (mod && e.key.toLowerCase() === 'y') { e.preventDefault(); $('#redo').click(); }
  else if (e.key === ' ') { e.preventDefault(); setPlaying(!playing); }
  else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') { t = snap(Math.max(0, Math.min(scene().duration, t + (e.key === 'ArrowRight' ? 1 : -1) / doc.fps))); need = true; }
  else if (e.key === 'Home') { t = 0; need = true; }
  else if ((e.key === 'Delete' || e.key === 'Backspace') && selKey) edit(() => deleteKey(selKey));
  else if (e.key.toLowerCase() === 'k' && layer()) edit(() => toggleKey(layer(), prop));
});
function refresh() { refreshLists(); refreshInspector(); range = null; need = true; }
let last = performance.now();
function loop(ms) {
  if (playing && doc) {
    t += (ms - last) / 1000;
    if (t >= scene().duration) t %= scene().duration;
    need = true;
  }
  last = ms;
  if (need && doc) {
    need = false;
    frame = JSON.parse(cut.frame(si, t) || 'null');
    drawViewport(); drawTimeline(); drawGraph(); updateInspector();
    $('#time').textContent = `${t.toFixed(2)} s  ·  f${Math.round(t * doc.fps)}`;
  }
  requestAnimationFrame(loop);
}
addEventListener('resize', () => { need = true; });

$('#file').textContent = await (await fetch('/name')).text();
await pull('loaded');
new EventSource('/events').onmessage = () => pull('reloaded from disk');
requestAnimationFrame(loop);
