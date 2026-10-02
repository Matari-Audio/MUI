import { getp, layer, scene } from './doc.js';
import { applyAll, at, clone, diff, notice, plain, same, touch, where, who } from './patch.js';
import { refreshSources } from './sources.js';
import { $, S, assets, cut, manifests, me, redo, undo, worker } from './state.js';
import { refresh } from './transport.js';

// ---------- edits, undo, save
export function begin() { if (S.base === null) S.base = JSON.stringify(S.doc); }
export function changed() {
  const json = JSON.stringify(S.doc);
  try { load(json); showError(''); } catch (e) { showError(String(e)); }
  S.need = true;
}
// Hand `json` to the engine and the viewport, as the chosen variant.
export function load(json) {
  cut.load(json, S.variant || undefined);
  S.R = JSON.parse(cut.resolved());
  worker.postMessage({ type: 'load', json, variant: S.variant });
}
export function end() {
  if (S.base === null) return;
  const now = JSON.stringify(S.doc);
  if (now !== S.base) { undo.push({ a: S.base, b: now }); redo.length = 0; save(); }
  S.base = null;
  refresh();
}
export function edit(fn) { begin(); fn(); changed(); end(); }
// The CPU viewport draws without effects: say so rather than look wrong.
export function noticeEffects() {
  const b = $('#backend');
  if (b.dataset.backend !== 'CPU') return;
  const any = S.doc.scenes.some(s => s.effects?.length || s.layers.some(l => l.effects?.length));
  b.textContent = any ? 'CPU · effects off' : 'CPU';
  b.title = any ? 'The CPU renderer draws the viewport without effects' : b.title;
}
// Replay the step from `from` to `to` (documents) onto the current one,
// skipping fields somebody else has changed since.
function step(from, to) {
  const kept = [];
  for (const o of diff(from, to)) {
    if (o.op !== 'add' && !same(at(S.doc, o.path), at(from, o.path))) kept.push(o.path);
    else if (applyAll(S.doc, [o]).length) kept.push(o.path);
  }
  if (kept.length) notice(`changed since, left as is: ${where(kept)}`);
  S.selKey = null;
  changed(); save(); refresh();
}
$('#undo').onclick = () => { const e = undo.pop(); if (e) { redo.push(e); step(JSON.parse(e.b), JSON.parse(e.a)); } };
$('#redo').onclick = () => { const e = redo.pop(); if (e) { undo.push(e); step(JSON.parse(e.a), JSON.parse(e.b)); } };

// Saving sends what changed since the server's revision as a patch; the
// reply is the merged revision, adopted like any other.
let saveTimer = 0, sending = false, again = false;
function save() {
  clearTimeout(saveTimer);
  status('unsaved');
  saveTimer = setTimeout(send, 200);
}
async function send() {
  if (sending) { again = true; return; }
  const ops = diff(S.server, plain(S.doc));
  if (!ops.length) { status('saved'); return; }
  sending = true;
  try {
    const r = await fetch('/patch', { method: 'POST', body: JSON.stringify({ base: S.rev, ops, by: me }) });
    if (!r.ok) { status('not saved: ' + await r.text(), true); return; }
    await adopt({ ...await r.json(), by: me });
    status(diff(S.server, plain(S.doc)).length ? 'unsaved' : 'saved');
  } catch (e) {
    status('not saved: ' + e, true);
  } finally {
    sending = false;
    if (again) { again = false; send(); }
  }
}
export function status(s, bad = false) { $('#status').textContent = s; $('#status').classList.toggle('bad', bad); }
export function showError(s) { $('#error').hidden = !s; $('#error').textContent = s; }

