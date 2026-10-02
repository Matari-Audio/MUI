import { layer, round, scene } from './doc.js';
import { edit, status } from './edit.js';
import { $, C, S, assets, clock, worker } from './state.js';
import { ROW, tlT, tlX } from './timeline.js';
import { toggle } from './transport.js';

// ---------- sound: `serve` plays it, the playhead follows the device
// Play, pause and seek go to `/transport`; while playing, the clock is
// re-anchored to the audio clock (what is heard), so they never drift.
let audioAt = 0;
export function transport() {
  fetch('/transport', { method: 'POST', body: JSON.stringify({ playing: S.playing, t: S.t, scene: S.si }) })
    .then(r => r.ok ? r.json() : null).then(showTransport).catch(() => {});
}
function showTransport(r) {
  if (!r) return;
  $('#audio-state').textContent = `${r.device || 'no device'} · ${r.latency_ms.toFixed(0)} ms`;
  if (!S.playing || !r.playing) return;
  const ahead = clock.t + (performance.now() - clock.at) / 1000;
  // Small differences are the request's own time: only a real drift moves it.
  if (Math.abs(ahead - r.t) > 0.02) Object.assign(clock, { at: performance.now(), t: r.t });
}
setInterval(() => {
  if (!S.playing || performance.now() - audioAt < 250) return;
  audioAt = performance.now();
  fetch('/transport').then(r => r.ok ? r.json() : null).then(showTransport).catch(() => {});
}, 100);
// A plugin layer's UI as it plays: its live capture drawn instead of the
// one the playhead names (`live:<layer>`); null when playing stops.
const liveShown = new Map();
export async function showLive(m) {
  const post = (path, bytes) => worker.postMessage({ type: 'asset', path, bytes });
  if (!m.state) { post('live:' + m.layer, new Uint8Array()); liveShown.delete(m.layer); S.need = true; return; }
  if (!S.playing) return;
  try {
    const path = `.cut-cache/${m.state}.json`;
    const r = await fetch('/asset/' + path);
    if (!r.ok) return;
    const bytes = new Uint8Array(await r.arrayBuffer());
    const man = JSON.parse(new TextDecoder().decode(bytes));
    for (const img of man.layers.flatMap(f => [f.src, f.free?.src]).filter(Boolean).map(s => '.cut-cache/' + s)) {
      if (assets.has(img)) continue;
      const ri = await fetch('/asset/' + img);
      if (!ri.ok) return;
      assets.add(img);
      post(img, new Uint8Array(await ri.arrayBuffer()));
    }
    if (!S.playing) return;
    post(path, bytes);
    post('live:' + m.layer, new TextEncoder().encode(m.state));
    // The one it replaces goes: a playing plugin makes many.
    const old = liveShown.get(m.layer);
    if (old) post(`.cut-cache/${old}.json`, new Uint8Array());
    liveShown.set(m.layer, m.state);
    S.need = true;
  } catch (e) { console.warn(e); }
}

