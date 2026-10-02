// ---------- align and distribute: the selected layers' outlines (the
// quads the last frame drew) lined up on the selection's bounds, or the
// canvas; distribute evens the gaps between three or more.
import { roots, scene, selectedLayers } from './doc.js';
import { edit, status } from './edit.js';
import { shiftBy } from './gizmo.js';
import { $, S } from './state.js';

let toCanvas = false;
$('#align-canvas').onclick = () => {
  toCanvas = !toCanvas;
  $('#align-canvas').setAttribute('aria-pressed', toCanvas);
};
// [x0, y0, x1, y1] around a layer's outline.
function bounds(l) {
  const q = S.quads.find(q => q.id === l.id);
  if (!q) return null;
  const xs = q.pts.map(p => p[0]), ys = q.pts.map(p => p[1]);
  return [Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys)];
}
// The layers it moves (no child of another selected one), with bounds.
function picked() {
  if (scene()?.mode === '3d') { status('Align works in 2D scenes', true); return []; }
  return roots(selectedLayers()).map(l => ({ l, b: bounds(l) })).filter(o => o.b);
}
// `how`: left, hcenter, right, top, vmiddle, bottom.
export function align(how) {
  const os = picked();
  if (!os.length) return;
  const [W, H] = S.R.size;
  const t = toCanvas || os.length === 1 ? [0, 0, W, H]
    : [0, 1, 2, 3].map(i => (i < 2 ? Math.min : Math.max)(...os.map(o => o.b[i])));
  const d = ({ b }) => ({
    left: [t[0] - b[0], 0], right: [t[2] - b[2], 0], hcenter: [(t[0] + t[2] - b[0] - b[2]) / 2, 0],
    top: [0, t[1] - b[1]], bottom: [0, t[3] - b[3]], vmiddle: [0, (t[1] + t[3] - b[1] - b[3]) / 2],
  })[how];
  edit(() => shiftBy(os.map(o => o.l), os.map(d)));
}
// Even gaps along `axis` (0 across, 1 down) between the first and the last.
export function distribute(axis) {
  const os = picked().sort((a, b) => a.b[axis] + a.b[axis + 2] - b.b[axis] - b.b[axis + 2]);
  if (os.length < 3) { status('Distribute takes three or more layers', true); return; }
  const span = os.at(-1).b[axis + 2] - os[0].b[axis];
  const gap = (span - os.reduce((n, o) => n + o.b[axis + 2] - o.b[axis], 0)) / (os.length - 1);
  let at = os[0].b[axis];
  const ds = os.map(o => {
    const d = [0, 0];
    d[axis] = at - o.b[axis];
    at += o.b[axis + 2] - o.b[axis] + gap;
    return d;
  });
  edit(() => shiftBy(os.map(o => o.l), ds));
}
const ACT = { left: () => align('left'), hcenter: () => align('hcenter'), right: () => align('right'), top: () => align('top'),
  vmiddle: () => align('vmiddle'), bottom: () => align('bottom'), hspace: () => distribute(0), vspace: () => distribute(1) };
for (const b of document.querySelectorAll('[data-align]')) b.onclick = () => ACT[b.dataset.align]();
// Alt+A/H/D left, centre, right; Alt+W/V/S top, middle, bottom;
// Alt+Shift+H/V distribute. By key position (code), so Alt's characters
// on a Mac keyboard do not get in the way.
const KEYS = { KeyA: 'left', KeyH: 'hcenter', KeyD: 'right', KeyW: 'top', KeyV: 'vmiddle', KeyS: 'bottom' };
addEventListener('keydown', e => {
  if (!e.altKey || e.ctrlKey || e.metaKey || e.target.closest?.('input, select, textarea')) return;
  const how = e.shiftKey ? { KeyH: 'hspace', KeyV: 'vspace' }[e.code] : KEYS[e.code];
  if (!how) return;
  e.preventDefault();
  ACT[how]();
});
