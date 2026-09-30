// mui-cut web editor. The project lives here as plain JSON; every edit goes
// through the WASM engine (`Cut.load`), which also draws the viewport and
// samples the graph editor's curves, so what you see is what `mui-cut render`
// writes. The viewport draws in worker.js (WebGPU or WebGL2 when the browser
// has it, else Vello CPU); this thread keeps a `Cut` for validation, the inspector
// and the graph's samples.
import init, { Cut } from './pkg/mui_cut.js';

const $ = s => document.querySelector(s);
const KIND_ICON = { rect: '▭', ellipse: '◯', text: 'T', image: '▣', path: '〰', duplicator: '⁂', svg: 'S', lottie: 'L', camera: '⌖', light: '☀', model: '◈' };
const VECTOR = ['text', 'path', 'duplicator', 'svg', 'lottie'];
// Graphite, as in style.css: greys only, state by value, weight and shape.
const C = {
  text: '#8a8a8a', textOn: '#e6e6e6', grid: '#262626', gridText: '#666666',
  tick: '#3a3a3a', row: '#181818', rowAlt: '#1c1c1c', rowOn: '#262626',
  curve: '#d4d4d4', key: '#b0b0b0', layerKey: '#6a6a6a', handle: '#7a7a7a',
  handleLine: '#ffffff30', picked: '#ffffff', pickedLine: '#ffffffa0',
  playhead: '#f2f2f2', hover: '#ffffff70', sel: '#ffffff', halo: '#000000a0',
};

await init();
const cut = new Cut();
const FX = JSON.parse(Cut.effects());   // the effect schema: [{name, about, params}]
const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });
let drawing = false;     // a draw is in flight in the worker

let doc = null;          // the project, as JSON data (bindings and all)
let R = null;            // the project as the engine resolved it: sizes, fps, durations
let variant = '';        // the variant the viewport previews, '' for the defaults
let saved = '';          // the file's text as last loaded or saved
let si = 0;              // scene index
let t = 0;               // playhead, seconds into the scene
let sel = null;          // selected layer id
let prop = 'x';          // property shown in the graph editor
let selKey = null;       // { l, p, k }: the selected key object
let quads = [];          // layer outlines from the last render, project px
let playing = false;
let need = true;
let locked = false;      // play on the project's frame grid only
// Playback clock: while playing, project time is the monotonic clock since
// `clock.at` (ms), plus the time `clock.t` it started from. It never
// accumulates per-frame deltas, so it cannot drift; a slow draw drops
// frames instead of slowing the clock.
const clock = { at: 0, t: 0 };
// Pacing, for the HUD and the e2e: when each drawn frame came back (ms) and
// the project frame it showed; frames the playhead passed without drawing.
const pacing = { shown: [], dropped: 0, last: -1, pending: -1 };
globalThis.pacing = pacing;
const undo = [], redo = [];
let base = null;         // the document before the gesture in progress
const assets = new Set();

// ---------- document helpers
const scene = () => doc.scenes[si];
const layer = () => scene()?.layers.find(l => l.id === sel) ?? null;
const isKeys = v => Array.isArray(v);
// `{"var": ...}`: a value a variable decides; the inspector shows it, the
// Variables panel changes it.
const isBind = v => v !== null && typeof v === 'object' && !Array.isArray(v) && 'var' in v;
const bound = (o, p) => { const v = getp(o, p); return isBind(v) || (isKeys(v) && v.some(k => isBind(k.v))); };
const snap = x => Math.round(x * R.fps) / R.fps;
const round = v => Math.round(v * 1000) / 1000;
const keyAt = (keys, time) => keys.findIndex(k => Math.abs(k.t - time) < 0.25 / R.fps);
// Properties are addressed by path: `x`, `fill`, `animators.0.offset`.
const getp = (o, p) => p.split('.').reduce((o, k) => o?.[k], o);
function setp(o, p, v) {
  const ks = p.split('.'), last = ks.pop(), parent = ks.reduce((o, k) => o[k], o);
  parent[last] = v;
}
// The engine says which properties a layer has and what they are at the
// playhead: `[{p, v}]`, v a number or a colour string.
const propsOf = (l, time = t) => l ? JSON.parse(cut.props(si, l.id, time)) : [];
const keyPaths = l => propsOf(l, 0).map(r => r.p);
const numPaths = l => propsOf(l, 0).filter(r => typeof r.v === 'number').map(r => r.p);
const now = (l, p) => propsOf(l).find(r => r.p === p)?.v;
function insertKey(keys, k) { keys.push(k); keys.sort((a, b) => a.t - b.t); return k; }
// Write a value at the playhead: into the key there (or a new one) when the
// property is animated, else as its plain value.
function setValue(l, p, v) {
  if (bound(l, p)) return;
  const cur = getp(l, p);
  if (!isKeys(cur)) { setp(l, p, v); return; }
  const i = keyAt(cur, snap(t));
  if (i >= 0) cur[i].v = v; else insertKey(cur, { t: snap(t), v, interp: 'bezier' });
}
// The ◆ button: add a key at the playhead, or remove the one there; the last
// key removed leaves the property a plain value.
function toggleKey(l, p, v = now(l, p)) {
  if (bound(l, p)) return;
  const cur = getp(l, p);
  if (!isKeys(cur)) { setp(l, p, [{ t: snap(t), v, interp: 'bezier' }]); return; }
  const i = keyAt(cur, snap(t));
  if (i < 0) selKey = { l, p, k: insertKey(cur, { t: snap(t), v, interp: 'bezier' }) };
  else if (cur.length === 1) setp(l, p, v);
  else cur.splice(i, 1);
}
function deleteKey({ l, p, k }) {
  const keys = getp(l, p);
  if (!isKeys(keys)) return;
  if (keys.length === 1) setp(l, p, k.v); else keys.splice(keys.indexOf(k), 1);
  selKey = null;
}

