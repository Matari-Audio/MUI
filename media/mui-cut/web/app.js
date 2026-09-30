// mui-cut web editor. The project lives here as plain JSON; every edit goes
// through the WASM engine (`Cut.load`), which also draws the viewport and
// samples the graph editor's curves, so what you see is what `mui-cut render`
// writes. The viewport draws in worker.js (WebGPU or WebGL2 when the browser
// has it, else Vello CPU); this thread keeps a `Cut` for validation, the inspector
// and the graph's samples.
import init, { Cut } from './pkg/mui_cut.js';

const $ = s => document.querySelector(s);
const KIND_ICON = { rect: '▭', ellipse: '◯', text: 'T', image: '▣', path: '〰', duplicator: '⁂', svg: 'S', lottie: 'L', camera: '⌖', light: '☀', model: '◈', plugin: '⧉', audio: '♪', patch: '☰' };
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
let selPart = null;      // with a plugin layer: its selected part id
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
globalThis.cutQuads = () => quads; // the e2e aims at plugin parts with these
// A plugin layer's part tree at the playhead (plugin::tree_json): id (the
// path keyed as `parts.<id>.x`), surface, level, frame, rects, thumb (an
// asset path), motion, children. Null before its capture has loaded.
globalThis.cutParts = id => { const j = cut.plugin_parts(si, t, id); return j ? JSON.parse(j) : null; };
let interact = false;    // viewport clicks drive the plugin, keyed at the playhead
const undo = [], redo = [];
let base = null;         // the document before the gesture in progress
const assets = new Set();
const manifests = new Map(); // plugin capture manifests by path
const unfolded = new Set();  // open folders in the Sources tree: `id`, `id/part`
const sizes = new Map();     // imported images' natural sizes, by path
let sourceList = [];         // the Sources panel's rows: `Cut.sources()`
globalThis.cutSources = () => sourceList; // the e2e reads a plugin's capture path
globalThis.cutTime = () => t; // the e2e checks the playhead keeps with the audio clock

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
// The evaluated frame at the playhead: scene values (a bound background,
// effect stacks) that props() does not cover.
// Once per inspector pass: every effect field reads it.
let framed;
const frameNow = () => framed ??= JSON.parse(cut.frame(si, t) || 'null');
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
  b.title = any ? 'The CPU renderer draws the viewport without effects' : b.title;
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
// The files image, SVG, Lottie and model layers and 3D environments name,
// each sent to the viewport once.
async function loadAssets() {
  const files = doc.scenes.flatMap(s => s.layers)
    .filter(l => ['image', 'svg', 'lottie', 'model'].includes(l.kind)).map(l => l.path)
    .concat(doc.scenes.map(s => s.environment?.hdri))
    // Fonts: every font source, and text layers' fonts named by path.
    .concat((doc.sources ?? []).filter(m => m.kind === 'font').map(m => m.path))
    .concat(doc.scenes.flatMap(s => s.layers).filter(l => l.kind === 'text' && l.font && !doc.sources?.some(m => m.id === l.font)).map(l => l.font));
  for (const path of files) {
    if (!path || typeof path !== 'string' || assets.has(path)) continue;
    assets.add(path);
    try {
      const r = await fetch('/asset/' + path);
      if (r.ok) worker.postMessage({ type: 'asset', path, bytes: new Uint8Array(await r.arrayBuffer()) });
    } catch (e) { console.warn(path, e); }
  }
  // Plugin captures: every state the project shows, each manifest with
  // the images it names. A missing one is not captured yet; `serve` sends
  // `plugin` when it is.
  for (const path of JSON.parse(cut.plugin_states())) {
    if (assets.has(path)) continue;
    try {
      const r = await fetch('/asset/' + path);
      if (!r.ok) continue;
      const bytes = new Uint8Array(await r.arrayBuffer());
      cut.add_asset(path, bytes); // for the part trees (`cutParts`, the Sources panel)
      const man = JSON.parse(new TextDecoder().decode(bytes));
      manifests.set(path, man);
      for (const img of man.layers
        .flatMap(f => [f.src, f.free?.src]).filter(Boolean).map(s => '.cut-cache/' + s)) {
        if (assets.has(img)) continue;
        const ri = await fetch('/asset/' + img);
        if (!ri.ok) continue;
        assets.add(img);
        worker.postMessage({ type: 'asset', path: img, bytes: new Uint8Array(await ri.arrayBuffer()) });
      }
      assets.add(path);
      worker.postMessage({ type: 'asset', path, bytes });
    } catch (e) { console.warn(path, e); }
  }
  if (doc) refreshSources();
  need = true;
}

