import { report } from './agent.js';
import { deleteKey, layer, snap, toggleKey } from './doc.js';
import { edit, load, noticeEffects, showError } from './edit.js';
import { drawGraph } from './graph.js';
import { refreshInspector, updateInspector } from './inspector.js';
import { deleteSelected, duplicateSelected, refreshLists, select, selectAll } from './lists.js';
import { transport } from './sound.js';
import { $, S, clock, pacing } from './state.js';
import { drawTimeline } from './timeline.js';
import { groupSelected, ungroupSelected } from './tree.js';
import { BEAUTY_MAX, beauty, drawViewport, refining } from './viewport.js';

// ---------- transport, keys, loop
export function setPlaying(on) {
  S.playing = on; $('#play').textContent = on ? '❚❚' : '▶';
  Object.assign(clock, { at: performance.now(), t: S.t });
  Object.assign(pacing, { shown: [], dropped: 0, last: -1 });
  transport();
}
// A seek while playing restarts the clock from the new playhead.
export function seek(time) { S.t = time; clock.at = performance.now(); clock.t = S.t; S.need = true; if (S.playing) transport(); }
export const toggle = (id, on) => { $(id).setAttribute('aria-pressed', String(on)); return on; };
$('#lock').onclick = () => { S.locked = toggle('#lock', !S.locked); seek(S.t); };
$('#hud-toggle').onclick = () => { $('#hud').hidden = !toggle('#hud-toggle', $('#hud').hidden); };
const quantile = (xs, q) => xs.length ? [...xs].sort((a, b) => a - b)[Math.min(xs.length - 1, Math.floor(q * xs.length))] : 0;
function drawHud() {
  const d = pacing.shown.slice(1).map((v, i) => v - pacing.shown[i]);
  const span = (pacing.shown.at(-1) - pacing.shown[0]) / 1000;
  $('#hud').textContent = `${d.length && span > 0 ? (d.length / span).toFixed(1) : '–'} fps  ${pacing.dropped} dropped\n`
    + `frame ${quantile(d, 0.5).toFixed(1)} / ${quantile(d, 0.95).toFixed(1)} ms p50/p95`;
}
$('#play').onclick = () => setPlaying(!S.playing);
addEventListener('keydown', e => {
  if (e.target.closest('input, select, textarea')) return;
  const mod = e.ctrlKey || e.metaKey;
  if (mod && e.key.toLowerCase() === 'z') { e.preventDefault(); $(e.shiftKey ? '#redo' : '#undo').click(); }
  else if (mod && e.key.toLowerCase() === 'y') { e.preventDefault(); $('#redo').click(); }
  else if (e.key === ' ') { e.preventDefault(); setPlaying(!S.playing); }
  else if (e.key === 'ArrowRight' || e.key === 'ArrowLeft') seek(snap(Math.max(0, Math.min(S.R.scenes[S.si].duration, S.t + (e.key === 'ArrowRight' ? 1 : -1) / S.R.fps))));
  else if (e.key === 'Home') seek(0);
  else if ((e.key === 'Delete' || e.key === 'Backspace') && S.selKey) edit(() => deleteKey(S.selKey));
  else if (e.key === 'Delete' || e.key === 'Backspace') deleteSelected();
  else if (mod && e.key.toLowerCase() === 'a') { e.preventDefault(); selectAll(); }
  else if (mod && e.key.toLowerCase() === 'd') { e.preventDefault(); duplicateSelected(); }
  else if (mod && e.key.toLowerCase() === 'g') { e.preventDefault(); if (e.shiftKey) ungroupSelected(); else groupSelected(); }
  else if (e.key === 'Escape') select(null);
  else if (e.key.toLowerCase() === 'k' && layer()) edit(() => toggleKey(layer(), S.prop));
});
// The header's variant switcher: hidden for a project without variants.
function refreshVariants() {
  const sel = $('#variant'), vs = S.doc.variants ?? [];
  sel.hidden = !vs.length;
  sel.replaceChildren(new Option('defaults', ''), ...vs.map(v => new Option(`${v.name}${v.size ? ` · ${v.size[0]}×${v.size[1]}` : ''}`, v.name)));
  sel.value = S.variant;
}
$('#variant').onchange = () => {
  S.variant = $('#variant').value;
  try { load(JSON.stringify(S.doc)); showError(''); } catch (e) { showError(String(e)); }
  S.t = Math.min(S.t, S.R.scenes[S.si].duration);
  refresh();
};
export function refresh() { refreshVariants(); refreshLists(); refreshInspector(); noticeEffects(); S.range = null; S.need = true; }
// rAF hands every callback of a display frame the same timestamp; the
// frame drawn now is presented about one display frame later, so that is
// the time it shows.
let vsync = 1000 / 60, lastMs = 0;
export function loop(ms) {
  if (lastMs) vsync += (Math.min(ms - lastMs, 100) - vsync) * 0.05;
  lastMs = ms;
  if (S.playing && S.doc) {
    const d = S.R.scenes[S.si].duration;
    let next = clock.t + (ms + vsync - clock.at) / 1000;
    if (next >= d) { next %= d; Object.assign(clock, { at: ms + vsync, t: next }); pacing.last = -1; }
    if (S.locked) next = Math.floor(next * S.R.fps + 1e-6) / S.R.fps;
    // On the grid, a frame is drawn once; off it, every display frame.
    if (next !== S.t || !S.locked) { S.t = next; S.need = true; }
  }
  if (!$('#hud').hidden) drawHud();
  if (S.need && S.doc) {
    S.need = false;
    beauty.n = 0;
    if (!drawViewport()) S.need = true;
    drawTimeline(); drawGraph(); updateInspector();
    $('#time').textContent = `${S.t.toFixed(2)} s  ·  f${Math.round(S.t * S.R.fps)}`;
  }
  else if (S.doc && !S.drawing && beauty.n > 0 && beauty.n < BEAUTY_MAX && refining()) drawViewport();
  report(ms);
  requestAnimationFrame(loop);
}