// A new revision from the server: an agent's edit, an outside write to the
// file, another editor's, or ours (its event or the reply, whichever
// comes first). This editor's edits not in it yet (a drag in progress, a
// save in flight) stay on top of it, and the difference is applied to the
// document in place, so a drag keeps the layer it holds. A field both
// changed is noticed.
export async function adopt(m, why) {
  if (m.rev <= S.rev) return;
  const was = S.variant;
  if (!m.doc.variants?.some(v => v.name === S.variant)) S.variant = '';
  const mine = S.doc ? diff(S.server, plain(S.doc)) : [];
  const next = clone(m.doc);
  applyAll(next, mine);
  try { load(JSON.stringify(next)); } catch (e) { S.variant = was; status('the file on disk has an error: ' + e, true); return; }
  const both = [...(m.conflicts ?? [])];
  if (S.doc && m.by !== me) {
    const paths = mine.map(o => o.path);
    both.push(...diff(S.server, m.doc).map(o => o.path).filter(p => paths.some(q => touch(p, q))));
  }
  if (S.doc) {
    applyAll(S.doc, diff(plain(S.doc), next));
    if (S.base !== null) { const b = clone(m.doc); applyAll(b, diff(S.server, JSON.parse(S.base))); S.base = JSON.stringify(b); }
  } else S.doc = next;
  S.server = m.doc; S.rev = m.rev;
  if (both.length) notice(m.by === me ? `your edit replaced a newer change to ${where(both)}` : `${who(m.by)} changed ${where(both)} too; the later edit wins`);
  S.si = Math.min(S.si, S.doc.scenes.length - 1);
  if (!layer()) S.sel = null;
  if (S.selKey && !(scene().layers.includes(S.selKey.l) && getp(S.selKey.l, S.selKey.p)?.includes?.(S.selKey.k))) S.selKey = null;
  S.need = true;
  await loadAssets();
  if (why) status(why);
  if (S.base === null) refresh();
}
// The files image, SVG, Lottie and model layers and 3D environments name,
// each sent to the viewport once.
export async function loadAssets() {
  const files = S.doc.scenes.flatMap(s => s.layers)
    .filter(l => ['image', 'svg', 'lottie', 'model'].includes(l.kind)).map(l => l.path)
    .concat(S.doc.scenes.map(s => s.environment?.hdri))
    // Fonts: every font source, and text layers' fonts named by path.
    .concat((S.doc.sources ?? []).filter(m => m.kind === 'font').map(m => m.path))
    .concat(S.doc.scenes.flatMap(s => s.layers).filter(l => l.kind === 'text' && l.font && !S.doc.sources?.some(m => m.id === l.font)).map(l => l.font));
  await Promise.all(files.filter(p => p && typeof p === 'string' && !assets.has(p)).map(fetchAsset));
  loadCaptures(); // not waited for: the editor runs while they arrive
  if (S.doc) refreshSources();
  S.need = true;
}
// Plugin captures: every state the project shows that `serve` has (a
// plugin layer draws a placeholder until then), each manifest after the
// images it names. `plugin` events name the ones written since.
async function loadCaptures() {
  // In the order `serve` named them: nearest the playhead first.
  const shows = new Set(JSON.parse(cut.plugin_states()));
  const want = [...captured].filter(p => shows.has(p) && !assets.has(p));
  await pool(want, 8, async path => {
    assets.add(path);
    try {
      const r = await fetch('/asset/' + path);
      if (!r.ok) { assets.delete(path); return; }
      const bytes = new Uint8Array(await r.arrayBuffer());
      const man = JSON.parse(new TextDecoder().decode(bytes));
      await Promise.all(man.layers.flatMap(f => [f.src, f.free?.src]).filter(Boolean)
        .map(s => fetchAsset('.cut-cache/' + s)));
      cut.add_asset(path, bytes); // for the part trees (`cutParts`, the Sources panel)
      manifests.set(path, man);
      worker.postMessage({ type: 'asset', path, bytes });
      S.need = true;
    } catch (e) { assets.delete(path); console.warn(path, e); }
  });
  if (S.doc) refreshSources();
  S.need = true;
}
// The plugin states `serve` has written, by manifest path.
export const captured = new Set();
// A file sent to the viewport once; a second ask waits for the first.
const fetching = new Map();
function fetchAsset(path) {
  if (!fetching.has(path)) {
    assets.add(path);
    fetching.set(path, fetch('/asset/' + path)
      .then(async r => { if (r.ok) worker.postMessage({ type: 'asset', path, bytes: new Uint8Array(await r.arrayBuffer()) }); })
      .catch(e => console.warn(path, e)));
  }
  return fetching.get(path);
}
// `f` over `items`, `n` at a time.
async function pool(items, n, f) {
  let i = 0;
  await Promise.all(Array.from({ length: n }, async () => { while (i < items.length) await f(items[i++]); }));
}