// ---------- edits, undo, save
function begin() { if (base === null) base = JSON.stringify(doc); }
function changed() {
  const json = JSON.stringify(doc);
  try { load(json); showError(''); } catch (e) { showError(String(e)); }
  need = true;
}
// Hand `json` to the engine and the viewport, as the chosen variant.
function load(json) {
  cut.load(json, variant || undefined);
  R = JSON.parse(cut.resolved());
  worker.postMessage({ type: 'load', json, variant });
}
function end() {
  if (base === null) return;
  if (JSON.stringify(doc) !== base) { undo.push(base); redo.length = 0; save(); }
  base = null;
  refresh();
}
function edit(fn) { begin(); fn(); changed(); end(); }
// The CPU viewport draws without effects: say so rather than look wrong.
function noticeEffects() {
  const b = $('#backend');
  if (b.dataset.backend !== 'CPU') return;
  const any = doc.scenes.some(s => s.effects?.length || s.layers.some(l => l.effects?.length));
  b.textContent = any ? 'CPU · effects off' : 'CPU';
  b.title = any ? 'This browser has no WebGPU: the viewport draws without effects' : b.title;
}
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
  const was = variant;
  if (!JSON.parse(text).variants?.some(v => v.name === variant)) variant = '';
  try { load(text); } catch (e) { variant = was; status('the file on disk has an error: ' + e, true); return; }
  if (doc) { undo.push(JSON.stringify(doc)); redo.length = 0; }
  doc = JSON.parse(text); saved = text; selKey = null;
  si = Math.min(si, doc.scenes.length - 1);
  if (!layer()) sel = null;
  await loadAssets();
  status(why); refresh();
}
// The files image, SVG, Lottie and model layers name, each sent to the viewport once.
async function loadAssets() {
  for (const l of doc.scenes.flatMap(s => s.layers)) {
    if (!['image', 'svg', 'lottie', 'model'].includes(l.kind) || !l.path || assets.has(l.path)) continue;
    assets.add(l.path);
    try {
      const r = await fetch('/asset/' + l.path);
      if (r.ok) worker.postMessage({ type: 'asset', path: l.path, bytes: new Uint8Array(await r.arrayBuffer()) });
    } catch (e) { console.warn(l.path, e); }
  }
}

