import { getp, isKeys, layer, numPaths, scene, selectedLayers } from './doc.js';
import { edit, showError, status } from './edit.js';
import { keyLayer } from './sound.js';
import { dropSource, refreshSources, sourceOf } from './sources.js';
import { $, KIND_ICON, S, cut, unfolded } from './state.js';
import { refresh } from './transport.js';

// ---------- scene and layer lists
export function refreshLists() {
  $('#scenes').replaceChildren(...S.doc.scenes.map((s, i) => {
    const b = document.createElement('button');
    const d = S.R.scenes[i].duration;
    b.textContent = `${s.name}  ·  ${d}s${s.mode === '3d' ? '  ·  3D' : ''}`;
    b.className = i === S.si ? 'on' : '';
    b.onclick = () => { S.si = i; S.sel = null; S.selKey = null; S.t = Math.min(S.t, d); refresh(); };
    return b;
  }));
  // Children under their parents, top of the paint order first.
  const ls = scene().layers;
  const rowsUnder = (parent, depth) => [...ls].reverse().filter(l => (l.parent ?? '') === parent).flatMap(l => {
    const b = document.createElement('button');
    b.innerHTML = `<span class="kind">${KIND_ICON[l.kind] ?? '?'}</span>`;
    b.append(l.name || l.id);
    b.className = S.selection.includes(l.id) ? 'on' : '';
    b.dataset.layer = l.id;
    b.style.paddingLeft = `${7 + depth * 14}px`;
    b.onclick = e => select(l.id, picking(e));
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
    const parts = [...new Set(S.quads.filter(q => q.id.startsWith(l.id + '#')).map(q => q.id.slice(l.id.length + 1)))];
    const partDepth = part => parts.filter(o => part.startsWith(o + '/')).length;
    // Under its parent, in the order the capture has them.
    const chain = part => [...parts.filter(o => part.startsWith(o + '/')), part].map(o => parts.indexOf(o));
    const cmp = (a, b) => { const x = chain(a), y = chain(b); for (let i = 0; i < Math.min(x.length, y.length); i++) if (x[i] !== y[i]) return x[i] - y[i]; return x.length - y.length; };
    parts.sort(cmp);
    return [b, ...parts.map(part => {
      const c = document.createElement('button');
      c.className = 'part' + (S.selection.includes(`${l.id}#${part}`) ? ' on' : '');
      c.dataset.part = part; c.dataset.depth = partDepth(part);
      c.style.paddingLeft = `${20 + 14 * (depth + partDepth(part))}px`;
      c.innerHTML = '<span class="kind">└</span>';
      c.append(part.slice(part.lastIndexOf('/') + 1));
      c.title = part;
      c.onclick = e => select(`${l.id}#${part}`, picking(e));
      return c;
    }), ...rowsUnder(l.id, depth + 1)];
  });
  $('#layers').replaceChildren(...rowsUnder('', 0));
  refreshSources();
}
// Drags within the editor: a source row, or a layer row.
export const DRAG_SOURCE = 'application/x-cut-source', DRAG_LAYER = 'application/x-cut-layer';
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
export function parentTo(id, parent) {
  const ls = scene().layers, i = ls.findIndex(l => l.id === id);
  if (i < 0 || (ls[i].parent ?? '') === parent) return;
  let json;
  try { json = cut.reparent(JSON.stringify(S.doc), S.si, id, parent, S.t); } catch (e) { showError(String(e)); return; }
  edit(() => { S.doc = JSON.parse(json); S.sel = id; S.selPart = null; S.selKey = null; });
}

