// mui-cut web editor. The project lives here as plain JSON; every edit goes
// through the WASM engine (`Cut.load`), which also draws the viewport and
// samples the graph editor's curves, so what you see is what `mui-cut render`
// writes. The viewport draws in worker.js (WebGPU or WebGL2 when the browser
// has it, else Vello CPU); this thread keeps a `Cut` for validation, the inspector
// and the graph's samples.
import init, { Cut } from './pkg/mui_cut.js';

export const $ = s => document.querySelector(s);
export const KIND_ICON = { rect: '▭', ellipse: '◯', text: 'T', image: '▣', path: '〰', duplicator: '⁂', svg: 'S', lottie: 'L', camera: '⌖', light: '☀', model: '◈', plugin: '⧉', audio: '♪', patch: '☰', group: '▤', comp: '⧈' };
export const VECTOR = ['text', 'path', 'duplicator', 'svg', 'lottie'];
// Graphite, as in style.css: greys only, state by value, weight and shape.
export const C = {
  text: '#8a8a8a', textOn: '#e6e6e6', grid: '#262626', gridText: '#666666',
  tick: '#3a3a3a', row: '#181818', rowAlt: '#1c1c1c', rowOn: '#262626',
  curve: '#d4d4d4', curves: ['#e6e6e6', '#e8806f', '#79c47f', '#6fa8e0', '#e2c76b', '#c590dc', '#6fd0c8'], key: '#b0b0b0', layerKey: '#6a6a6a', handle: '#7a7a7a',
  handleLine: '#ffffff30', picked: '#ffffff', pickedLine: '#ffffffa0',
  bar: '#2a2a2a', barOn: '#3a3a3a', barLine: '#444444', barEdge: '#9a9a9a', ruler: '#151515', outside: '#0000004d',
  playhead: '#f2f2f2', hover: '#ffffff70', sel: '#ffffff', halo: '#000000a0', marquee: '#ffffff14', guide: '#ffffffd0',
};

await init();
export const cut = new Cut();
export const FX = JSON.parse(Cut.effects());   // the effect schema: [{name, about, params}]
export const worker = new Worker(new URL('./worker.js', import.meta.url), { type: 'module' });

// The editor's shared state: one object, so every module reads and writes
// the same fields.
export const S = {
  drawing: false,     // a draw is in flight in the worker
  doc: null,          // the project, as JSON data (bindings and all)
  R: null,            // the project as the engine resolved it: sizes, fps, durations
  variant: '',        // the variant the viewport previews, '' for the defaults
  server: null,       // the project at revision `rev`, as `serve` last sent it
  rev: 0,
  si: 0,              // scene index
  t: 0,               // playhead, seconds into the scene
  // What is selected: layer ids or `layer#part` ids, in the order picked;
  // the last is the primary, which the inspector and the graph show.
  selection: [],
  // The primary as its layer id and part (null for a whole layer).
  get sel() { const id = this.selection.at(-1); return id == null ? null : id.split('#')[0]; },
  set sel(id) { this.selection = id == null ? [] : [id]; },
  get selPart() { const id = this.selection.at(-1), i = id?.indexOf('#') ?? -1; return i < 0 ? null : id.slice(i + 1); },
  set selPart(part) { const l = this.sel; if (l != null) this.selection = [...this.selection.slice(0, -1), part ? `${l}#${part}` : l]; },
  prop: 'x',          // property shown in the graph editor
  // The selected keys, `{ l, p, k }` (layer, property path, the key object),
  // in the order picked; the last is the primary.
  selKeys: [],
  get selKey() { return this.selKeys.at(-1) ?? null; },
  set selKey(k) { this.selKeys = k ? [k] : []; },
  quads: [],          // layer outlines from the last render, project px
  playing: false,
  need: true,
  locked: false,      // play on the project's frame grid only
  interact: false,    // viewport clicks drive the plugin, keyed at the playhead
  base: null,         // the document before the gesture in progress
  sourceList: [],     // the Sources panel's rows: `Cut.sources()`
  framed: undefined,  // the frame at the playhead, once per inspector pass
  range: null,        // the graph's value range
  // The time view the timeline and the graph share: seconds at the left
  // and right of their track areas, for scene `si`.
  view: { t0: 0, t1: 1, si: -1 },
  tlNeed: false,      // the timeline and graph (only) need a redraw
};

// Who this editor is in the server's merge reports.
export const me = 'editor-' + Math.random().toString(36).slice(2, 10);

// Playback clock: while playing, project time is the monotonic clock since
// `clock.at` (ms), plus the time `clock.t` it started from. It never
// accumulates per-frame deltas, so it cannot drift; a slow draw drops
// frames instead of slowing the clock.
export const clock = { at: 0, t: 0 };
// Pacing, for the HUD and the e2e: when each drawn frame came back (ms) and
// the project frame it showed; frames the playhead passed without drawing.
export const pacing = { shown: [], dropped: 0, last: -1, pending: -1 };
globalThis.pacing = pacing;
globalThis.cutQuads = () => S.quads; // the e2e aims at plugin parts with these
// A plugin layer's part tree at the playhead (plugin::tree_json): id (the
// path keyed as `parts.<id>.x`), surface, level, frame, rects, thumb (an
// asset path), motion, children. Null before its capture has loaded.
globalThis.cutParts = id => { const j = cut.plugin_parts(S.si, S.t, id); return j ? JSON.parse(j) : null; };
// Gestures, as the document before and after (JSON): undo replays the
// difference backwards, only where the field still holds this editor's
// value, so it never takes back an agent's edit.
export const undo = [], redo = [];
export const assets = new Set();
export const manifests = new Map(); // plugin capture manifests by path
export const unfolded = new Set();  // open folders in the Sources tree: `id`, `id/part`
export const sizes = new Map();     // imported images' natural sizes, by path
globalThis.cutSources = () => S.sourceList; // the e2e reads a plugin's capture path
globalThis.cutTime = () => S.t; // the e2e checks the playhead keeps with the audio clock
