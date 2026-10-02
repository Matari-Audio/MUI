import { getp, insertKey, isKeys, keyAt, now, roots, round, scene, selectedLayers, setValue, setp, snap } from './doc.js';
import { begin, changed, end, showError } from './edit.js';
import { exportMessage } from './export.js';
import { DRAG_SOURCE, picking, refreshLists, select, setSelection } from './lists.js';
import { dropSource } from './sources.js';
import { boundsOf, clearGuides, drawGuides, snapMove, snapPoint, targets } from './snap.js';
import { drawGizmo, gizmoBox, gizmoCursor, gizmoHit, moveGizmo, startGizmo } from './gizmo.js';
import { $, C, S, cut, pacing, worker } from './state.js';
import { refresh } from './transport.js';
import { openComp } from './tree.js';

// ---------- viewport
const view = $('#view'), over = $('#overlay');
const octx = over.getContext('2d');
let vw = 2, vh = 2;       // the viewport's pixel size; the worker owns the canvas
// The worker takes the canvas and says what it draws on; then its draws
// come back here.
export async function initViewport() {
  const canvas = view.transferControlToOffscreen();
  const renderer = new URLSearchParams(location.search).get('renderer');
  worker.postMessage({ type: 'init', canvas, renderer }, [canvas]);
  const { backend, adapter, engine } = await new Promise(ok => { worker.onmessage = e => ok(e.data); });
  $('#backend').textContent = backend;
  $('#backend').dataset.backend = backend;
  $('#backend').dataset.engine = engine;
  $('#backend').title = backend === 'CPU' ? 'Viewport on the CPU (Vello CPU): no WebGPU or WebGL2'
    : `Viewport on ${backend}, Vello ${engine} (${adapter})`;
  worker.onmessage = onDrawn;
}
function onDrawn({ data: m }) {
  if (m.type === 'progress' || m.type === 'exported' || m.type === 'export-error') { exportMessage(m); return; }
  if (m.type !== 'drawn') { if (m.error) showError(m.error); return; }
  S.drawing = false;
  if (S.playing && pacing.pending >= 0) {
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
    const partsBefore = S.quads.filter(q => q.id.includes('#')).map(q => q.id).join();
    S.quads = JSON.parse(m.quads);
    if (S.quads.filter(q => q.id.includes('#')).map(q => q.id).join() !== partsBefore) refreshLists();
  }
  $('#notice').hidden = !m.notice; $('#notice').textContent = m.notice ?? '';
  $('#orbit').hidden = scene()?.mode !== '3d' || !!m.notice;
  $('#beauty').hidden = $('#orbit').hidden;
  view.dataset.samples = beauty.n;   // the e2e watches it climb
  drawOverlay();
}
let hover = null, drag = null;
function layoutViewport() {
  const box = $('#stage').getBoundingClientRect(), [pw, ph] = S.R.size;
  const k = Math.min((box.width - 32) / pw, (box.height - 32) / ph);
  const cw = Math.max(1, Math.floor(pw * k)), ch = Math.max(1, Math.floor(ph * k));
  $('#frame').style.width = cw + 'px'; $('#frame').style.height = ch + 'px';
  vw = Math.max(2, Math.round(Math.min(pw, cw * devicePixelRatio))); vh = Math.max(2, Math.round(vw * ph / pw));
  const ow = Math.round(cw * devicePixelRatio), oh = Math.round(ch * devicePixelRatio);
  if (over.width !== ow || over.height !== oh) { over.width = ow; over.height = oh; }
}
// False while the worker is still busy: the loop asks again next frame, so
// only the newest playhead is ever drawn.
export function drawViewport() {
  layoutViewport();
  drawOverlay();
  if (S.drawing) return false;
  S.drawing = true;
  // A refining Beauty draw is the next sample; any other starts over.
  const sample = refining() ? beauty.n++ : undefined;
  worker.postMessage({ type: 'draw', si: S.si, t: S.t, w: vw, h: vh, sample });
  pacing.pending = S.playing ? Math.floor(S.t * S.R.fps + 1e-6) : -1;
  return true;
}
function drawOverlay() {
  const k = over.width / S.R.size[0];
  octx.clearRect(0, 0, over.width, over.height);
  const dpr = devicePixelRatio;
  const selId = S.selection.at(-1), box = gizmoBox();
  const marked = drag?.marquee ? marqueeHits(drag.marquee).map(id => [id, 1, C.hover]) : [];
  for (const [id, width, color] of [[hover, 1, C.hover], ...marked, ...S.selection.map(id => [id, 1.5, C.sel])]) {
    const q = S.quads.find(q => q.id === id);
    if (!q) continue;
    octx.beginPath();
    q.pts.forEach(([x, y], i) => i ? octx.lineTo(x * k, y * k) : octx.moveTo(x * k, y * k));
    octx.closePath();
    // A dark hairline under the light one keeps it legible on light scenes.
    octx.lineWidth = (width + 2) * dpr; octx.strokeStyle = C.halo; octx.stroke();
    octx.lineWidth = width * dpr; octx.strokeStyle = color; octx.stroke();
    if (id === selId && !box) for (const [x, y] of q.pts) { // corner squares mark the selection
      octx.fillStyle = C.halo; octx.fillRect(x * k - 4 * dpr, y * k - 4 * dpr, 8 * dpr, 8 * dpr);
      octx.fillStyle = C.sel; octx.fillRect(x * k - 3 * dpr, y * k - 3 * dpr, 6 * dpr, 6 * dpr);
    }
  }
  drawGizmo(octx, k, dpr, perPx());
  if (drag) drawGuides(octx, k, dpr);
  if (drag?.marquee) {
    const [[ax, ay], [bx, by]] = drag.marquee;
    octx.fillStyle = C.marquee; octx.fillRect(ax * k, ay * k, (bx - ax) * k, (by - ay) * k);
    octx.lineWidth = dpr; octx.strokeStyle = C.sel; octx.strokeRect(ax * k, ay * k, (bx - ax) * k, (by - ay) * k);
  }
}
// Project pixels per CSS pixel of the viewport.
const perPx = () => S.R.size[0] / (over.clientWidth || 1);
function toProject(e) {
  const r = over.getBoundingClientRect();
  return [(e.clientX - r.left) / r.width * S.R.size[0], (e.clientY - r.top) / r.height * S.R.size[1]];
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
const hit = p => [...S.quads].reverse().find(q => inside(p, q.pts))?.id ?? null;
// The orbit preview swings the shot camera about its target; the project
// never sees it.
const orbit = { on: false, yaw: 0, pitch: 0, zoom: 1, from: null };
// The Beauty preview: while paused on a 3D scene, each finished draw asks
// for the next sample, folded into the mean of the ones before, up to
// BEAUTY_MAX; any change of what is shown starts again from sample 0.
export const BEAUTY_MAX = 256;
export const beauty = { on: false, n: 0 };
export const refining = () => beauty.on && !S.playing && !$('#beauty').hidden;
$('#beauty').onclick = () => {
  beauty.on = !beauty.on;
  $('#beauty').setAttribute('aria-pressed', beauty.on); $('#beauty').classList.toggle('on', beauty.on);
  S.need = true;
};
const sendOrbit = () => { worker.postMessage({ type: 'orbit', ...orbit, from: undefined }); S.need = true; };
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
// In 3D: the plugin layer and UI point the pointer's ray meets, through
// the camera the viewport shows (`only`: stay on the slab a drag began on).
function pick3d([x, y], only) {
  if (scene()?.mode !== '3d') return null;
  const j = cut.pick(S.si, S.t, x, y, new Float64Array(orbit.on ? [orbit.yaw, orbit.pitch, orbit.zoom] : []), only ?? '');
  return j ? JSON.parse(j) : null;
}
globalThis.cutPick = (x, y) => pick3d([x, y]);
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
  S.interact = !S.interact;
  $('#interact').setAttribute('aria-pressed', S.interact); $('#interact').classList.toggle('on', S.interact);
};
over.onpointerdown = e => {
  if (orbit.on) { orbit.from = [e.clientX, e.clientY, orbit.yaw, orbit.pitch]; over.setPointerCapture(e.pointerId); return; }
  const p = toProject(e), id = hit(p);
  if (S.interact) {
    // A 2D quad maps to the UI itself; a 3D slab is found by its ray.
    const q = [...S.quads].reverse().find(q => q.ui && inside(p, q.pts));
    const h = q ? null : pick3d(p);
    const l = scene().layers.find(l => l.id === (q ? q.id.split('#')[0] : h?.layer));
    if (!l || l.kind !== 'plugin') return;
    const t0 = snap(S.t);
    begin(); pointerKeys(l, t0, q ? toUi(q, p) : h.ui, 1); changed();
    drag = { interact: true, l, q, slab: h?.slab, ui: h?.ui, t0, last: t0 };
    over.setPointerCapture(e.pointerId);
    return;
  }
  // A gizmo handle first; then, with several selected, their box moves
  // them from anywhere inside it.
  const gh = gizmoHit(p, perPx());
  if (gh) { drag = { gizmo: startGizmo(gh, p), snap: targets(S.selection, perPx()) }; over.setPointerCapture(e.pointerId); begin(); return; }
  const how = picking(e), box = !id && !how && gizmoBox();
  if (box && !box.one && inside(p, box.pts)) { drag = moving(p); over.setPointerCapture(e.pointerId); begin(); return; }
  if (!id) {
    // A marquee from empty space: Shift or Ctrl adds what it touches.
    if (!how && S.selection.length) select(null);
    drag = { marquee: [p, p], keep: [...S.selection] };
    over.setPointerCapture(e.pointerId);
    return;
  }
  // A click on what is selected keeps the selection, to move it all; a
  // click without a drag then selects just that.
  let collapse = null;
  if (how) { select(id, 'toggle'); if (!S.selection.includes(id)) return; }
  else if (!S.selection.includes(id)) select(id);
  else collapse = id;
  drag = { ...moving(p), collapse };
  over.setPointerCapture(e.pointerId);
  begin();
};
// The 2D map of a layer's parent from local offsets to project pixels (a
// parented layer moves in its parent's frame), from the evaluated frame.
function parentLin(l, frame) {
  const p = l.parent && scene().mode !== '3d' && frame?.layers.find(d => d.id === l.parent);
  if (!p) return [1, 0, 0, 1];
  const a = p.rotation * Math.PI / 180, k = p.scale;
  return [k * Math.cos(a), k * Math.sin(a), -k * Math.sin(a), k * Math.cos(a)];
}
// A part moves in its parent's frame (the plugin's, at the top): the
// parent quad's map from UI pixels to project pixels.
function partLin(l, part) {
  const mine = S.quads.filter(q => q.id.startsWith(l.id + '#')).map(q => q.id.slice(l.id.length + 1));
  const parent = mine.filter(o => part.startsWith(o + '/')).sort((a, b) => b.length - a.length)[0];
  const pq = S.quads.find(q => q.id === (parent ? `${l.id}#${parent}` : l.id));
  const k = now(l, 'scale') || 1;
  return pq?.ui ? pq.ui.slice(0, 4) : [k, 0, 0, k];
}
// A move from `p`: what it drags, the bounds it snaps and what to.
const moving = p => ({ p, items: moveItems(), moved: false, b0: boundsOf(S.selection), snap: targets(S.selection, perPx()) });
// What a move drags: each selected part, and each selected layer that no
// selected ancestor carries already, with where it starts.
function moveItems() {
  const frame = JSON.parse(cut.frame(S.si, S.t) || 'null');
  const ls = new Set(roots(selectedLayers()));
  const items = [];
  for (const id of S.selection) {
    const i = id.indexOf('#'), l = scene().layers.find(l => l.id === (i < 0 ? id : id.slice(0, i)));
    if (!l || (i < 0 && !ls.has(l))) continue;
    const px = i < 0 ? '' : `parts.${id.slice(i + 1)}.`;
    items.push({ l, px, lin: i < 0 ? parentLin(l, frame) : partLin(l, id.slice(i + 1)), x0: now(l, px + 'x'), y0: now(l, px + 'y') });
  }
  return items;
}
// Whole layers whose outline meets the marquee.
function marqueeHits([[ax, ay], [bx, by]]) {
  const [x0, x1, y0, y1] = [Math.min(ax, bx), Math.max(ax, bx), Math.min(ay, by), Math.max(ay, by)];
  return S.quads.filter(q => !q.id.includes('#') && q.pts.some(([x]) => x >= x0) && q.pts.some(([x]) => x <= x1)
    && q.pts.some(([, y]) => y >= y0) && q.pts.some(([, y]) => y <= y1)).map(q => q.id);
}
over.onpointermove = e => {
  if (orbit.on) {
    if (!orbit.from) return;
    const [x0, y0, yaw, pitch] = orbit.from;
    orbit.yaw = yaw - (e.clientX - x0) * 0.4; orbit.pitch = Math.max(-89, Math.min(89, pitch + (e.clientY - y0) * 0.4)); sendOrbit();
    return;
  }
  const p = toProject(e);
  if (!drag) {
    const gh = !S.interact && gizmoHit(p, perPx());
    if (gh) { over.style.cursor = gizmoCursor(gh); return; }
    const h = hit(p); if (h !== hover) { hover = h; S.need = true; }
    over.style.cursor = h ? (S.interact ? 'pointer' : 'move') : 'default'; return;
  }
  if (drag.interact) {
    drag.last = Math.max(snap(S.t), drag.t0 + 1 / S.R.fps);
    if (!drag.q) drag.ui = pick3d(p, drag.slab)?.ui ?? drag.ui;
    pointerKeys(drag.l, drag.last, drag.q ? toUi(drag.q, p) : drag.ui);
    changed(); return;
  }
  if (drag.marquee) { drag.marquee[1] = p; drawOverlay(); return; }
  const free = e.ctrlKey || e.metaKey;
  if (drag.gizmo) {
    // A scale handle on an upright box: the edges it drags snap.
    const g = drag.gizmo, h = g.h, upright = Math.abs(g.box.a) < 1e-6;
    const q = h.turn || !upright ? p
      : snapPoint(drag.snap, p, [h.at[0] + p[0] - g.p0[0], h.at[1] + p[1] - g.p0[1]], [h.u !== 0.5, h.v !== 0.5], free);
    moveGizmo(g, q, { shift: e.shiftKey, alt: e.altKey });
    changed(); drawOverlay(); return;
  }
  drag.moved = true;
  const d = snapMove(drag.snap, drag.b0, [p[0] - drag.p[0], p[1] - drag.p[1]], free);
  for (const it of drag.items) {
    const [dx, dy] = unmap(it.lin, d);
    setValue(it.l, it.px + 'x', round(it.x0 + dx));
    setValue(it.l, it.px + 'y', round(it.y0 + dy));
  }
  changed();
};
over.onpointerup = () => {
  if (orbit.on) { orbit.from = null; return; }
  if (drag?.interact) {
    const l = drag.l, up = Math.max(snap(S.t), drag.last + 1 / S.R.fps);
    holdKey(l, 'pointer_down', up, 0);
    changed();
  }
  const was = drag;
  drag = null;
  clearGuides(); drawOverlay();
  if (was?.marquee) { setSelection([...was.keep, ...marqueeHits(was.marquee)]); return; }
  end();
  if (was?.collapse && !was.moved) select(was.collapse);
};
// Double-click on a comp opens its scene.
over.ondblclick = e => {
  const id = hit(toProject(e));
  if (!S.interact && id && openComp(scene().layers.find(l => l.id === id.split('#')[0]))) refresh();
};
// A source dragged from the Sources panel lands where it is dropped.
over.addEventListener('dragover', e => { if (e.dataTransfer.types.includes(DRAG_SOURCE)) e.preventDefault(); });
over.addEventListener('drop', e => { e.preventDefault(); dropSource(e, toProject(e)); });

