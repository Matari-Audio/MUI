// The look of a 3D scene in the inspector: its sky and what lights it,
// ground, fog, glow and occlusion, the project's 3D render settings, and
// a 3D layer's material. Keyed or bound numbers show read-only; edit
// those in the JSON.
import { edit, loadAssets } from './edit.js';
import { field, input, choice, lock, section, adder } from './inspector.js';
import { $, S } from './state.js';

function head(text) {
  const h = document.createElement('h3'); h.className = 'fx-head'; h.textContent = text;
  $('#inspector').append(h);
}
// A box that adds `key` to `o` (fresh) or takes it away; returns what is there.
function toggle(o, label, key, fresh) {
  const c = document.createElement('input'); c.type = 'checkbox';
  c.checked = !!o[key]; c.dataset.look = key;
  c.onchange = () => edit(() => { if (c.checked) o[key] = fresh(); else delete o[key]; });
  field(label, c);
  return o[key];
}
// A number; left at its default it leaves the file (`dflt` undefined: kept).
function num(o, k, dflt, label, lo = -Infinity, hi = Infinity) {
  const v = o[k] ?? dflt, fixed = typeof v === 'number';
  const i = lock(input(fixed ? v : 'keyed', x => edit(() => {
    const n = Math.min(hi, Math.max(lo, Number(x) || 0));
    if (n === dflt) delete o[k]; else o[k] = n;
  }), fixed ? 'number' : 'text'), v);
  if (!fixed) i.disabled = true;
  i.dataset.look = k;
  field(label, i);
}
function color(o, k, dflt, label) {
  const v = o[k] ?? dflt;
  const i = lock(input(typeof v === 'string' ? v : 'keyed', x => edit(() => { o[k] = x; })), v);
  if (typeof v !== 'string') i.disabled = true;
  i.dataset.look = k;
  field(label, i);
}
function check(o, k, dflt, label) {
  const c = document.createElement('input'); c.type = 'checkbox';
  c.checked = o[k] ?? dflt; c.dataset.look = k;
  c.onchange = () => edit(() => { if (c.checked === dflt) delete o[k]; else o[k] = c.checked; });
  field(label, c);
}

// The sun seen from above: straight into the scene is up, overhead the
// middle, the horizon the ring (and 10° below it the rim). Drag the sun.
const NS = 'http://www.w3.org/2000/svg';
const RING = 48, LOW = -10;
function sunDial(k) {
  const el = k.elevation ?? 30, az = k.azimuth ?? 0;
  const live = typeof el === 'number' && typeof az === 'number';
  const svg = document.createElementNS(NS, 'svg');
  svg.setAttribute('viewBox', '-60 -60 120 120');
  svg.classList.add('sun-dial'); svg.dataset.sunDial = '';
  if (!live) svg.classList.add('keyed');
  const mk = (tag, attrs) => {
    const e = document.createElementNS(NS, tag);
    for (const [a, v] of Object.entries(attrs)) e.setAttribute(a, v);
    svg.append(e); return e;
  };
  mk('circle', { r: RING * (90 - LOW) / 90, class: 'below' });
  mk('circle', { r: RING, class: 'horizon' });
  mk('circle', { r: RING / 2, class: 'ring' });
  mk('path', { d: `M0 ${-RING - 9}l-4 7h8z`, class: 'ahead' });
  const sun = mk('circle', { r: 6, class: 'sun' });
  const tip = mk('title', {});
  const place = (e, a) => {
    const r = RING * (90 - e) / 90, t = a * Math.PI / 180;
    sun.setAttribute('cx', r * Math.sin(t)); sun.setAttribute('cy', -r * Math.cos(t));
    tip.textContent = `sun ${e}° up, ${a}° round`;
  };
  place(live ? el : 30, live ? az : 0);
  if (live) {
    let at = null;
    const read = ev => {
      const b = svg.getBoundingClientRect();
      const x = (ev.clientX - b.left) / b.width * 120 - 60, y = (ev.clientY - b.top) / b.height * 120 - 60;
      const r = Math.min(Math.hypot(x, y), RING * (90 - LOW) / 90);
      return [Math.round((90 - r / RING * 90) * 2) / 2, Math.round(Math.atan2(x, -y) * 180 / Math.PI)];
    };
    svg.onpointerdown = ev => { svg.setPointerCapture(ev.pointerId); at = read(ev); place(...at); };
    svg.onpointermove = ev => { if (at) { at = read(ev); place(...at); } };
    svg.onpointerup = () => {
      if (!at) return;
      const [e, a] = at; at = null;
      edit(() => { k.elevation = e; if (a) k.azimuth = a; else delete k.azimuth; });
    };
  }
  field('sun', svg);
}

