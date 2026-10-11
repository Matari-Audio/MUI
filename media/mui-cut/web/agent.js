import { layer, numPaths, round, scene } from './doc.js';
import { select } from './lists.js';
import { S } from './state.js';
import { refresh, setPlaying } from './transport.js';

// ---------- what an agent sees: the editor's state out, its moves in
// The server keeps the last state for `mui-cut mcp` (editor_state), and
// relays an agent's `control` messages (editor_goto) back here.
let reported = '', reportedAt = 0;
export function report(ms) {
  if (!S.doc || ms - reportedAt < (S.playing ? 500 : 150)) return;
  const s = JSON.stringify({ scene: scene()?.name ?? null, scene_index: S.si, t: round(S.t), selection: S.selPart ? `${S.sel}#${S.selPart}` : S.sel, prop: S.prop, playing: S.playing, key: S.selKey ? { prop: S.selKey.p, t: S.selKey.k.t } : null });
  if (s === reported) return;
  reported = s; reportedAt = ms;
  fetch('/state', { method: 'PUT', body: s }).catch(() => {});
}
export function control(m) {
  if (typeof m.scene === 'string') {
    const i = S.doc.scenes.findIndex(s => s.name === m.scene);
    if (i >= 0 && i !== S.si) { S.si = i; S.sel = null; S.selKey = null; }
  }
  if (typeof m.t === 'number') S.t = Math.max(0, Math.min(scene().duration, m.t));
  if (typeof m.playing === 'boolean') setPlaying(m.playing);
  if ('select' in m) {
    if (m.select === null || scene().layers.some(l => l.id === m.select.split('#')[0])) select(m.select);
  }
  if (typeof m.prop === 'string' && layer() && numPaths(layer()).includes(m.prop)) S.prop = m.prop;
  refresh();
}