// ---------- scene and layer lists
function refreshLists() {
  $('#scenes').replaceChildren(...doc.scenes.map((s, i) => {
    const b = document.createElement('button');
    const d = R.scenes[i].duration;
    b.textContent = `${s.name}  ·  ${d}s${s.mode === '3d' ? '  ·  3D' : ''}`;
    b.className = i === si ? 'on' : '';
    b.onclick = () => { si = i; sel = null; selKey = null; t = Math.min(t, d); refresh(); };
    return b;
  }));
  // Children under their parents, top of the paint order first.
  const ls = scene().layers;
  const rowsUnder = (parent, depth) => [...ls].reverse().filter(l => (l.parent ?? '') === parent).flatMap(l => {
    const b = document.createElement('button');
    b.innerHTML = `<span class="kind">${KIND_ICON[l.kind] ?? '?'}</span>`;
    b.append(l.name || l.id);
    b.className = l.id === sel && !selPart ? 'on' : '';
    b.dataset.layer = l.id;
    b.style.paddingLeft = `${7 + depth * 14}px`;
    b.onclick = () => select(l.id);
    b.draggable = true;
    b.ondragstart = e => { e.dataTransfer.setData(DRAG_LAYER, l.id); e.dataTransfer.effectAllowed = 'move'; };
    b.ondragover = e => { if (dragged(e)) { e.preventDefault(); b.classList.add('drop-on'); } };
    b.ondragleave = () => b.classList.remove('drop-on');
    b.ondrop = e => {
      b.classList.remove('drop-on');
      const id = e.dataTransfer.getData(DRAG_LAYER);
      if (!id && !e.dataTransfer.getData(DRAG_SOURCE)) return;
      e.preventDefault(); e.stopPropagation();
      if (id) { if (id !== l.id) parentTo(id, l.id); return; }
      dropSource(e);
    };
    // A plugin's parts, as the last frame drew them: child layers, nested
    // by path (`osc`, then `osc/osc-shape` under it), parents first.
    const parts = [...new Set(quads.filter(q => q.id.startsWith(l.id + '#')).map(q => q.id.slice(l.id.length + 1)))];
    const partDepth = part => parts.filter(o => part.startsWith(o + '/')).length;
    // Under its parent, in the order the capture has them.
    const chain = part => [...parts.filter(o => part.startsWith(o + '/')), part].map(o => parts.indexOf(o));
    const cmp = (a, b) => { const x = chain(a), y = chain(b); for (let i = 0; i < Math.min(x.length, y.length); i++) if (x[i] !== y[i]) return x[i] - y[i]; return x.length - y.length; };
    parts.sort(cmp);
    return [b, ...parts.map(part => {
      const c = document.createElement('button');
      c.className = 'part' + (l.id === sel && part === selPart ? ' on' : '');
      c.dataset.part = part; c.dataset.depth = partDepth(part);
      c.style.paddingLeft = `${20 + 14 * (depth + partDepth(part))}px`;
      c.innerHTML = '<span class="kind">└</span>';
      c.append(part.slice(part.lastIndexOf('/') + 1));
      c.title = part;
      c.onclick = () => select(`${l.id}#${part}`);
      return c;
    }), ...rowsUnder(l.id, depth + 1)];
  });
  $('#layers').replaceChildren(...rowsUnder('', 0));
  refreshSources();
}
// Drags within the editor: a source row, or a layer row.
const DRAG_SOURCE = 'application/x-cut-source', DRAG_LAYER = 'application/x-cut-layer';
const dragged = e => e.dataTransfer.types.includes(DRAG_SOURCE) || e.dataTransfer.types.includes(DRAG_LAYER);
// The layer list's empty space: a layer dropped there leaves its parent;
// a source dropped anywhere on the list becomes a layer.
$('#layers').ondragover = e => { if (dragged(e)) e.preventDefault(); };
$('#layers').ondrop = e => {
  e.preventDefault();
  const id = e.dataTransfer.getData(DRAG_LAYER);
  if (id) parentTo(id, ''); else dropSource(e);
};
// Attach `id` to `parent` ('' detaches), kept where it is on screen in
// every variant: the engine rewrites its local transform (`Cut.reparent`).
function parentTo(id, parent) {
  const ls = scene().layers, i = ls.findIndex(l => l.id === id);
  if (i < 0 || (ls[i].parent ?? '') === parent) return;
  let json;
  try { json = cut.reparent(JSON.stringify(doc), si, id, parent, t); } catch (e) { showError(String(e)); return; }
  edit(() => { doc = JSON.parse(json); sel = id; selPart = null; selKey = null; });
}

// ---------- sources: imported files and plugins, a plugin a folder of its parts
const SOURCE_ICON = { image: '▣', svg: 'S', lottie: 'L', model: '◈', plugin: '⧉', font: 'Aa', audio: '♪' };
const sameSource = (m, rl) => m.kind === rl.kind && (m.kind === 'plugin'
  ? JSON.stringify(m.source) === JSON.stringify(rl.source) : m.path === rl.path);
