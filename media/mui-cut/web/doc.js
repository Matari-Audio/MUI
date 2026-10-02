import { S, cut } from './state.js';

// ---------- document helpers
export const scene = () => S.doc.scenes[S.si];
export const layer = () => scene()?.layers.find(l => l.id === S.sel) ?? null;
export const isKeys = v => Array.isArray(v);
// `{"var": ...}`: a value a variable decides; the inspector shows it, the
// Variables panel changes it.
export const isBind = v => v !== null && typeof v === 'object' && !Array.isArray(v) && 'var' in v;
const bound = (o, p) => { const v = getp(o, p); return isBind(v) || (isKeys(v) && v.some(k => isBind(k.v))); };
export const snap = x => Math.round(x * S.R.fps) / S.R.fps;
export const round = v => Math.round(v * 1000) / 1000;
export const keyAt = (keys, time) => keys.findIndex(k => Math.abs(k.t - time) < 0.25 / S.R.fps);
// Properties are addressed by path: `x`, `fill`, `animators.0.offset`.
export const getp = (o, p) => p.split('.').reduce((o, k) => o?.[k], o);
export function setp(o, p, v) {
  const ks = p.split('.'), last = ks.pop(), parent = ks.reduce((o, k) => o[k], o);
  parent[last] = v;
}
// The engine says which properties a layer has and what they are at the
// playhead: `[{p, v}]`, v a number or a colour string.
// The evaluated frame at the playhead: scene values (a bound background,
// effect stacks) that props() does not cover.
// Once per inspector pass: every effect field reads it.

export const frameNow = () => S.framed ??= JSON.parse(cut.frame(S.si, S.t) || 'null');
export const propsOf = (l, time = S.t) => l ? JSON.parse(cut.props(S.si, l.id, time)) : [];
export const keyPaths = l => propsOf(l, 0).map(r => r.p);
export const numPaths = l => propsOf(l, 0).filter(r => typeof r.v === 'number').map(r => r.p);
export const now = (l, p) => propsOf(l).find(r => r.p === p)?.v;
export function insertKey(keys, k) { keys.push(k); keys.sort((a, b) => a.t - b.t); return k; }
// Write a value at the playhead: into the key there (or a new one) when the
// property is animated, else as its plain value.
export function setValue(l, p, v) {
  if (bound(l, p)) return;
  const cur = getp(l, p);
  if (!isKeys(cur)) { setp(l, p, v); return; }
  const i = keyAt(cur, snap(S.t));
  if (i >= 0) cur[i].v = v; else insertKey(cur, { t: snap(S.t), v, interp: 'bezier' });
}
// The ◆ button: add a key at the playhead, or remove the one there; the last
// key removed leaves the property a plain value.
export function toggleKey(l, p, v = now(l, p)) {
  if (bound(l, p)) return;
  const cur = getp(l, p);
  if (!isKeys(cur)) { setp(l, p, [{ t: snap(S.t), v, interp: 'bezier' }]); return; }
  const i = keyAt(cur, snap(S.t));
  if (i < 0) S.selKey = { l, p, k: insertKey(cur, { t: snap(S.t), v, interp: 'bezier' }) };
  else if (cur.length === 1) setp(l, p, v);
  else cur.splice(i, 1);
}
export function deleteKey({ l, p, k }) {
  const keys = getp(l, p);
  if (!isKeys(keys)) return;
  if (keys.length === 1) setp(l, p, k.v); else keys.splice(keys.indexOf(k), 1);
  S.selKey = null;
}