// Shift or Ctrl (Cmd) adds to the selection, or takes out what is in it.
export const picking = e => e.shiftKey || e.ctrlKey || e.metaKey ? 'toggle' : undefined;
// `id` is a layer id, or `layer#part` for a plugin's part. `how` 'toggle'
// adds it to the selection (as the primary) or takes it out; else it is
// the whole selection (null: nothing).
export function select(id, how) {
  if (how === 'toggle' && id) setSelection(S.selection.includes(id) ? S.selection.filter(x => x !== id) : [...S.selection, id]);
  else setSelection(id ? [id] : []);
}
// Selecting a part tracks it (an empty `parts` entry), so it can be moved
// and keyed. The inspector and graph follow the primary, the last id.
export function setSelection(ids) {
  const was = S.selection.at(-1) ?? null;
  S.selection = [...new Set(ids)];
  if ((S.selection.at(-1) ?? null) !== was) S.selKey = null;
  const l = layer();
  if (l && S.selPart && !l.parts?.[S.selPart]) edit(() => { (l.parts ??= {})[S.selPart] = {}; });
  const nums = numPaths(l).filter(p => S.selPart ? p.startsWith(`parts.${S.selPart}.`) : !p.startsWith('parts.'));
  if (l && S.selPart) S.prop = nums.includes(S.prop) ? S.prop : `parts.${S.selPart}.x`;
  else if (l && !nums.includes(S.prop)) S.prop = 'x';
  if (l && !isKeys(getp(l, S.prop))) S.prop = nums.find(p => isKeys(getp(l, p))) ?? S.prop;
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
  S.doc.scenes.push({ name: `scene ${S.doc.scenes.length + 1}`, duration: 2, layers: [] });
  S.si = S.doc.scenes.length - 1; S.sel = null;
});
document.querySelectorAll('[data-add]').forEach(b => b.onclick = () => edit(() => {
  // A shader layer is a rect whose stack starts with a generator: the
  // rect is its mask.
  const name = b.dataset.add, kind = name === 'shader' ? 'rect' : name, ls = scene().layers;
  let n = 1; while (ls.some(l => l.id === `${name}${n}`)) n++;
  const [w, h] = S.R.size;
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
  ls.push(l); S.sel = l.id;
}));
// The selected layers one step up or down the paint order, as a block:
// nothing moves when the first in that direction is at the end.
function moveLayers(d) {
  const ls = scene().layers, on = new Set(selectedLayers().map(l => l.id));
  const idx = ls.map((l, i) => on.has(l.id) ? i : -1).filter(i => i >= 0);
  if (d > 0) idx.reverse();
  if (!idx.length || idx[0] + d < 0 || idx[0] + d >= ls.length) return;
  edit(() => { for (const i of idx) [ls[i], ls[i + d]] = [ls[i + d], ls[i]]; });
}
$('#layer-up').onclick = () => moveLayers(1);
$('#layer-down').onclick = () => moveLayers(-1);
// Deleting a parent hands its children to its nearest ancestor that stays,
// where they are.
export function deleteSelected() {
  const doomed = selectedLayers();
  if (!doomed.length) return;
  const gone = new Set(doomed.map(l => l.id)), by = new Map(scene().layers.map(l => [l.id, l]));
  const keeper = l => { let p = l.parent ?? ''; while (p && gone.has(p)) p = by.get(p)?.parent ?? ''; return p; };
  let next = structuredClone(S.doc);
  for (const o of scene().layers.filter(o => !gone.has(o.id) && gone.has(o.parent))) {
    const to = keeper(o);
    try { next = JSON.parse(cut.reparent(JSON.stringify(next), S.si, o.id, to, S.t)); }
    catch { const k = next.scenes[S.si].layers.find(k => k.id === o.id); if (to) k.parent = to; else delete k.parent; }
  }
  edit(() => {
    S.doc = next;
    scene().layers = scene().layers.filter(o => !gone.has(o.id));
    S.sel = null; S.selKey = null;
  });
}
$('#layer-del').onclick = deleteSelected;
// Copies of the selected layers, each just above its original, selected;
// a copy of a selected parent's child keeps to the parent's copy.
export function duplicateSelected() {
  const ls = selectedLayers();
  if (!ls.length) return;
  const all = scene().layers, ids = new Set(all.map(l => l.id)), to = new Map();
  for (const l of ls) {
    const stem = l.id.replace(/\d+$/, '') || 'layer';
    let n = 2; while (ids.has(stem + n)) n++;
    ids.add(stem + n); to.set(l.id, stem + n);
  }
  edit(() => {
    for (const l of ls) {
      const c = structuredClone(l);
      c.id = to.get(l.id);
      if (to.has(c.parent)) c.parent = to.get(c.parent);
      all.splice(all.indexOf(l) + 1, 0, c);
    }
    S.selection = ls.map(l => to.get(l.id)); S.selKey = null;
  });
}
export const selectAll = () => setSelection(scene().layers.map(l => l.id));