// The source and part a layer shows: what the tree highlights for it.
function sourceOf(l) {
  const rl = R.scenes[si]?.layers.find(o => o.id === l.id);
  const m = rl && sourceList.find(m => sameSource(m, rl));
  if (!m) return null;
  return { m, part: selPart ?? (l.show?.length === 1 ? l.show[0] : null) };
}
function refreshSources() {
  sourceList = JSON.parse(cut.sources() || '[]');
  const cur = layer() && sourceOf(layer());
  const rows = [];
  const row = (m, node, depth) => {
    const part = node?.id ?? null, key = part ? `${m.id}/${part}` : m.id;
    const r = document.createElement('div');
    r.className = 'src-row' + (part ? ' part' : '') + (cur && cur.m.id === m.id && cur.part === part ? ' on' : '');
    r.style.paddingLeft = `${depth * 14}px`;
    r.draggable = true;
    r.dataset.source = m.id;
    if (part) r.dataset.part = part;
    r.title = part ? `${part}: drag onto the viewport or the layers to add just this part` : `${m.kind}: drag onto the viewport or the layers to add it`;
    const twist = document.createElement('button');
    twist.className = 'twist';
    const folder = m.kind === 'plugin' && (!node || node.children?.length);
    if (folder) {
      twist.textContent = unfolded.has(key) ? '▾' : '▸';
      twist.setAttribute('aria-expanded', unfolded.has(key));
      twist.title = unfolded.has(key) ? 'Collapse' : 'Expand';
      twist.onclick = e => { e.stopPropagation(); if (!unfolded.delete(key)) unfolded.add(key); refreshSources(); };
    }
    r.append(twist, thumb(m, node), label(part ? part.split('/').pop() : m.name ?? m.id, !m.listed && !part));
    r.onclick = () => pickSource(m, part);
    r.ondragstart = e => { e.dataTransfer.setData(DRAG_SOURCE, JSON.stringify({ id: m.id, part })); e.dataTransfer.effectAllowed = 'copy'; };
    rows.push(r);
    if (!folder || !unfolded.has(key)) return;
    // A plugin's home capture as `cutParts` has it: nodes id, thumb, children.
    const kids = node ? node.children : manifests.has(m.state) ? JSON.parse(cut.source_parts(m.state)) : null;
    if (!kids) {
      const n = document.createElement('div'); n.className = 'empty'; n.textContent = 'capturing its parts…'; rows.push(n); return;
    }
    if (!kids.length) { const n = document.createElement('div'); n.className = 'empty'; n.textContent = 'no parts'; rows.push(n); }
    for (const k of kids) row(m, k, depth + 1);
  };
  for (const m of sourceList) row({ ...m, listed: doc.sources?.some(s => s.id === m.id) }, null, 0);
  if (!sourceList.length) { const n = document.createElement('div'); n.className = 'empty'; n.textContent = 'Import files or add a plugin.'; rows.push(n); }
  $('#sources').replaceChildren(...rows);
}
function label(text, dim) {
  const s = document.createElement('span'); s.className = 'label' + (dim ? ' dim' : ''); s.textContent = text;
  if (dim) s.title = 'Used by a layer, not imported';
  return s;
}
// A source's thumbnail: the file itself, or the plugin's capture (the whole
// UI's backdrop, or the part tree node's own thumb).
function thumb(m, node) {
  let src = null;
  if (m.kind === 'image' || m.kind === 'svg') src = '/asset/' + m.path;
  else if (node?.thumb) src = '/asset/' + node.thumb;
  else if (m.kind === 'plugin' && !node) {
    const f = manifests.get(m.state)?.layers.find(f => f.group === 'background');
    if (f) src = '/asset/.cut-cache/' + f.src;
  }
  if (!src) { const s = document.createElement('span'); s.className = 'thumb'; s.textContent = SOURCE_ICON[m.kind] ?? '?'; return s; }
  const i = document.createElement('img'); i.className = 'thumb'; i.alt = ''; i.draggable = false; i.src = src;
  if (m.kind === 'image') i.onload = () => sizes.set(m.path, [i.naturalWidth, i.naturalHeight]);
  return i;
}
// Clicking a tree row selects what shows it: a component layer of that part,
// else the part on its whole plugin layer, else a layer of the file.
function pickSource(m, part) {
  const ls = scene().layers, rls = R.scenes[si].layers;
  const using = ls.filter((l, i) => sameSource(m, rls[i]));
  if (part) {
    const comp = using.find(l => l.show?.length === 1 && l.show[0] === part);
    if (comp) { select(comp.id); return; }
    const whole = using.find(l => !l.show?.length);
    if (whole) { select(`${whole.id}#${part}`); return; }
  }
  const l = using.find(l => !l.show?.length) ?? using[0];
  if (l) select(l.id);
}
// A layer from a dragged source (or one of a plugin's parts), dropped at
// project point `at`. A part is a plugin layer showing just it, where the
// whole plugin puts it (on the scene's whole plugin layer, if there is one).
function layerFrom(m, part, at) {
  const ls = scene().layers, [w, h] = R.size;
  const base = ((part ?? m.id).split('/').pop().replace(/\.\w+$/, '').replace(/[^\w-]+/g, '_')) || 'layer';
  let id = base, n = 2;
  while (ls.some(l => l.id === id)) id = `${base}${n++}`;
  const l = { id, kind: m.kind };
  if (m.kind === 'plugin') {
    l.source = m.source;
    const rls = R.scenes[si].layers;
    const whole = ls.find((o, i) => !o.show?.length && sameSource(m, rls[i]));
    if (whole) {
      for (const p of ['x', 'y', 'scale', 'rotation']) l[p] = round(now(whole, p));
      if (whole.parent) l.parent = whole.parent;
    } else Object.assign(l, { x: w / 2, y: h / 2 });
    // Two levels deep, like the Sources panel's capture: it draws at
    // once, and its controls are parts too.
    if (!whole) l.explode_levels = 2;
    if (part) { l.show = [part]; l.name = part; }
  } else if (m.kind === 'font') {
    // A font makes a text layer in it, by the source's id once imported.
    const font = doc.sources?.some(s => s.id === m.id) ? m.id : m.path;
    Object.assign(l, { kind: 'text', text: 'Text', font, x: round(at?.[0] ?? w / 2), y: round(at?.[1] ?? h / 2) });
  } else if (m.kind === 'audio') {
    l.path = m.path;
  } else {
    l.path = m.path;
    Object.assign(l, { x: round(at?.[0] ?? w / 2), y: round(at?.[1] ?? h / 2) });
    if (m.kind === 'image') { const [iw, ih] = sizes.get(m.path) ?? [200, 200]; Object.assign(l, { width: iw, height: ih }); }
    if (m.kind === 'model') Object.assign(l, { height: 200, fill: '#ffffff' });
  }
  edit(() => { ls.push(l); sel = l.id; selPart = null; selKey = null; });
  loadAssets();
  return l;
}
function dropSource(e, at) {
  const raw = e.dataTransfer.getData(DRAG_SOURCE);
  if (!raw) return;
  const { id, part } = JSON.parse(raw);
  const m = sourceList.find(m => m.id === id);
  if (m) layerFrom(m, part, at);
}
// Import: files from the button or dropped on the left panel are written
// beside the project (`media/`) and listed as sources.
const IMPORT_KIND = { png: 'image', svg: 'svg', json: 'lottie', glb: 'model', ttf: 'font', otf: 'font', wav: 'audio', mp3: 'audio', ogg: 'audio', flac: 'audio', m4a: 'audio' };
async function importFiles(files) {
  for (const f of files) {
    const kind = IMPORT_KIND[f.name.split('.').pop().toLowerCase()];
    if (!kind) { status(`${f.name}: import PNG, SVG, Lottie JSON, glTF (.glb), font (.ttf, .otf) or audio (.wav, .mp3, .ogg, .flac, .m4a) files`, true); continue; }
    const name = f.name.replace(/[^\w.-]+/g, '_'), path = 'media/' + name;
    const r = await fetch('/asset/' + path, { method: 'PUT', body: f });
    if (!r.ok) { status(`${f.name}: ${await r.text()}`, true); continue; }
    edit(() => {
      const list = doc.sources ??= [];
      if (list.some(s => s.path === path)) return;
      let id = name, n = 2;
      while (list.some(s => s.id === id)) id = `${name} ${n++}`;
      list.push({ id, kind, path });
    });
    status('imported ' + name);
  }
}
$('#import').onclick = () => $('#import-file').click();
$('#import-file').onchange = e => { importFiles([...e.target.files]); e.target.value = ''; };
const left = $('#left');
left.addEventListener('dragover', e => { if (e.dataTransfer.types.includes('Files')) { e.preventDefault(); left.classList.add('drop'); } });
left.addEventListener('dragleave', e => { if (!left.contains(e.relatedTarget)) left.classList.remove('drop'); });
left.addEventListener('drop', e => { left.classList.remove('drop'); if (e.dataTransfer.files.length) { e.preventDefault(); importFiles([...e.dataTransfer.files]); } });
// A file dropped anywhere else does not navigate away from the editor.
addEventListener('dragover', e => { if (e.dataTransfer.types.includes('Files')) e.preventDefault(); });
addEventListener('drop', e => e.preventDefault());
$('#sources-fold').onclick = () => {
  const b = $('#sources-fold'), open = b.getAttribute('aria-expanded') !== 'true';
  b.setAttribute('aria-expanded', open); b.textContent = open ? '▾' : '▸';
  $('#sources').hidden = !open;
};
// The plugin dialog: a plugin crate's folder or git URL (detected by the
// server), or a Cargo target (example or bin), or a prebuilt adapter.
$('#add-plugin').onclick = () => { $('#pl-error').textContent = ''; $('#plugin-dialog').showModal(); };
$('#pl-cancel').onclick = () => $('#plugin-dialog').close();
function addSource(id, source) {
  const list = doc.sources ?? [], base = id;
  let n = 2;
  while (list.some(s => s.id === id)) id = `${base} ${n++}`;
  edit(() => { (doc.sources ??= []).push({ id, kind: 'plugin', source }); });
  unfolded.add(id);
  $('#plugin-dialog').close();
  refreshSources();
}
$('#pl-add').onclick = async () => {
  const v = s => $(s).value.trim(), from = v('#pl-from'), cargo = v('#pl-cargo'), example = v('#pl-example'), bin = v('#pl-bin');
  const err = t => { $('#pl-error').textContent = t; };
  if (from) {
    err('Looking at the plugin…');
    const r = await fetch('/plugin', { method: 'POST', body: JSON.stringify({ from, id: v('#pl-id') }) });
    if (!r.ok) return err(await r.text());
    const { entry } = await r.json();
    return addSource(entry.id, entry.source);
  }
  const source = cargo ? (example ? { cargo, example } : { cargo, bin }) : { bin };
  if (cargo ? !example === !bin : !bin) return err('A plugin folder or git URL; or a Cargo.toml with an example or a bin name; or a binary alone.');
  addSource(v('#pl-id') || example || bin.split('/').pop() || 'plugin', source);
};

