// ---------- the layer hierarchy: the scene's layers as a tree (children
// under their parents, the top of the paint order first), which branches
// are folded (kept per project in localStorage, shared by the layer list
// and the timeline), and the edits that change the tree: reorder, group,
// ungroup and precompose. Every move goes through `Cut.reparent`, so a
// layer keeps its place on screen.
import { roots, round, scene, selectedLayers } from './doc.js';
import { edit, showError } from './edit.js';
import { boundsOf } from './snap.js';
import { $, S, cut } from './state.js';

// Folded layers, as `scene/id`, for the project open (by its file name).
let folded = null, foldedFor = '';
function foldSet() {
  const file = $('#file').textContent || '';
  if (folded && foldedFor === file) return folded;
  foldedFor = file;
  try { folded = new Set(JSON.parse(localStorage.getItem('mui-cut.folded:' + file)) ?? []); } catch { folded = new Set(); }
  return folded;
}
export const isFolded = id => foldSet().has(`${scene().name}/${id}`);
export function setFolded(id, on) {
  const f = foldSet(), k = `${scene().name}/${id}`;
  if (on) f.add(k); else f.delete(k);
  try { localStorage.setItem('mui-cut.folded:' + foldedFor, JSON.stringify([...f])); } catch { /* not kept */ }
  S.need = true;
}

// The rows to show: `{ l, depth, kids }`, depth first, each parent's
// children top of the paint order first; a folded layer's subtree left
// out (`all` keeps it).
export function treeRows(all = false) {
  const kids = new Map();
  for (const l of scene().layers) {
    const p = l.parent ?? '';
    if (!kids.has(p)) kids.set(p, []);
    kids.get(p).push(l);
  }
  const out = [];
  const walk = (p, depth) => {
    for (const l of [...(kids.get(p) ?? [])].reverse()) {
      const n = kids.get(l.id)?.length ?? 0;
      out.push({ l, depth, kids: n });
      if (n && (all || !isFolded(l.id))) walk(l.id, depth + 1);
    }
  };
  walk('', 0);
  return out;
}
// Every id under `id`, at any depth.
export function descendants(id, ls = scene().layers) {
  const out = new Set();
  for (let grew = true; grew;) {
    grew = false;
    for (const l of ls) if ((l.parent === id || out.has(l.parent)) && !out.has(l.id)) { out.add(l.id); grew = true; }
  }
  return out;
}
// `doc` with layer `id` attached to `parent` ('' detaches), where it is.
const reparent = (doc, id, parent) => JSON.parse(cut.reparent(JSON.stringify(doc), S.si, id, parent, S.t));
const freeId = (ls, stem) => { let n = 1; while (ls.some(l => l.id === `${stem}${n}`)) n++; return `${stem}${n}`; };

// Move layer `id` with its subtree above or below `target` (`where`
// 'above' | 'below': next to it among its siblings, in front of or
// behind its whole subtree in the paint order).
export function moveLayer(id, target, where) {
  if (id === target || descendants(id).has(target)) return;
  const by = new Map(scene().layers.map(l => [l.id, l]));
  const parent = by.get(target)?.parent ?? '';
  let doc = structuredClone(S.doc);
  try { if ((by.get(id)?.parent ?? '') !== parent) doc = reparent(doc, id, parent); } catch (e) { showError(String(e)); return; }
  const ls = doc.scenes[S.si].layers, block = new Set([id, ...descendants(id)]), theirs = new Set([target, ...descendants(target)]);
  const moved = ls.filter(l => block.has(l.id)), rest = ls.filter(l => !block.has(l.id));
  const at = rest.map((l, i) => theirs.has(l.id) ? i : -1).filter(i => i >= 0);
  rest.splice(where === 'above' ? Math.max(...at) + 1 : Math.min(...at), 0, ...moved);
  doc.scenes[S.si].layers = rest;
  edit(() => { S.doc = doc; S.selection = [id]; S.selKey = null; });
}

// Ctrl+G: a new group at the middle of the selection's outlines, under
// the selection's common parent, with the selected layers moved into it
// where they are. It sits in the paint order just above the topmost.
export function groupSelected() {
  const sel = roots(selectedLayers());
  if (!sel.length) return;
  const ls = scene().layers, parents = new Set(sel.map(l => l.parent ?? ''));
  const id = freeId(ls, 'group'), b = boundsOf(sel.map(l => l.id)), [W, H] = S.R.size;
  const [x, y] = b ? [(b[0] + b[2]) / 2, (b[1] + b[3]) / 2] : [W / 2, H / 2];
  let doc = structuredClone(S.doc);
  doc.scenes[S.si].layers.splice(Math.max(...sel.map(l => ls.indexOf(l))) + 1, 0, { id, kind: 'group', x: round(x), y: round(y) });
  try {
    if (parents.size === 1 && !parents.has('')) doc = reparent(doc, id, [...parents][0]);
    for (const l of sel) doc = reparent(doc, l.id, id);
  } catch (e) { showError(String(e)); return; }
  edit(() => { S.doc = doc; S.selection = [id]; S.selKey = null; });
}
// Ctrl+Shift+G: each selected group's children handed to its parent,
// where they are, and the group gone.
export function ungroupSelected() {
  const groups = selectedLayers().filter(l => l.kind === 'group');
  if (!groups.length) return;
  let doc = structuredClone(S.doc);
  const freed = [];
  try {
    for (const g of groups) {
      const of = () => doc.scenes[S.si].layers;
      const up = of().find(l => l.id === g.id)?.parent ?? '';
      for (const c of of().filter(l => l.parent === g.id)) { doc = reparent(doc, c.id, up); freed.push(c.id); }
      doc.scenes[S.si].layers = of().filter(l => l.id !== g.id);
    }
  } catch (e) { showError(String(e)); return; }
  edit(() => { S.doc = doc; S.selection = freed.filter(id => !groups.some(g => g.id === id)); S.selKey = null; });
}

