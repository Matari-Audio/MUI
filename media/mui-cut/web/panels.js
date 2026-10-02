// ---------- panels: drag a gutter to resize the sources/layers column, the
// inspector or the timeline; double-click it for the default size. Sizes
// are CSS variables on :root, kept in localStorage (when it works).
import { S } from './state.js';

const KEY = 'mui-cut.panels', root = document.documentElement.style;
// The least each may be, and the most: a share of the window.
const LIMITS = { left: [150, 0.4], right: [190, 0.45], bottom: [120, 0.8] };
let sizes = {};
try { sizes = JSON.parse(localStorage.getItem(KEY)) ?? {}; } catch { /* storage off: defaults */ }
const clamp = (k, v) => {
  const [lo, share] = LIMITS[k];
  return Math.round(Math.max(lo, Math.min(v, (k === 'bottom' ? innerHeight - 140 : innerWidth) * share)));
};
// `null` is back to the stylesheet's default.
function size(k, v) {
  if (v == null) { delete sizes[k]; root.removeProperty('--' + k); }
  else { sizes[k] = clamp(k, v); root.setProperty('--' + k, sizes[k] + 'px'); }
  S.need = true;
}
const save = () => { try { localStorage.setItem(KEY, JSON.stringify(sizes)); } catch { /* not kept */ } };
for (const k of Object.keys(sizes)) if (k in LIMITS && typeof sizes[k] === 'number') size(k, sizes[k]); else delete sizes[k];
for (const g of document.querySelectorAll('[data-gutter]')) {
  const k = g.dataset.gutter;
  g.onpointerdown = e => { g.setPointerCapture(e.pointerId); g.classList.add('on'); e.preventDefault(); };
  g.onpointermove = e => {
    if (!g.hasPointerCapture(e.pointerId)) return;
    size(k, k === 'left' ? e.clientX : k === 'right' ? innerWidth - e.clientX : innerHeight - e.clientY);
  };
  g.onpointerup = () => { g.classList.remove('on'); save(); };
  g.ondblclick = () => { size(k, null); save(); };
}
