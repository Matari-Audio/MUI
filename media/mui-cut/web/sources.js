import { layer, now, round, scene } from './doc.js';
import { edit, loadAssets, status } from './edit.js';
import { DRAG_SOURCE, select } from './lists.js';
import { $, S, cut, manifests, sizes, unfolded } from './state.js';

// ---------- sources: imported files and plugins, a plugin a folder of its parts
const SOURCE_ICON = { image: '▣', svg: 'S', lottie: 'L', model: '◈', plugin: '⧉', font: 'Aa', audio: '♪' };
const sameSource = (m, rl) => m.kind === rl.kind && (m.kind === 'plugin'
  ? JSON.stringify(m.source) === JSON.stringify(rl.source) : m.path === rl.path);
// The source and part a layer shows: what the tree highlights for it.
export function sourceOf(l) {
  const rl = S.R.scenes[S.si]?.layers.find(o => o.id === l.id);
  const m = rl && S.sourceList.find(m => sameSource(m, rl));
  if (!m) return null;
  return { m, part: S.selPart ?? (l.show?.length === 1 ? l.show[0] : null) };
}
export function refreshSources() {
  S.sourceList = JSON.parse(cut.sources() || '[]');
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
  for (const m of S.sourceList) row({ ...m, listed: S.doc.sources?.some(s => s.id === m.id) }, null, 0);
  if (!S.sourceList.length) { const n = document.createElement('div'); n.className = 'empty'; n.textContent = 'Import files or add a plugin.'; rows.push(n); }
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
  const ls = scene().layers, rls = S.R.scenes[S.si].layers;
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
  const ls = scene().layers, [w, h] = S.R.size;
  const base = ((part ?? m.id).split('/').pop().replace(/\.\w+$/, '').replace(/[^\w-]+/g, '_')) || 'layer';
  let id = base, n = 2;
  while (ls.some(l => l.id === id)) id = `${base}${n++}`;
  const l = { id, kind: m.kind };
  if (m.kind === 'plugin') {
    l.source = m.source;
    const rls = S.R.scenes[S.si].layers;
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
    const font = S.doc.sources?.some(s => s.id === m.id) ? m.id : m.path;
    Object.assign(l, { kind: 'text', text: 'Text', font, x: round(at?.[0] ?? w / 2), y: round(at?.[1] ?? h / 2) });
  } else if (m.kind === 'audio') {
    l.path = m.path;
  } else {
    l.path = m.path;
    Object.assign(l, { x: round(at?.[0] ?? w / 2), y: round(at?.[1] ?? h / 2) });
    if (m.kind === 'image') { const [iw, ih] = sizes.get(m.path) ?? [200, 200]; Object.assign(l, { width: iw, height: ih }); }
    if (m.kind === 'model') Object.assign(l, { height: 200, fill: '#ffffff' });
  }
  edit(() => { ls.push(l); S.sel = l.id; S.selPart = null; S.selKey = null; });
  loadAssets();
  return l;
}
export function dropSource(e, at) {
  const raw = e.dataTransfer.getData(DRAG_SOURCE);
  if (!raw) return;
  const { id, part } = JSON.parse(raw);
  const m = S.sourceList.find(m => m.id === id);
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
      const list = S.doc.sources ??= [];
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
  const list = S.doc.sources ?? [], base = id;
  let n = 2;
  while (list.some(s => s.id === id)) id = `${base} ${n++}`;
  edit(() => { (S.doc.sources ??= []).push({ id, kind: 'plugin', source }); });
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