export function sceneLook(s) {
  head('Look');
  // The sky, and what lights the scene: the sky, an environment or neither.
  const k = toggle(s, 'sky', 'sky', () => ({ model: 'physical', elevation: 30 }));
  if (k) {
    const physical = k.model === 'physical';
    const m = choice(k.model ?? 'gradient', ['gradient', 'physical'], v => edit(() => {
      if (v === 'gradient') delete k.model; else k.model = v;
    }));
    m.dataset.look = 'model';
    field('model', m);
    sunDial(k);
    num(k, 'elevation', 30, 'elevation', -90, 90);
    num(k, 'azimuth', 0, 'azimuth');
    if (physical) num(k, 'turbidity', 2, 'turbidity', 1, 32);
    num(k, 'cover', 0.35, 'clouds', 0, 1);
    num(k, 'intensity', 1, 'intensity', 0);
    const by = s.environment ? 'environment' : (k.light ?? physical) ? 'sky' : 'none';
    const lit = choice(by, ['sky', 'environment', 'none'], v => edit(() => {
      if (v === 'environment') { s.environment ??= {}; return; }
      delete s.environment;
      if ((v === 'sky') === physical) delete k.light; else k.light = v === 'sky';
    }));
    lit.dataset.look = 'lit'; lit.title = 'What lights the scene and shows in reflections';
    field('lit by', lit);
    check(k, 'sun_light', physical, 'sun lamp');
  }
  const e = toggle(s, 'environment', 'environment', () => ({}));
  if (e) {
    field('hdri', input(e.hdri ?? '', v => edit(() => { if (v) e.hdri = v; else delete e.hdri; loadAssets(); })));
    num(e, 'intensity', 1, 'intensity', 0);
    num(e, 'rotation', 0, 'rotation');
    check(e, 'background', false, 'env background');
  }
  const g = toggle(s, 'ground', 'ground', () => ({ y: Math.round(S.R.size[1] * 0.8) }));
  if (g) {
    num(g, 'y', undefined, 'ground y');
    color(g, 'color', '#1c1d24', 'color');
    num(g, 'reflect', 0, 'reflect', 0, 1);
    num(g, 'contact', 0.6, 'contact', 0, 1);
    num(g, 'radius', 2400, 'radius', 0);
  }
  const f = toggle(s, 'fog', 'fog', () => ({ color: '#9aa4b2', near: 1500, far: 6000 }));
  if (f) { color(f, 'color', undefined, 'fog color'); num(f, 'near', undefined, 'near', 0); num(f, 'far', undefined, 'far', 0); }
  const b = toggle(s, 'bloom', 'bloom', () => ({}));
  if (b) { num(b, 'strength', 0.5, 'bloom', 0); num(b, 'threshold', 1, 'threshold', 0); }
  const a = toggle(s, 'occlusion', 'ao', () => ({}));
  if (a) { num(a, 'strength', 1, 'ao strength', 0); num(a, 'radius', 60, 'ao radius', 0); }

  // The project's 3D render settings: how glass is drawn.
  head('Render');
  const r = S.doc.render ?? {};
  const tidy = () => { if (Object.keys(r).length) S.doc.render = r; else delete S.doc.render; };
  const glass = choice(r.glass ?? 'raster', ['raster', 'trace', 'rt', 'rt-path'], v => edit(() => {
    if (v === 'raster') delete r.glass; else r.glass = v;
    if (v !== 'rt-path') delete r.glass_samples;
    tidy();
  }));
  glass.dataset.look = 'glass'; glass.title = 'raster: screen-space; trace: closed form; rt: ray traced; rt-path: path traced';
  field('glass', glass);
  if (r.glass === 'rt-path') {
    const n = input(r.glass_samples ?? 16, x => edit(() => {
      const v = Math.min(4096, Math.max(1, Math.round(Number(x)) || 16));
      if (v === 16) delete r.glass_samples; else r.glass_samples = v;
      tidy();
    }), 'number');
    n.dataset.look = 'glass_samples';
    field('samples', n);
  }
}

// A 3D layer's material starts from a preset; its fields then show as rows.
export const MATERIALS = {
  'glass': { transmission: 1, roughness: 0.02, ior: 1.5, bevel: 12 },
  'frosted glass': { transmission: 1, roughness: 0.35, ior: 1.5, bevel: 12 },
  'metal': { metallic: 1, roughness: 0.15 },
  'plastic': { metallic: 0, roughness: 0.45 },
  'emissive': { roughness: 1, emission: 2 },
};
const SOLID = ['camera', 'light', 'audio', 'group', 'comp'];
export function layerLook(l, scene) {
  if (scene.mode !== '3d' || SOLID.includes(l.kind)) return;
  const c = document.createElement('input'); c.type = 'checkbox';
  c.checked = l.cast_shadows ?? true; c.dataset.look = 'cast_shadows';
  c.onchange = () => edit(() => { if (c.checked) delete l.cast_shadows; else l.cast_shadows = false; });
  field('shadows', c);
  if (!l.material) adder('+ material', Object.keys(MATERIALS), v => edit(() => { l.material = structuredClone(MATERIALS[v]); }));
}
// The header over a layer's material rows: swap the preset, or remove it.
export function materialHeader(l) {
  const swap = choice('', ['', ...Object.keys(MATERIALS)], v => { if (v) edit(() => { l.material = structuredClone(MATERIALS[v]); }); });
  swap.options[0].textContent = 'preset'; swap.dataset.look = 'preset';
  section('Material', [swap], () => edit(() => { delete l.material; }));
}