// ---------- scene and layer lists
function refreshLists() {
  $('#scenes').replaceChildren(...doc.scenes.map((s, i) => {
    const b = document.createElement('button');
    const d = R.scenes[i].duration;
    b.textContent = `${s.name}  ·  ${d}s`;
    b.className = i === si ? 'on' : '';
    b.onclick = () => { si = i; sel = null; selKey = null; t = Math.min(t, d); refresh(); };
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
  const l = layer(), nums = numPaths(l);
  if (l && !nums.includes(prop)) prop = 'x';
  if (l && !isKeys(getp(l, prop))) prop = nums.find(p => isKeys(getp(l, p))) ?? prop;
  refresh();
}
$('#add-scene').onclick = () => edit(() => {
  doc.scenes.push({ name: `scene ${doc.scenes.length + 1}`, duration: 2, layers: [] });
  si = doc.scenes.length - 1; sel = null;
});
document.querySelectorAll('[data-add]').forEach(b => b.onclick = () => edit(() => {
  // A shader layer is a rect whose stack starts with a generator: the
  // rect is its mask.
  const name = b.dataset.add, kind = name === 'shader' ? 'rect' : name, ls = scene().layers;
  let n = 1; while (ls.some(l => l.id === `${name}${n}`)) n++;
  const [w, h] = R.size;
  const l = { id: `${name}${n}`, kind, x: w / 2, y: h / 2 };
  if (kind === 'text') Object.assign(l, { text: 'Text' });
  else if (kind === 'path') Object.assign(l, { d: 'M -150 0 C -75 -120 75 120 150 0', fill: '#00000000', stroke: '#8b7cff', stroke_width: 6 });
  else if (kind === 'duplicator') Object.assign(l, { width: 40, height: 40, radius: 8, fill: '#8b7cff', count: 12, spacing_x: 60, spacing_y: 60 });
  else if (kind === 'camera') Object.assign(l, { distance: Math.round(h / 2 / Math.tan(20 * Math.PI / 180)) });
  else if (kind === 'light') Object.assign(l, { rx: 50, ry: -30 });
  else if (kind === 'model') Object.assign(l, { path: 'model.glb', height: 200, fill: '#ffffff' });
  else Object.assign(l, { width: 200, height: 200, fill: '#8b7cff' });
  if (name === 'shader') Object.assign(l, { width: w / 2, height: h / 2, effects: [{ type: 'plasma' }] });
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
  const i = document.createElement(type === 'area' ? 'textarea' : 'input');
  if (type !== 'area') i.type = type;
  i.value = value;
  if (type === 'number') i.step = 'any';
  i.onchange = () => onchange(i.value);
  return i;
}
function choice(value, options, onchange) {
  const s = document.createElement('select');
  s.replaceChildren(...options.map(o => new Option(o, o, false, o === String(value))));
  s.onchange = () => onchange(s.value);
  return s;
}
// A full-width header over an animator's or deformer's rows, with its
// settings and a remove button.
function section(title, controls, remove) {
  const h = document.createElement('div'); h.className = 'section';
  const n = document.createElement('span'); n.textContent = title;
  const x = document.createElement('button'); x.textContent = '×'; x.title = 'Remove'; x.onclick = remove;
  h.append(n, ...controls, x);
  $('#inspector').append(h);
}
// The settings of a layer that are not keyable: its text, file, path data,
// layout and so on, and how an animator or deformer selects and moves.
function kindFields(l) {
  const set = (k, v, dflt) => edit(() => { if (v === dflt) delete l[k]; else l[k] = v; });
  if (l.kind === 'text') {
    field('text', input(l.text, v => edit(() => { l.text = v; }), 'area'));
    field('align', choice(l.align ?? 'center', ['left', 'center', 'right'], v => set('align', v, 'center')));
  }
  if (['image', 'svg', 'lottie', 'model'].includes(l.kind)) field('path', input(l.path, v => edit(() => { l.path = v; loadAssets(); })));
  if (l.kind === 'lottie') {
    field('speed', input(l.speed ?? 1, v => set('speed', Number(v), 1), 'number'));
    field('loop', choice(l.loop ?? true, ['true', 'false'], v => set('loop', v === 'true', true)));
  }
  if (l.kind === 'path') field('d', input(l.d, v => edit(() => { l.d = v; }), 'area'));
  if (l.kind === 'camera') {
    field('look_at', choice(l.look_at ?? '', ['', ...scene().layers.filter(o => o !== l).map(o => o.id)], v => set('look_at', v, '')));
    field('path', input(l.path ?? '', v => set('path', v, ''), 'area'));
  }
  if (l.kind === 'light') field('type', choice(l.type ?? 'directional', ['directional', 'spot', 'point', 'ambient'], v => set('type', v, 'directional')));
  if (l.kind === 'duplicator') {
    field('shape', choice(l.shape ?? 'rect', ['rect', 'ellipse', 'path'], v => set('shape', v, 'rect')));
    if (l.shape === 'path') field('d', input(l.d ?? '', v => set('d', v, ''), 'area'));
    field('layout', choice(l.layout ?? 'grid', ['grid', 'radial', 'linear', 'path'], v => set('layout', v, 'grid')));
    if (l.layout === 'path') field('along', input(l.along ?? '', v => set('along', v, ''), 'area'));
    field('orient', choice(l.orient ?? false, ['false', 'true'], v => set('orient', v === 'true', false)));
  }
}
function groupHeader(l, group, i) {
  const list = l[group], item = list[i];
  const remove = () => edit(() => { list.splice(i, 1); if (!list.length) delete l[group]; selKey = null; });
  const set = (k, v, dflt) => edit(() => { if (v === dflt) delete item[k]; else item[k] = v; });
  if (group === 'animators') {
    const c = [];
    if (l.kind === 'text') c.push(choice(item.by ?? 'char', ['char', 'word', 'line'], v => set('by', v, 'char')));
    c.push(choice(item.shape ?? 'square', ['square', 'ramp_up', 'ramp_down', 'triangle', 'round', 'smooth'], v => set('shape', v, 'square')));
    c.push(choice(item.ease ?? 'linear', ['linear', 'in', 'out', 'in_out', 'step'], v => set('ease', v, 'linear')));
    c.push(choice(item.order ?? 'forward', ['forward', 'reverse', 'random'], v => set('order', v, 'forward')));
    if (item.order === 'random') { const s = input(item.seed ?? 0, v => set('seed', Math.max(0, Math.round(Number(v))) || 0, 0), 'number'); s.title = 'seed'; c.push(s); }
    section(`Animator ${i + 1}`, c, remove);
  } else {
    const c = [];
    if (item.kind === 'noise') { const s = input(item.seed ?? 0, v => set('seed', Math.max(0, Math.round(Number(v))) || 0, 0), 'number'); s.title = 'seed'; c.push(s); }
    section(`${item.kind} ${i + 1}`, c, remove);
  }
}
function adder(label, options, add) {
  const s = choice('', ['', ...options], v => { if (v) add(v); });
  s.options[0].textContent = label;
  s.classList.add('wide'); s.dataset.adder = label;
  const box = $('#inspector'); box.append(document.createElement('span'), s);
}
// A bound field shows its value and stays read-only.
function lock(i, v) {
  if (!isBind(v) && !(isKeys(v) && v.some(k => isBind(k.v)))) return i;
  i.disabled = true;
  i.title = `bound to the variable \`${(isBind(v) ? v : v.find(k => isBind(k.v)).v).var}\``;
  return i;
}
// The project's variables, as the chosen variant sets them: edits go to the
// variant, or to the declared value with no variant chosen.
function variablesSection() {
  const vars = Object.entries(doc.variables ?? {});
  if (!vars.length) return;
  const box = $('#inspector'), head = document.createElement('h3');
  head.className = 'fx-head'; head.textContent = variant ? `Variables · ${variant}` : 'Variables';
  box.append(head);
  const v = doc.variants?.find(v => v.name === variant);
  for (const [name, d] of vars) {
    const cur = v?.vars?.[name] ?? d.value;
    const set = x => edit(() => { if (v) (v.vars ??= {})[name] = x; else d.value = x; });
    let i;
    if (d.type === 'enum') {
      i = document.createElement('select');
      i.append(...d.options.map(o => new Option(o, o)));
      i.value = cur; i.onchange = () => set(i.value);
    } else if (d.type === 'bool') {
      i = document.createElement('input'); i.type = 'checkbox';
      i.checked = cur; i.onchange = () => set(i.checked);
    } else i = input(cur, x => set(d.type === 'number' ? Number(x) : x), d.type === 'number' ? 'number' : 'text');
    i.dataset.var = name;
    field(name, i);
  }
}
function refreshInspector() {
  const box = $('#inspector'); box.replaceChildren();
  const l = layer();
  if (!l) {
    const s = scene();
    $('#insp-title').textContent = 'Scene';
    field('name', input(s.name, v => edit(() => { s.name = v; })));
    field('duration', lock(input(R.scenes[si].duration, v => edit(() => { s.duration = Math.max(0.05, Number(v) || 1); }), 'number'), s.duration));
    const bg = lock(input(frame?.background ?? s.background ?? '#101014', v => edit(() => { s.background = v; })), s.background);
    bg.dataset.bg = '';
    field('background', bg);
    field('project', input(`${R.size[0]}×${R.size[1]} @ ${R.fps} fps`, () => {}));
    variablesSection();
    fxSection(s, () => frame?.effects ?? []);
    return;
  }
  $('#insp-title').textContent = `Layer · ${l.kind}`;
  field('id', input(l.id, v => { if (v && !scene().layers.some(o => o.id === v)) edit(() => { l.id = v; sel = v; }); }));
  kindFields(l);
  let group = '';
  for (const { p, v } of propsOf(l)) {
    const parts = p.split('.');
    const g = parts.length > 1 ? parts.slice(0, 2).join('.') : '';
    if (g !== group) { group = g; if (g) groupHeader(l, parts[0], +parts[1]); }
    const color = typeof v === 'string';
    const i = input(color ? v : round(v), x => edit(() => setValue(l, p, color ? x : Number(x))), color ? 'text' : 'number');
    i.dataset.prop = p;
    lock(i, getp(l, p));
    const k = document.createElement('button');
    k.className = 'key'; k.textContent = '◆'; k.dataset.key = p;
    k.title = 'Add or remove a key at the playhead';
    k.onclick = () => { edit(() => toggleKey(l, p)); if (!color) prop = p; refresh(); };
    field(parts.at(-1), i, k);
  }
  if (l.kind === 'text' || l.kind === 'duplicator') adder('+ animator', ['plain', 'typewriter', 'cascade', 'pop'], v => edit(() => {
    const a = v === 'plain' ? {} : JSON.parse(cut.preset(v, snap(t), 1));
    (l.animators ??= []).push(a);
  }));
  if (VECTOR.includes(l.kind)) adder('+ deformer', ['noise', 'twist', 'bend', 'wave'], v => edit(() => { (l.deformers ??= []).push({ kind: v }); }));
  fxSection(l, () => frame?.layers.find(d => d.id === l.id)?.effects ?? []);
  updateInspector();
}
// An effect stack (a layer's or the scene's): each effect's parameters are
// properties like any other, typed and keyed from the inspector.
let fxFields = [];
function fxSection(owner, live) {
  const box = $('#inspector');
  fxFields = [];
  const head = document.createElement('h3');
  head.className = 'fx-head'; head.textContent = 'Effects';
  const add = document.createElement('select');
  add.id = 'add-effect'; add.setAttribute('aria-label', 'Add an effect');
  add.append(new Option('+ add', ''), ...FX.map(d => new Option(d.name, d.name)));
  add.onchange = () => { const type = add.value; if (type) edit(() => { (owner.effects ??= []).push({ type }); }); };
  box.append(head, add);
  (owner.effects ?? []).forEach((e, i) => {
    const def = FX.find(d => d.name === e.type);
    const title = document.createElement('div');
    title.className = 'fx-title'; title.textContent = e.type; title.title = def?.about ?? '';
    const rm = document.createElement('button');
    rm.className = 'key'; rm.textContent = '✕'; rm.title = 'Remove the effect';
    rm.onclick = () => edit(() => { owner.effects.splice(i, 1); if (!owner.effects.length) delete owner.effects; });
    box.append(title, rm);
    for (const p of def?.params ?? []) {
      const color = typeof p.default === 'string';
      const get = () => live()[i]?.[p.name] ?? p.default;
      const inp = input(color ? get() : round(get()), v => edit(() => setValue(e, p.name, color ? v : Number(v))), color ? 'text' : 'number');
      inp.dataset.fx = `${i}.${p.name}`;
      lock(inp, e[p.name]);
      if (!color) { inp.min = p.min; inp.max = p.max; }
      const k = document.createElement('button');
      k.className = 'key'; k.textContent = '◆'; k.title = 'Add or remove a key at the playhead';
      k.onclick = () => edit(() => toggleKey(e, p.name, get()));
      field(p.name, inp, k);
      fxFields.push({ inp, k, get, keys: () => e[p.name], color });
    }
  });
}
// Values follow the playhead without rebuilding the panel.
function updateInspector() {
  const l = layer();
  if (!l) {
    const bg = $('#inspector [data-bg]');
    if (bg?.disabled && frame) bg.value = frame.background;
    updateFx(); return;
  }
  const vals = Object.fromEntries(propsOf(l).map(r => [r.p, r.v]));
  for (const i of document.querySelectorAll('#inspector input[data-prop]')) {
    if (document.activeElement === i) continue;
    const v = vals[i.dataset.prop];
    i.value = typeof v === 'string' ? v : round(v);
  }
  for (const b of document.querySelectorAll('#inspector [data-key]')) {
    const v = getp(l, b.dataset.key);
    b.className = 'key' + (isKeys(v) ? (keyAt(v, snap(t)) >= 0 ? ' here' : ' animated') : '');
  }
  updateFx();
}
function updateFx() {
  for (const f of fxFields) {
    if (document.activeElement !== f.inp) f.inp.value = f.color ? f.get() : round(f.get());
    const v = f.keys();
    f.k.className = 'key' + (isKeys(v) ? (keyAt(v, snap(t)) >= 0 ? ' here' : ' animated') : '');
  }
}

// ---------- viewport
const view = $('#view'), over = $('#overlay');
const octx = over.getContext('2d');
let vw = 2, vh = 2;       // the viewport's pixel size; the worker owns the canvas
{
  const canvas = view.transferControlToOffscreen();
  const renderer = new URLSearchParams(location.search).get('renderer');
  worker.postMessage({ type: 'init', canvas, renderer }, [canvas]);
  const { backend, adapter, engine } = await new Promise(ok => { worker.onmessage = e => ok(e.data); });
  $('#backend').textContent = backend;
  $('#backend').dataset.backend = backend;
  $('#backend').dataset.engine = engine;
  $('#backend').title = backend === 'CPU' ? 'Viewport on the CPU (Vello CPU): no WebGPU or WebGL2'
    : `Viewport on ${backend}, Vello ${engine} (${adapter})`;
}
worker.onmessage = ({ data: m }) => {
  if (m.type === 'progress' || m.type === 'exported' || m.type === 'export-error') { exportMessage(m); return; }
  if (m.type !== 'drawn') { if (m.error) showError(m.error); return; }
  drawing = false;
  if (playing && pacing.pending >= 0) {
    // A frame the playhead skipped over is dropped; so is a whole pass.
    const skipped = pacing.pending - pacing.last - 1;
    if (pacing.last >= 0 && skipped > 0) pacing.dropped += skipped;
    pacing.last = pacing.pending;
    pacing.shown.push(performance.now());
    if (pacing.shown.length > 240) pacing.shown.shift();
  }
  pacing.pending = -1;
  view.dataset.draws = +(view.dataset.draws ?? 0) + 1;   // e2e counts these
  view.dataset.ms = +(view.dataset.ms ?? 0) + (m.ms ?? 0);  // and times them
  if (m.error) showError(m.error); else quads = JSON.parse(m.quads);
  $('#notice').hidden = !m.notice; $('#notice').textContent = m.notice ?? '';
  $('#orbit').hidden = scene()?.mode !== '3d' || !!m.notice;
  drawOverlay();
};
let hover = null, drag = null;
function layoutViewport() {
  const box = $('#stage').getBoundingClientRect(), [pw, ph] = R.size;
  const k = Math.min((box.width - 32) / pw, (box.height - 32) / ph);
  const cw = Math.max(1, Math.floor(pw * k)), ch = Math.max(1, Math.floor(ph * k));
  $('#frame').style.width = cw + 'px'; $('#frame').style.height = ch + 'px';
  vw = Math.max(2, Math.round(Math.min(pw, cw * devicePixelRatio))); vh = Math.max(2, Math.round(vw * ph / pw));
  const ow = Math.round(cw * devicePixelRatio), oh = Math.round(ch * devicePixelRatio);
  if (over.width !== ow || over.height !== oh) { over.width = ow; over.height = oh; }
}
// False while the worker is still busy: the loop asks again next frame, so
// only the newest playhead is ever drawn.
function drawViewport() {
  layoutViewport();
  drawOverlay();
  if (drawing) return false;
  drawing = true;
  worker.postMessage({ type: 'draw', si, t, w: vw, h: vh });
  pacing.pending = playing ? Math.floor(t * R.fps + 1e-6) : -1;
  return true;
}
function drawOverlay() {
  const k = over.width / R.size[0];
  octx.clearRect(0, 0, over.width, over.height);
  const dpr = devicePixelRatio;
  for (const [id, width, color] of [[hover, 1, C.hover], [sel, 1.5, C.sel]]) {
    const q = quads.find(q => q.id === id);
    if (!q) continue;
    octx.beginPath();
    q.pts.forEach(([x, y], i) => i ? octx.lineTo(x * k, y * k) : octx.moveTo(x * k, y * k));
    octx.closePath();
    // A dark hairline under the light one keeps it legible on light scenes.
    octx.lineWidth = (width + 2) * dpr; octx.strokeStyle = C.halo; octx.stroke();
    octx.lineWidth = width * dpr; octx.strokeStyle = color; octx.stroke();
    if (id === sel) for (const [x, y] of q.pts) { // corner squares mark the selection
      octx.fillStyle = C.halo; octx.fillRect(x * k - 4 * dpr, y * k - 4 * dpr, 8 * dpr, 8 * dpr);
      octx.fillStyle = C.sel; octx.fillRect(x * k - 3 * dpr, y * k - 3 * dpr, 6 * dpr, 6 * dpr);
    }
  }
}
function toProject(e) {
  const r = over.getBoundingClientRect();
  return [(e.clientX - r.left) / r.width * R.size[0], (e.clientY - r.top) / r.height * R.size[1]];
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
// The orbit preview swings the shot camera about its target; the project
// never sees it.
const orbit = { on: false, yaw: 0, pitch: 0, zoom: 1, from: null };
const sendOrbit = () => { worker.postMessage({ type: 'orbit', ...orbit, from: undefined }); need = true; };
$('#orbit').onclick = () => {
  Object.assign(orbit, { on: !orbit.on, yaw: 0, pitch: 0, zoom: 1 });
  $('#orbit').setAttribute('aria-pressed', orbit.on); $('#orbit').classList.toggle('on', orbit.on);
  sendOrbit();
};
over.addEventListener('wheel', e => {
  if (!orbit.on) return;
  e.preventDefault(); orbit.zoom = Math.min(8, Math.max(0.1, orbit.zoom * Math.exp(e.deltaY * 0.001))); sendOrbit();
}, { passive: false });
over.onpointerdown = e => {
  if (orbit.on) { orbit.from = [e.clientX, e.clientY, orbit.yaw, orbit.pitch]; over.setPointerCapture(e.pointerId); return; }
  const p = toProject(e), id = hit(p);
  select(id);
  if (!id) return;
  const l = layer();
  drag = { p, l, x0: now(l, 'x'), y0: now(l, 'y') };
  over.setPointerCapture(e.pointerId);
  begin();
};
over.onpointermove = e => {
  if (orbit.on) {
    if (!orbit.from) return;
    const [x0, y0, yaw, pitch] = orbit.from;
    orbit.yaw = yaw - (e.clientX - x0) * 0.4; orbit.pitch = Math.max(-89, Math.min(89, pitch + (e.clientY - y0) * 0.4)); sendOrbit();
    return;
  }
  const p = toProject(e);
  if (!drag) { const h = hit(p); if (h !== hover) { hover = h; need = true; } over.style.cursor = h ? 'move' : 'default'; return; }
  setValue(drag.l, 'x', round(drag.x0 + p[0] - drag.p[0]));
  setValue(drag.l, 'y', round(drag.y0 + p[1] - drag.p[1]));
  changed();
};
over.onpointerup = () => { if (orbit.on) { orbit.from = null; return; } drag = null; end(); };

// ---------- timeline
const tl = $('#timeline'), tctx = tl.getContext('2d');
const LABEL = 150, RULER = 22, ROW = 20;
let tlRows = [], tlDrag = null;
const tlX = time => LABEL + time / R.scenes[si].duration * (tl.clientWidth - LABEL - 12);
const tlT = x => Math.max(0, Math.min(R.scenes[si].duration, (x - LABEL) / (tl.clientWidth - LABEL - 12) * R.scenes[si].duration));
// `animators.0.offset` as `a1 offset`, to fit the label column.
const short = p => p.replace(/^animators\.(\d+)\./, (_, i) => `a${+i + 1} `).replace(/^deformers\.(\d+)\./, (_, i) => `d${+i + 1} `);
function rows() {
  const out = [];
  for (const l of [...scene().layers].reverse()) {
    out.push({ l });
    if (l.id === sel) for (const p of keyPaths(l)) if (isKeys(getp(l, p))) out.push({ l, p });
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
function playhead(c, x, h) {
  c.fillStyle = C.playhead; c.fillRect(Math.round(x), 0, 1, h);
  c.beginPath(); c.moveTo(x - 5, 0); c.lineTo(x + 6, 0); c.lineTo(x + 6, 6); c.lineTo(x + .5, 11); c.lineTo(x - 5, 6); c.closePath(); c.fill();
}
function drawTimeline() {
  tlRows = rows();
  const dpr = devicePixelRatio, w = tl.clientWidth, h = RULER + tlRows.length * ROW + 8;
  tl.style.height = h + 'px';
  if (tl.width !== Math.round(w * dpr) || tl.height !== Math.round(h * dpr)) { tl.width = Math.round(w * dpr); tl.height = Math.round(h * dpr); }
  const c = tctx; c.setTransform(dpr, 0, 0, dpr, 0, 0); c.clearRect(0, 0, w, h);
  c.font = '11px Inter, sans-serif'; c.textBaseline = 'middle';
  const dur = R.scenes[si].duration;
  // Ruler: a tick every tenth, a label every second.
  for (let i = 0; i <= Math.round(dur * 10); i++) {
    const x = tlX(i / 10), whole = i % 10 === 0;
    c.fillStyle = whole ? C.text : C.tick;
    c.fillRect(x, whole ? 4 : 14, 1, whole ? 18 : 8);
    if (whole) c.fillText(`${i / 10}s`, x + 3, 9);
  }
  tlRows.forEach((r, i) => {
    const y = RULER + i * ROW;
    const on = r.l.id === sel && (r.p ? r.p === prop : false);
    c.fillStyle = on ? C.rowOn : i % 2 ? C.rowAlt : C.row; c.fillRect(0, y, w, ROW);
    if (on) { c.fillStyle = C.textOn; c.fillRect(0, y, 2, ROW); }
    c.fillStyle = r.l.id === sel && (!r.p || on) ? C.textOn : C.text;
    c.fillText(r.p ? `   ${short(r.p)}` : `${KIND_ICON[r.l.kind] ?? ''} ${r.l.name || r.l.id}`, 8, y + ROW / 2);
    for (const key of rowKeys(r)) {
      const picked = selKey && selKey.k === key.k;
      diamond(c, tlX(key.k.t), y + ROW / 2, picked ? 6 : r.p ? 5 : 4, picked ? C.picked : r.p ? C.key : C.layerKey, picked);
    }
  });
  playhead(c, tlX(t), h);
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
  if (h.row && h.x < LABEL) { select(h.row.l.id); if (h.row.p && numPaths(h.row.l).includes(h.row.p)) prop = h.row.p; refresh(); return; }
  if (h.key) {
    // A layer-row diamond carries every key of the layer at that time.
    const group = h.row.p ? [h.key] : rowKeys(h.row).filter(k => Math.abs(k.k.t - h.key.k.t) < 1e-9);
    select(h.row.l.id);
    selKey = h.key; if (numPaths(h.row.l).includes(h.key.p)) prop = h.key.p;
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

// ---------- graph editor
const gr = $('#graph'), gctx = gr.getContext('2d');
let range = null, grDrag = null;
function gKeys() { const l = layer(), k = l && getp(l, prop); return isKeys(k) ? k : null; }
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
  const nums = l ? numPaths(l) : [];
  if (sel0.dataset.for !== (l?.id ?? '') + prop + nums.length) {
    sel0.dataset.for = (l?.id ?? '') + prop + nums.length;
    sel0.replaceChildren(...nums.map(p => new Option(short(p) + (isKeys(getp(l, p)) ? ' ◆' : ''), p, false, p === prop)));
  }
  if (!l) { c.fillStyle = C.text; c.fillText('Select a layer to see its curves.', LABEL, h / 2); return; }
  const dur = R.scenes[si].duration, n = Math.max(2, Math.round(w - LABEL - 12));
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
  c.fillStyle = C.grid;
  for (let s = 0; s <= dur + 1e-9; s += 0.5) c.fillRect(gx(s), 0, 1, h);
  for (let i = 0; i <= 4; i++) {
    const v = range[0] + (range[1] - range[0]) * i / 4;
    c.fillStyle = C.grid; c.fillRect(LABEL, gy(v), w - LABEL - 12, 1);
    c.fillStyle = C.gridText; c.fillText(Math.abs(v) >= 100 ? v.toFixed(0) : v.toFixed(2), 8, gy(v));
  }
  c.beginPath();
  samples.forEach((v, i) => { const x = gx(i / (n - 1) * dur); i ? c.lineTo(x, gy(v)) : c.moveTo(x, gy(v)); });
  c.strokeStyle = C.curve; c.lineWidth = 1.5; c.stroke();
  if (!keys) { c.fillStyle = C.text; c.fillText(`${prop} is a plain value: press ◆ in the inspector to animate it.`, LABEL + 8, 14); }
  keys?.forEach((k, i) => {
    const picked = selKey && selKey.k === k;
    for (const hd of handles(keys, i)) {
      c.beginPath(); c.moveTo(gx(k.t), gy(k.v)); c.lineTo(gx(hd.t), gy(hd.v));
      c.strokeStyle = picked ? C.pickedLine : C.handleLine; c.lineWidth = 1; c.stroke();
      // Handles are round, keys square: shape, not colour, tells them apart.
      c.beginPath(); c.arc(gx(hd.t), gy(hd.v), picked ? 4 : 3, 0, 7); c.fillStyle = picked ? C.picked : C.handle; c.fill();
    }
  });
  keys?.forEach(k => {
    const picked = selKey && selKey.k === k;
    const s = picked ? 5 : 4;
    c.fillStyle = C.halo; c.fillRect(gx(k.t) - s - 1, gy(k.v) - s - 1, 2 * s + 2, 2 * s + 2);
    c.fillStyle = picked ? C.picked : C.key; c.fillRect(gx(k.t) - s, gy(k.v) - s, 2 * s, 2 * s);
  });
  playhead(c, gx(t), h);
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
    const lo = i > 0 ? keys[i - 1].t + 1 / R.fps : 0, hi = i < keys.length - 1 ? keys[i + 1].t - 1 / R.fps : R.scenes[si].duration;
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
  const time = snap(Math.max(0, Math.min(R.scenes[si].duration, m.tOf(e.clientX - r.left))));
  const v = round(m.vOf(e.clientY - r.top));
  edit(() => {
    if (!isKeys(getp(l, prop))) setp(l, prop, []);
    const keys = getp(l, prop), i = keyAt(keys, time);
    if (i >= 0) keys[i].v = v; else selKey = { l, p: prop, k: insertKey(keys, { t: time, v, interp: 'bezier' }) };
  });
};
$('#graph-prop').onchange = e => { prop = e.target.value; selKey = null; range = null; refresh(); };
document.querySelectorAll('[data-interp]').forEach(b => b.onclick = () => { if (selKey) edit(() => { selKey.k.interp = b.dataset.interp; }); });
$('#reset-handles').onclick = () => { if (selKey) edit(() => { delete selKey.k.in; delete selKey.k.out; }); };

// ---------- export: WebCodecs in the worker, MP4 by mp4.js
// Codec strings by what the frame needs: level 4.x up to 1080p, 5.x above.
function codecs() {
  const big = R.size[0] * R.size[1] > 1920 * 1088;
  return [
    ['H.264', big ? 'avc1.640033' : 'avc1.640028'],
    ['H.265', big ? 'hvc1.1.6.L153.B0' : 'hvc1.1.6.L123.B0'],
    ['AV1', big ? 'av01.0.12M.08' : 'av01.0.08M.08'],
  ];
}
let exportRun = null;   // { start } while an export runs
async function openExport() {
  const [w, h] = R.size, dlg = $('#export-dialog');
  const sel = $('#ex-codec'); sel.replaceChildren();
  for (const [name, codec] of codecs()) {
    const ok = (await VideoEncoder.isConfigSupported({ codec, width: w, height: h, bitrate: 8e6, framerate: R.fps }).catch(() => ({}))).supported;
    const o = new Option(name + (ok ? '' : ' (not in this browser)'), codec); o.disabled = !ok;
    sel.append(o);
  }
  sel.value = [...sel.options].find(o => !o.disabled)?.value ?? '';
  // About 0.1 bit a pixel a frame, at least 2 Mbit/s.
  $('#ex-bitrate').value = Math.max(2, Math.round(w * h * R.fps * 0.1 / 1e5) / 10);
  const gpu = $('#backend').dataset.backend === 'WebGPU';
  $('#ex-mb').value = gpu ? (doc.render?.mb ?? 1) : 1;
  $('#ex-mb').disabled = !gpu;
  $('#ex-note').textContent = `${w}×${h} at ${R.fps} fps, every scene` + (gpu ? '' : ' · CPU: no effects or motion blur');
  $('#ex-status').textContent = ''; $('#ex-progress').hidden = true;
  $('#ex-start').disabled = !sel.value;
  if (!sel.value) $('#ex-status').textContent = 'This browser encodes none of these codecs.';
  dlg.showModal();
}
$('#export').onclick = () => { if (!exportRun) openExport(); else $('#export-dialog').showModal(); };
if (!('VideoEncoder' in self)) { $('#export').disabled = true; $('#export').title = 'This browser has no WebCodecs'; }
$('#ex-start').onclick = () => {
  const codec = $('#ex-codec').value;
  exportRun = { start: performance.now(), codec };
  $('#ex-start').disabled = true; $('#ex-cancel').textContent = 'Cancel';
  $('#ex-progress').hidden = false; $('#ex-progress').value = 0;
  $('#ex-status').textContent = 'rendering';
  worker.postMessage({
    type: 'export', codec, w: R.size[0], h: R.size[1], fps: R.fps,
    mb: Math.max(1, Math.min(64, Math.round(+$('#ex-mb').value || 1))),
    bitrate: Math.round((+$('#ex-bitrate').value || 8) * 1e6),
    scenes: R.scenes.map(s => ({ duration: s.duration })),
  });
};
$('#ex-cancel').onclick = () => { if (exportRun) worker.postMessage({ type: 'cancel' }); else $('#export-dialog').close(); };
function exportMessage(m) {
  if (m.type === 'progress') {
    $('#ex-progress').value = m.done / m.total;
    $('#ex-status').textContent = `frame ${m.done} of ${m.total}`;
    return;
  }
  const secs = ((performance.now() - exportRun.start) / 1000).toFixed(1);
  exportRun = null;
  $('#ex-start').disabled = false; $('#ex-cancel').textContent = 'Close';
  if (m.type === 'export-error') { $('#ex-status').textContent = m.error === 'cancelled' ? 'cancelled' : 'failed: ' + m.error; return; }
  const blob = new Blob([m.bytes], { type: 'video/mp4' });
  window.lastExport = { blob, frames: m.frames, codec: m.codec, hardware: m.hardware };   // e2e reads it
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = ($('#file').textContent || 'cut').replace(/\.cut\.json$|\.json$/, '') + '.mp4';
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 60_000);
  $('#ex-status').textContent = `${m.frames} frames, ${(blob.size / 1e6).toFixed(1)} MB in ${secs} s`;
}

// ---------- transport, keys, loop
function setPlaying(on) {
  playing = on; $('#play').textContent = on ? '❚❚' : '▶';
  Object.assign(clock, { at: performance.now(), t });
  Object.assign(pacing, { shown: [], dropped: 0, last: -1 });
}
// A seek while playing restarts the clock from the new playhead.
function seek(time) { t = time; clock.at = performance.now(); clock.t = t; need = true; }
const toggle = (id, on) => { $(id).setAttribute('aria-pressed', String(on)); return on; };
$('#lock').onclick = () => { locked = toggle('#lock', !locked); seek(t); };
$('#hud-toggle').onclick = () => { $('#hud').hidden = !toggle('#hud-toggle', $('#hud').hidden); };
const quantile = (xs, q) => xs.length ? [...xs].sort((a, b) => a - b)[Math.min(xs.length - 1, Math.floor(q * xs.length))] : 0;
function drawHud() {
  const d = pacing.shown.slice(1).map((v, i) => v - pacing.shown[i]);
  const span = (pacing.shown.at(-1) - pacing.shown[0]) / 1000;
  $('#hud').textContent = `${d.length && span > 0 ? (d.length / span).toFixed(1) : '–'} fps  ${pacing.dropped} dropped\n`
    + `frame ${quantile(d, 0.5).toFixed(1)} / ${quantile(d, 0.95).toFixed(1)} ms p50/p95`;
}
$('#play').onclick = () => setPlaying(!playing);
addEventListener('keydown', e => {
  if (e.target.closest('input, select, textarea')) return;
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key.toLowerCase() === 'z') { e.preventDefault(); $(e.shiftKey ? '#redo' : '#undo').click(); }
  else if (mod && e.key.toLowerCase() === 'y') { e.preventDefault(); $('#redo').click(); }
  else if (e.key === ' ') { e.preventDefault(); setPlaying(!playing); }
  else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') seek(snap(Math.max(0, Math.min(R.scenes[si].duration, t + (e.key === 'ArrowRight' ? 1 : -1) / R.fps))));
  else if (e.key === 'Home') seek(0);
  else if ((e.key === 'Delete' || e.key === 'Backspace') && selKey) edit(() => deleteKey(selKey));
  else if (e.key.toLowerCase() === 'k' && layer()) edit(() => toggleKey(layer(), prop));
});
// The header's variant switcher: hidden for a project without variants.
function refreshVariants() {
  const sel = $('#variant'), vs = doc.variants ?? [];
  sel.hidden = !vs.length;
  sel.replaceChildren(new Option('defaults', ''), ...vs.map(v => new Option(`${v.name}${v.size ? ` · ${v.size[0]}×${v.size[1]}` : ''}`, v.name)));
  sel.value = variant;
}
$('#variant').onchange = () => {
  variant = $('#variant').value;
  try { load(JSON.stringify(doc)); showError(''); } catch (e) { showError(String(e)); }
  t = Math.min(t, R.scenes[si].duration);
  refresh();
};
function refresh() { refreshVariants(); refreshLists(); refreshInspector(); noticeEffects(); range = null; need = true; }
// rAF hands every callback of a display frame the same timestamp; the
// frame drawn now is presented about one display frame later, so that is
// the time it shows.
let vsync = 1000 / 60, lastMs = 0;
function loop(ms) {
  if (lastMs) vsync += (Math.min(ms - lastMs, 100) - vsync) * 0.05;
  lastMs = ms;
  if (playing && doc) {
    const d = R.scenes[si].duration;
    let next = clock.t + (ms + vsync - clock.at) / 1000;
    if (next >= d) { next %= d; Object.assign(clock, { at: ms + vsync, t: next }); pacing.last = -1; }
    if (locked) next = Math.floor(next * R.fps + 1e-6) / R.fps;
    // On the grid, a frame is drawn once; off it, every display frame.
    if (next !== t || !locked) { t = next; need = true; }
  }
  if (!$('#hud').hidden) drawHud();
  if (need && doc) {
    need = false;
    if (!drawViewport()) need = true;
    drawTimeline(); drawGraph(); updateInspector();
    $('#time').textContent = `${t.toFixed(2)} s  ·  f${Math.round(t * R.fps)}`;
  }
  report(ms);
  requestAnimationFrame(loop);
}

// ---------- what an agent sees: the editor's state out, its moves in
// The server keeps the last state for `mui-cut mcp` (editor_state), and
// relays an agent's `control` messages (editor_goto) back here.
let reported = '', reportedAt = 0;
function report(ms) {
  if (!doc || ms - reportedAt < (playing ? 500 : 150)) return;
  const s = JSON.stringify({ scene: scene()?.name ?? null, scene_index: si, t: round(t), selection: sel, prop, playing, key: selKey ? { prop: selKey.p, t: selKey.k.t } : null });
  if (s === reported) return;
  reported = s; reportedAt = ms;
  fetch('/state', { method: 'PUT', body: s }).catch(() => {});
}
function control(m) {
  if (typeof m.scene === 'string') {
    const i = doc.scenes.findIndex(s => s.name === m.scene);
    if (i >= 0 && i !== si) { si = i; sel = null; selKey = null; }
  }
  if (typeof m.t === 'number') t = Math.max(0, Math.min(scene().duration, m.t));
  if (typeof m.playing === 'boolean') setPlaying(m.playing);
  if ('select' in m) {
    if (m.select === null || scene().layers.some(l => l.id === m.select)) select(m.select);
  }
  if (typeof m.prop === 'string' && layer() && numPaths(layer()).includes(m.prop)) prop = m.prop;
  refresh();
}
addEventListener('resize', () => { need = true; });

$('#file').textContent = await (await fetch('/name')).text();
await pull('loaded');
const events = new EventSource('/events');
events.onmessage = () => pull('reloaded from disk');
events.addEventListener('control', e => control(JSON.parse(e.data)));
requestAnimationFrame(loop);