// `id` is a layer id, or `layer#part` for a plugin's part: selecting a
// part tracks it (an empty `parts` entry), so it can be moved and keyed.
function select(id) {
  const hash = id ? id.indexOf('#') : -1;
  const lid = hash < 0 ? id : id.slice(0, hash), part = hash < 0 ? null : id.slice(hash + 1);
  if (sel !== lid || selPart !== part) selKey = null;
  sel = lid; selPart = part;
  const l = layer();
  if (l && selPart && !l.parts?.[selPart]) edit(() => { (l.parts ??= {})[selPart] = {}; });
  const nums = numPaths(l).filter(p => selPart ? p.startsWith(`parts.${selPart}.`) : !p.startsWith('parts.'));
  if (l && selPart) prop = nums.includes(prop) ? prop : `parts.${selPart}.x`;
  else if (l && !nums.includes(prop)) prop = 'x';
  if (l && !isKeys(getp(l, prop))) prop = nums.find(p => isKeys(getp(l, p))) ?? prop;
  // The Sources tree opens down to what is selected.
  const cur = l && sourceOf(l);
  if (cur) {
    unfolded.add(cur.m.id);
    const steps = cur.part?.split('/') ?? [];
    for (let i = 1; i < steps.length; i++) unfolded.add(`${cur.m.id}/${steps.slice(0, i).join('/')}`);
  }
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
  else if (kind === 'patch') {
    const of = keyLayer();
    if (!of) { status('A patch view shows a plugin layer: add one first', true); return; }
    Object.assign(l, { of: of.id, width: 420, height: 520 });
  }
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
// Deleting a parent hands its children to its own parent, where they are.
$('#layer-del').onclick = () => {
  const l = layer();
  if (!l) return;
  let next = structuredClone(doc);
  for (const o of scene().layers.filter(o => o.parent === l.id)) {
    try { next = JSON.parse(cut.reparent(JSON.stringify(next), si, o.id, l.parent ?? '', t)); }
    catch { const k = next.scenes[si].layers.find(k => k.id === o.id); if (l.parent) k.parent = l.parent; else delete k.parent; }
  }
  edit(() => {
    doc = next;
    scene().layers = scene().layers.filter(o => o.id !== l.id);
    sel = null; selKey = null;
  });
};

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
    // '' is Inter; the rest are the font sources, by id.
    const fonts = ['', ...(doc.sources ?? []).filter(m => m.kind === 'font').map(m => m.id)];
    if (l.font && !fonts.includes(l.font)) fonts.push(l.font);
    const f = choice(l.font ?? '', fonts, v => { set('font', v, ''); loadAssets(); });
    f.options[0].textContent = 'Inter'; f.dataset.font = ''; f.title = 'The font: Inter, or a font source';
    field('font', f);
  }
  if (l.kind === 'patch') field('of', input(l.of, v => edit(() => { l.of = v; })));
  if (['image', 'svg', 'lottie', 'model', 'audio'].includes(l.kind)) field('path', input(l.path, v => edit(() => { l.path = v; loadAssets(); })));
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
  if (l.kind === 'plugin') {
    const json = (v, k) => { try { const o = JSON.parse(v); edit(() => { l[k] = o; }); } catch (e) { showError(`${k}: ${e}`); } };
    field('source', input(JSON.stringify(l.source), v => json(v, 'source'), 'area'));
    field('select', input((l.select ?? []).join(', '), v => set('select', v.split(',').map(s => s.trim()).filter(Boolean), undefined)));
    const b = document.createElement('button');
    b.textContent = 'Explode / collapse'; b.dataset.explode = '';
    b.title = 'Key explode at the playhead: 0.5 when collapsed, 0 when exploded';
    b.onclick = () => edit(() => setValue(l, 'explode', (now(l, 'explode') ?? 0) > 0.01 ? 0 : 0.5));
    field('parts', b);
    const lv = input(l.explode_levels ?? 1, v => set('explode_levels', Math.min(8, Math.max(1, Math.round(Number(v)) || 1)), 1), 'number');
    lv.dataset.levels = ''; lv.min = 1; lv.max = 8; lv.step = 1;
    lv.title = 'Explode levels: 1 the panels, 2 the panels and then their controls (captured this deep)';
    field('explode levels', lv);
    const st = input(l.explode_stagger ?? 0, v => set('explode_stagger', Math.max(0, Number(v) || 0), 0), 'number');
    st.title = 'Seconds each level of explode runs behind the one above';
    field('level stagger', st);
  }
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
// A 3D scene's environment light and ambient occlusion, each switched on by
// its box. Keyed or bound numbers show read-only; edit those in the JSON.
function sceneLook(s) {
  const toggle = (label, key, fresh) => {
    const c = document.createElement('input'); c.type = 'checkbox';
    c.checked = !!s[key]; c.dataset.look = key;
    c.onchange = () => edit(() => { if (c.checked) s[key] = fresh(); else delete s[key]; });
    field(label, c);
    return s[key];
  };
  const num = (o, k, dflt, label) => {
    const v = o[k] ?? dflt, fixed = typeof v === 'number';
    const i = lock(input(fixed ? v : 'keyed', x => edit(() => { o[k] = Number(x); }), fixed ? 'number' : 'text'), v);
    if (!fixed) i.disabled = true;
    field(label, i);
  };
  const e = toggle('environment', 'environment', () => ({}));
  if (e) {
    field('hdri', input(e.hdri ?? '', v => edit(() => { if (v) e.hdri = v; else delete e.hdri; loadAssets(); })));
    num(e, 'intensity', 1, 'intensity');
    num(e, 'rotation', 0, 'rotation');
    const b = document.createElement('input'); b.type = 'checkbox';
    b.checked = !!e.background; b.onchange = () => edit(() => { e.background = b.checked; });
    field('env background', b);
  }
  const a = toggle('occlusion', 'ao', () => ({}));
  if (a) { num(a, 'strength', 1, 'ao strength'); num(a, 'radius', 60, 'ao radius'); }
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
  framed = undefined;
  const box = $('#inspector'); box.replaceChildren();
  const l = layer();
  if (!l) {
    const s = scene();
    $('#insp-title').textContent = 'Scene';
    field('name', input(s.name, v => edit(() => { s.name = v; })));
    field('duration', lock(input(R.scenes[si].duration, v => edit(() => { s.duration = Math.max(0.05, Number(v) || 1); }), 'number'), s.duration));
    const bg = lock(input(frameNow()?.background ?? s.background ?? '#101014', v => edit(() => { s.background = v; })), s.background);
    bg.dataset.bg = '';
    field('background', bg);
    const mode = choice(s.mode ?? '2d', ['2d', '3d'], setMode);
    mode.dataset.mode = ''; mode.title = '3D keeps the layout (its default camera sees the 2D frame); back to 2D, layers go where the camera showed them';
    field('mode', mode);
    field('project', input(`${R.size[0]}×${R.size[1]} @ ${R.fps} fps`, () => {}));
    if (s.mode === '3d') sceneLook(s);
    variablesSection();
    fxSection(s, () => frameNow()?.effects ?? []);
    return;
  }
  $('#insp-title').textContent = selPart ? `Part · ${selPart}` : `Layer · ${l.kind}`;
  if (!selPart) {
    field('id', input(l.id, v => { if (v && !scene().layers.some(o => o.id === v)) edit(() => {
      for (const o of scene().layers) if (o.parent === l.id) o.parent = v;
      l.id = v; sel = v;
    }); }));
    const others = scene().layers.filter(o => o !== l).map(o => o.id);
    const par = choice(l.parent ?? '', ['', ...others], v => parentTo(l.id, v));
    par.dataset.parent = ''; par.title = 'Attach to another layer: it follows the parent, and keeps its place on screen now';
    field('parent', par);
    kindFields(l);
  }
  const reset = document.createElement('button');
  reset.textContent = 'Reset to default'; reset.dataset.reset = '';
  reset.title = selPart ? 'Back to where the plugin puts this part: its offsets and keys cleared' : 'Back to where it was placed: transform keys and offsets cleared';
  reset.onclick = () => resetSelected();
  field('layout', reset);
  let group = '';
  // A plugin's part rows show when that part is selected, and only then.
  const mine = p => selPart ? p.startsWith(`parts.${selPart}.`) : !p.startsWith('parts.');
  for (const { p, v } of propsOf(l).filter(r => mine(r.p))) {
    const parts = p.split('.');
    const g = parts.length > 1 ? parts.slice(0, 2).join('.') : '';
    if (g !== group) {
      group = g;
      if (parts[0] === 'parts') section(`Part ${selPart}`, [], () => edit(() => { delete l.parts[selPart]; if (!Object.keys(l.parts).length) delete l.parts; selPart = null; selKey = null; }));
      else if (parts[0] === 'params') { const q = l.params[+parts[1]]; section(`${q.id} · ${q.field}`, [], () => edit(() => { l.params.splice(+parts[1], 1); if (!l.params.length) delete l.params; selKey = null; })); }
      else if (g) groupHeader(l, parts[0], +parts[1]);
    }
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
  fxSection(l, () => frameNow()?.layers.find(d => d.id === l.id)?.effects ?? []);
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
// Reset to default: a part back where the plugin puts it, a layer back to
// its captured layout (`Cut.reset`).
function resetSelected() {
  const l = layer();
  if (!l) return;
  if (selPart) { edit(() => { (l.parts ??= {})[selPart] = {}; selKey = null; }); return; }
  let json;
  try { json = cut.reset(JSON.stringify(doc), si, l.id); } catch (e) { showError(String(e)); return; }
  edit(() => { doc = JSON.parse(json); selKey = null; });
}
// The scene's 2D/3D switch. Into 3D nothing moves: the default camera sees
// the z = 0 plane as the 2D frame. Out of 3D, layers go where the camera
// shows them at the playhead (`Cut.flatten`).
function setMode(v) {
  const s = scene();
  if ((s.mode ?? '2d') === v) return;
  if (v === '3d') { edit(() => { s.mode = '3d'; }); return; }
  let json;
  try { json = cut.flatten(JSON.stringify(doc), si, t); } catch (e) { showError(String(e)); return; }
  edit(() => { doc = JSON.parse(json); selKey = null; });
}
// Values follow the playhead without rebuilding the panel.
function updateInspector() {
  framed = undefined;
  const l = layer();
  if (!l) {
    const bg = $('#inspector [data-bg]');
    if (bg?.disabled) bg.value = frameNow()?.background ?? bg.value;
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
  if (m.error) showError(m.error);
  else {
    const partsBefore = quads.filter(q => q.id.includes('#')).map(q => q.id).join();
    quads = JSON.parse(m.quads);
    if (quads.filter(q => q.id.includes('#')).map(q => q.id).join() !== partsBefore) refreshLists();
  }
  $('#notice').hidden = !m.notice; $('#notice').textContent = m.notice ?? '';
  $('#orbit').hidden = scene()?.mode !== '3d' || !!m.notice;
  $('#beauty').hidden = $('#orbit').hidden;
  view.dataset.samples = beauty.n;   // the e2e watches it climb
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
  // A refining Beauty draw is the next sample; any other starts over.
  const sample = refining() ? beauty.n++ : undefined;
  worker.postMessage({ type: 'draw', si, t, w: vw, h: vh, sample });
  pacing.pending = playing ? Math.floor(t * R.fps + 1e-6) : -1;
  return true;
}
function drawOverlay() {
  const k = over.width / R.size[0];
  octx.clearRect(0, 0, over.width, over.height);
  const dpr = devicePixelRatio;
  const selId = selPart ? `${sel}#${selPart}` : sel;
  for (const [id, width, color] of [[hover, 1, C.hover], [selId, 1.5, C.sel]]) {
    const q = quads.find(q => q.id === id);
    if (!q) continue;
    octx.beginPath();
    q.pts.forEach(([x, y], i) => i ? octx.lineTo(x * k, y * k) : octx.moveTo(x * k, y * k));
    octx.closePath();
    // A dark hairline under the light one keeps it legible on light scenes.
    octx.lineWidth = (width + 2) * dpr; octx.strokeStyle = C.halo; octx.stroke();
    octx.lineWidth = width * dpr; octx.strokeStyle = color; octx.stroke();
    if (id === selId) for (const [x, y] of q.pts) { // corner squares mark the selection
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
// The Beauty preview: while paused on a 3D scene, each finished draw asks
// for the next sample, folded into the mean of the ones before, up to
// BEAUTY_MAX; any change of what is shown starts again from sample 0.
const BEAUTY_MAX = 256;
const beauty = { on: false, n: 0 };
const refining = () => beauty.on && !playing && !$('#beauty').hidden;
$('#beauty').onclick = () => {
  beauty.on = !beauty.on;
  $('#beauty').setAttribute('aria-pressed', beauty.on); $('#beauty').classList.toggle('on', beauty.on);
  need = true;
};
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
// A 2x2 linear map [a, b, c, d] (x' = a x + c y) inverted, applied to v.
const unmap = ([a, b, c, d], [x, y]) => { const k = a * d - b * c || 1; return [(d * x - c * y) / k, (a * y - b * x) / k]; };
// A project point in a plugin's own pixels, through the quad it hit (a
// moved part maps back to where the UI has it).
const toUi = (q, [x, y]) => unmap(q.ui, [x - q.ui[4], y - q.ui[5]]);
// Interact: the pointer, keyed as the plugin's `pointer_*` (hold keys) so
// the adapter turns the knob or drags the slider under it. Down at the
// playhead, moves a frame on (at least), up a frame after the last move.
function holdKey(l, p, time, v) {
  let cur = getp(l, p);
  if (!isKeys(cur)) { cur = [{ t: 0, v: cur ?? (p === 'pointer_down' ? 0 : -1), interp: 'hold' }]; setp(l, p, cur); }
  const i = keyAt(cur, time);
  if (i >= 0) Object.assign(cur[i], { v, interp: 'hold' }); else insertKey(cur, { t: time, v, interp: 'hold' });
}
function pointerKeys(l, time, [x, y], down) {
  holdKey(l, 'pointer_x', time, round(x)); holdKey(l, 'pointer_y', time, round(y));
  if (down !== undefined) holdKey(l, 'pointer_down', time, down);
}
$('#interact').onclick = () => {
  interact = !interact;
  $('#interact').setAttribute('aria-pressed', interact); $('#interact').classList.toggle('on', interact);
};
over.onpointerdown = e => {
  if (orbit.on) { orbit.from = [e.clientX, e.clientY, orbit.yaw, orbit.pitch]; over.setPointerCapture(e.pointerId); return; }
  const p = toProject(e), id = hit(p);
  if (interact) {
    const q = [...quads].reverse().find(q => q.ui && inside(p, q.pts));
    const l = q && scene().layers.find(l => l.id === q.id.split('#')[0]);
    if (!l || l.kind !== 'plugin') return;
    const t0 = snap(t);
    begin(); pointerKeys(l, t0, toUi(q, p), 1); changed();
    drag = { interact: true, l, q, t0, last: t0 };
    over.setPointerCapture(e.pointerId);
    return;
  }
  select(id);
  if (!id) return;
  const l = layer();
  // A part moves in its parent's frame (the plugin's, at the top): the
  // parent quad's map from UI pixels to project pixels, inverted.
  const px = selPart ? `parts.${selPart}.` : '';
  let lin = [1, 0, 0, 1];
  if (selPart) {
    const mine = quads.filter(q => q.id.startsWith(sel + '#')).map(q => q.id.slice(sel.length + 1));
    const parent = mine.filter(o => selPart.startsWith(o + '/')).sort((a, b) => b.length - a.length)[0];
    const pq = quads.find(q => q.id === (parent ? `${sel}#${parent}` : sel));
    const k = now(l, 'scale') || 1;
    lin = pq?.ui ? pq.ui.slice(0, 4) : [k, 0, 0, k];
  }
  drag = { p, l, px, lin, x0: now(l, px + 'x'), y0: now(l, px + 'y') };
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
  if (!drag) {
    const h = hit(p); if (h !== hover) { hover = h; need = true; }
    over.style.cursor = h ? (interact ? 'pointer' : 'move') : 'default'; return;
  }
  if (drag.interact) {
    drag.last = Math.max(snap(t), drag.t0 + 1 / R.fps);
    pointerKeys(drag.l, drag.last, toUi(drag.q, p));
    changed(); return;
  }
  const [dx, dy] = unmap(drag.lin, [p[0] - drag.p[0], p[1] - drag.p[1]]);
  setValue(drag.l, drag.px + 'x', round(drag.x0 + dx));
  setValue(drag.l, drag.px + 'y', round(drag.y0 + dy));
  changed();
};
over.onpointerup = () => {
  if (orbit.on) { orbit.from = null; return; }
  if (drag?.interact) {
    const l = drag.l, up = Math.max(snap(t), drag.last + 1 / R.fps);
    holdKey(l, 'pointer_down', up, 0);
    changed();
  }
  drag = null; end();
};
// A source dragged from the Sources panel lands where it is dropped.
over.addEventListener('dragover', e => { if (e.dataTransfer.types.includes(DRAG_SOURCE)) e.preventDefault(); });
over.addEventListener('drop', e => { e.preventDefault(); dropSource(e, toProject(e)); });

// ---------- timeline
const tl = $('#timeline'), tctx = tl.getContext('2d');
const LABEL = 150, RULER = 22, ROW = 20;
let tlRows = [], tlDrag = null;
const tlX = time => LABEL + time / R.scenes[si].duration * (tl.clientWidth - LABEL - 12);
const tlT = x => Math.max(0, Math.min(R.scenes[si].duration, (x - LABEL) / (tl.clientWidth - LABEL - 12) * R.scenes[si].duration));
// `animators.0.offset` as `a1 offset`, to fit the label column.
const short = p => p.replace(/^animators\.(\d+)\./, (_, i) => `a${+i + 1} `).replace(/^deformers\.(\d+)\./, (_, i) => `d${+i + 1} `)
  .replace(/^parts\.([^.]+)\./, '$1 ').replace(/^params\.(\d+)\.value$/, (_, i) => `param ${+i + 1}`);
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
    if (!r.p) drawSound(c, r.l, y);
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
  window.lastExport = { blob, frames: m.frames, codec: m.codec, sound: m.sound, hardware: m.hardware };   // e2e reads it
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = ($('#file').textContent || 'cut').replace(/\.cut\.json$|\.json$/, '') + '.mp4';
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 60_000);
  const sound = { 'mp4a.40.2': ', AAC sound', opus: ', Opus sound' }[m.sound] ?? '';
  $('#ex-status').textContent = `${m.frames} frames${sound}, ${(blob.size / 1e6).toFixed(1)} MB in ${secs} s`;
}

// ---------- transport, keys, loop
function setPlaying(on) {
  playing = on; $('#play').textContent = on ? '❚❚' : '▶';
  Object.assign(clock, { at: performance.now(), t });
  Object.assign(pacing, { shown: [], dropped: 0, last: -1 });
  transport();
}
// A seek while playing restarts the clock from the new playhead.
function seek(time) { t = time; clock.at = performance.now(); clock.t = t; need = true; if (playing) transport(); }
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
    beauty.n = 0;
    if (!drawViewport()) need = true;
    drawTimeline(); drawGraph(); updateInspector();
    $('#time').textContent = `${t.toFixed(2)} s  ·  f${Math.round(t * R.fps)}`;
  }
  else if (doc && !drawing && beauty.n > 0 && beauty.n < BEAUTY_MAX && refining()) drawViewport();
  report(ms);
  requestAnimationFrame(loop);
}

// ---------- sound: `serve` plays it, the playhead follows the device
// Play, pause and seek go to `/transport`; while playing, the clock is
// re-anchored to the audio clock (what is heard), so they never drift.
let audioAt = 0;
function transport() {
  fetch('/transport', { method: 'POST', body: JSON.stringify({ playing, t, scene: si }) })
    .then(r => r.ok ? r.json() : null).then(showTransport).catch(() => {});
}
function showTransport(r) {
  if (!r) return;
  $('#audio-state').textContent = `${r.device || 'no device'} · ${r.latency_ms.toFixed(0)} ms`;
  if (!playing || !r.playing) return;
  const ahead = clock.t + (performance.now() - clock.at) / 1000;
  // Small differences are the request's own time: only a real drift moves it.
  if (Math.abs(ahead - r.t) > 0.02) Object.assign(clock, { at: performance.now(), t: r.t });
}
setInterval(() => {
  if (!playing || performance.now() - audioAt < 250) return;
  audioAt = performance.now();
  fetch('/transport').then(r => r.ok ? r.json() : null).then(showTransport).catch(() => {});
}, 100);
// A plugin layer's UI as it plays: its live capture drawn instead of the
// one the playhead names (`live:<layer>`); null when playing stops.
const liveShown = new Map();
async function showLive(m) {
  const post = (path, bytes) => worker.postMessage({ type: 'asset', path, bytes });
  if (!m.state) { post('live:' + m.layer, new Uint8Array()); liveShown.delete(m.layer); need = true; return; }
  if (!playing) return;
  try {
    const path = `.cut-cache/${m.state}.json`;
    const r = await fetch('/asset/' + path);
    if (!r.ok) return;
    const bytes = new Uint8Array(await r.arrayBuffer());
    const man = JSON.parse(new TextDecoder().decode(bytes));
    for (const img of man.layers.flatMap(f => [f.src, f.free?.src]).filter(Boolean).map(s => '.cut-cache/' + s)) {
      if (assets.has(img)) continue;
      const ri = await fetch('/asset/' + img);
      if (!ri.ok) return;
      assets.add(img);
      post(img, new Uint8Array(await ri.arrayBuffer()));
    }
    if (!playing) return;
    post(path, bytes);
    post('live:' + m.layer, new TextEncoder().encode(m.state));
    // The one it replaces goes: a playing plugin makes many.
    const old = liveShown.get(m.layer);
    if (old) post(`.cut-cache/${old}.json`, new Uint8Array());
    liveShown.set(m.layer, m.state);
    need = true;
  } catch (e) { console.warn(e); }
}

// Keys: on-screen, the computer keyboard and MIDI play the selected plugin
// layer (else the scene's first); while playing, each note is recorded.
const KEYMAP = 'awsedftgyhujk';
let octave = 60, keysOn = false;
const held = new Map();
const keyLayer = () => (layer()?.kind === 'plugin' ? layer() : scene()?.layers.find(l => l.kind === 'plugin')) ?? null;
function playKey(note, on, vel = 100) {
  const l = keyLayer();
  if (!l) { status('Keys play a plugin layer: add one', true); return; }
  fetch('/live/note', { method: 'POST', body: JSON.stringify({ layer: l.id, note, on, velocity: vel }) }).catch(() => {});
  $(`#keys [data-note="${note}"]`)?.classList.toggle('down', on);
  if (on) { held.set(note, { t, vel, l }); return; }
  const h = held.get(note);
  held.delete(note);
  if (!h || !playing) return;
  const dur = Math.max(1 / R.fps, t >= h.t ? t - h.t : R.scenes[si].duration - h.t);
  edit(() => {
    h.l.notes = [...(h.l.notes ?? []), { t: round(h.t), dur: round(dur), pitch: note, ...(h.vel !== 100 && { vel: h.vel }) }]
      .sort((a, b) => a.t - b.t || a.pitch - b.pitch);
  });
}
function drawKeys() {
  const k = $('#keys');
  k.replaceChildren(...Array.from({ length: 25 }, (_, i) => {
    const note = octave - 12 + i, b = document.createElement('button');
    b.dataset.note = note;
    b.className = [1, 3, 6, 8, 10].includes(note % 12) ? 'black' : '';
    b.title = `MIDI ${note}`;
    b.onpointerdown = e => { b.setPointerCapture(e.pointerId); playKey(note, true); };
    b.onpointerup = () => playKey(note, false);
    return b;
  }));
}
$('#keys-toggle').onclick = async () => {
  keysOn = toggle('#keys-toggle', !keysOn);
  $('#keys').hidden = !keysOn;
  if (!keysOn) return;
  drawKeys();
  try {
    const midi = await navigator.requestMIDIAccess?.();
    for (const input of midi?.inputs.values() ?? []) {
      input.onmidimessage = ({ data: [s, n, v] }) => {
        if ((s & 0xf0) === 0x90 && v > 0) playKey(n, true, v);
        else if ((s & 0xf0) === 0x80 || (s & 0xf0) === 0x90) playKey(n, false);
      };
    }
  } catch (e) { console.warn('MIDI', e); }
};
addEventListener('keydown', e => {
  if (!keysOn || e.repeat || e.ctrlKey || e.metaKey || e.target.closest('input, select, textarea')) return;
  const k = e.key.toLowerCase(), i = KEYMAP.indexOf(k);
  if (k === 'z' || k === 'x') { octave = Math.max(24, Math.min(96, octave + (k === 'z' ? -12 : 12))); drawKeys(); }
  else if (i >= 0) playKey(octave + i, true);
  else return;
  e.preventDefault(); e.stopImmediatePropagation();
}, true);
addEventListener('keyup', e => {
  const i = KEYMAP.indexOf(e.key.toLowerCase());
  if (keysOn && i >= 0) playKey(octave + i, false);
}, true);

// Audio layers' waveforms for the timeline, decoded once by the browser.
const waves = new Map();
function wave(path) {
  if (waves.has(path)) return waves.get(path);
  waves.set(path, null);
  fetch('/asset/' + path).then(r => r.arrayBuffer())
    .then(b => new OfflineAudioContext(1, 1, 48000).decodeAudioData(b))
    .then(a => {
      const d = a.getChannelData(0), n = Math.ceil(a.duration * 100), peaks = new Float32Array(n);
      for (let i = 0; i < d.length; i++) { const j = Math.floor(i / a.sampleRate * 100); peaks[j] = Math.max(peaks[j], Math.abs(d[i])); }
      waves.set(path, peaks); need = true;
    }).catch(() => {});
  return null;
}
// A layer row's sound: a plugin's notes as bars by pitch, an audio
// layer's waveform (from `time` into the file).
function drawSound(c, l, y) {
  if (l.kind === 'plugin' && l.notes?.length) {
    const ps = l.notes.map(n => n.pitch), lo = Math.min(...ps), span = Math.max(1, Math.max(...ps) - lo);
    c.fillStyle = C.layerKey;
    for (const n of l.notes) {
      const x0 = tlX(n.t), x1 = tlX(n.t + n.dur);
      c.fillRect(x0, y + ROW - 4 - (n.pitch - lo) / span * (ROW - 8), Math.max(1, x1 - x0), 2);
    }
  } else if (l.kind === 'audio') {
    const peaks = wave(l.path);
    if (!peaks) return;
    const off = typeof l.time === 'number' ? l.time : 0;
    c.fillStyle = C.tick;
    for (let x = tlX(0); x < tlX(R.scenes[si].duration); x++) {
      const p = peaks[Math.floor((tlT(x) + off) * 100)] ?? 0;
      c.fillRect(x, y + ROW / 2 - p * ROW / 2, 1, Math.max(1, p * ROW));
    }
  }
}

// ---------- what an agent sees: the editor's state out, its moves in
// The server keeps the last state for `mui-cut mcp` (editor_state), and
// relays an agent's `control` messages (editor_goto) back here.
let reported = '', reportedAt = 0;
function report(ms) {
  if (!doc || ms - reportedAt < (playing ? 500 : 150)) return;
  const s = JSON.stringify({ scene: scene()?.name ?? null, scene_index: si, t: round(t), selection: selPart ? `${sel}#${selPart}` : sel, prop, playing, key: selKey ? { prop: selKey.p, t: selKey.k.t } : null });
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
    if (m.select === null || scene().layers.some(l => l.id === m.select.split('#')[0])) select(m.select);
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
// `serve` finished capturing plugin states: fetch the new ones.
events.addEventListener('plugin', () => loadAssets());
events.addEventListener('live', e => showLive(JSON.parse(e.data)));
requestAnimationFrame(loop);