// Keys: on-screen, the computer keyboard and MIDI play the selected plugin
// layer (else the scene's first); while playing, each note is recorded.
const KEYMAP = 'awsedftgyhujk';
let octave = 60, keysOn = false;
const held = new Map();
export const keyLayer = () => (layer()?.kind === 'plugin' ? layer() : scene()?.layers.find(l => l.kind === 'plugin')) ?? null;
function playKey(note, on, vel = 100) {
  const l = keyLayer();
  if (!l) { status('Keys play a plugin layer: add one', true); return; }
  fetch('/live/note', { method: 'POST', body: JSON.stringify({ layer: l.id, note, on, velocity: vel }) }).catch(() => {});
  $(`#keys [data-note="${note}"]`)?.classList.toggle('down', on);
  if (on) { held.set(note, { t: S.t, vel, l }); return; }
  const h = held.get(note);
  held.delete(note);
  if (!h || !S.playing) return;
  const dur = Math.max(1 / S.R.fps, S.t >= h.t ? S.t - h.t : S.R.scenes[S.si].duration - h.t);
  edit(() => {
    h.l.notes = [...(h.l.notes ?? []), { t: round(h.t), dur: round(dur), pitch: note, ...(h.vel !== 100 && { vel: h.vel }) }]
      .sort((a, b) => a.t - b.t || a.pitch - b.pitch);
  });
}
function drawKeys() {
  const k = $('#keys');
  k.replaceChildren(...Array.from({ length: 25 }, (_, i) => {
    const note = octave - 12 + i, b = document.createElement('button');
    b.dataset.note = note;
    b.className = [1, 3, 6, 8, 10].includes(note % 12) ? 'black' : '';
    b.title = `MIDI ${note}`;
    b.onpointerdown = e => { b.setPointerCapture(e.pointerId); playKey(note, true); };
    b.onpointerup = () => playKey(note, false);
    return b;
  }));
}
$('#keys-toggle').onclick = async () => {
  keysOn = toggle('#keys-toggle', !keysOn);
  $('#keys').hidden = !keysOn;
  if (!keysOn) return;
  drawKeys();
  try {
    const midi = await navigator.requestMIDIAccess?.();
    for (const input of midi?.inputs.values() ?? []) {
      input.onmidimessage = ({ data: [s, n, v] }) => {
        if ((s & 0xf0) === 0x90 && v > 0) playKey(n, true, v);
        else if ((s & 0xf0) === 0x80 || (s & 0xf0) === 0x90) playKey(n, false);
      };
    }
  } catch (e) { console.warn('MIDI', e); }
};
addEventListener('keydown', e => {
  if (!keysOn || e.repeat || e.ctrlKey || e.metaKey || e.altKey || e.target.closest('input, select, textarea')) return;
  const k = e.key.toLowerCase(), i = KEYMAP.indexOf(k);
  if (k === 'z' || k === 'x') { octave = Math.max(24, Math.min(96, octave + (k === 'z' ? -12 : 12))); drawKeys(); }
  else if (i >= 0) playKey(octave + i, true);
  else return;
  e.preventDefault(); e.stopImmediatePropagation();
}, true);
addEventListener('keyup', e => {
  const i = KEYMAP.indexOf(e.key.toLowerCase());
  if (keysOn && i >= 0) playKey(octave + i, false);
}, true);

// Audio layers' waveforms for the timeline, decoded once by the browser.
const waves = new Map();
function wave(path) {
  if (waves.has(path)) return waves.get(path);
  waves.set(path, null);
  fetch('/asset/' + path).then(r => r.arrayBuffer())
    .then(b => new OfflineAudioContext(1, 1, 48000).decodeAudioData(b))
    .then(a => {
      const d = a.getChannelData(0), n = Math.ceil(a.duration * 100), peaks = new Float32Array(n);
      for (let i = 0; i < d.length; i++) { const j = Math.floor(i / a.sampleRate * 100); peaks[j] = Math.max(peaks[j], Math.abs(d[i])); }
      waves.set(path, peaks); S.need = true;
    }).catch(() => {});
  return null;
}
// A layer row's sound: a plugin's notes as bars by pitch, an audio
// layer's waveform (from `time` into the file).
export function drawSound(c, l, y) {
  if (l.kind === 'plugin' && l.notes?.length) {
    const ps = l.notes.map(n => n.pitch), lo = Math.min(...ps), span = Math.max(1, Math.max(...ps) - lo);
    c.fillStyle = C.layerKey;
    for (const n of l.notes) {
      const x0 = tlX(n.t), x1 = tlX(n.t + n.dur);
      c.fillRect(x0, y + ROW - 4 - (n.pitch - lo) / span * (ROW - 8), Math.max(1, x1 - x0), 2);
    }
  } else if (l.kind === 'audio') {
    const peaks = wave(l.path);
    if (!peaks) return;
    const off = typeof l.time === 'number' ? l.time : 0;
    c.fillStyle = C.tick;
    for (let x = tlX(0); x < tlX(S.R.scenes[S.si].duration); x++) {
      const p = peaks[Math.floor((tlT(x) + off) * 100)] ?? 0;
      c.fillRect(x, y + ROW / 2 - p * ROW / 2, 1, Math.max(1, p * ROW));
    }
  }
}

