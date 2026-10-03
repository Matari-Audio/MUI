import { frameNow, getp, isBind, isKeys, keyAt, layer, now, propsOf, round, scene, selectedLayers, setValue, snap, tidy, toggleKey } from './doc.js';
import { edit, loadAssets, showError } from './edit.js';
import { parentTo } from './lists.js';
import { LOOKS } from './looks.js';
import { $, FX, S, VECTOR, cut } from './state.js';
import { refresh } from './transport.js';
import { compable, ungroupSelected } from './tree.js';
import { layerLook, materialHeader, sceneLook } from './look.js';

// ---------- inspector
export function field(label, input, keyBtn) {
  const l = document.createElement('label'); l.textContent = label;
  const box = $('#inspector');
  box.append(l, input);
  if (keyBtn) box.append(keyBtn); else input.classList.add('wide');
}
export function input(value, onchange, type = 'text') {
  const i = document.createElement(type === 'area' ? 'textarea' : 'input');
  if (type !== 'area') i.type = type;
  i.value = value;
  if (type === 'number') i.step = 'any';
  i.onchange = () => onchange(i.value);
  return i;
}
export function choice(value, options, onchange) {
  const s = document.createElement('select');
  s.replaceChildren(...options.map(o => new Option(o, o, false, o === String(value))));
  s.onchange = () => onchange(s.value);
  return s;
}
// A full-width header over an animator's or deformer's rows, with its
// settings and a remove button.
export function section(title, controls, remove) {
  const h = document.createElement('div'); h.className = 'section';
  const n = document.createElement('span'); n.textContent = title;
  const x = document.createElement('button'); x.textContent = '×'; x.title = 'Remove'; x.onclick = remove;
  h.append(n, ...controls, x);
  $('#inspector').append(h);
}
// The settings of a layer that are not keyable: its text, file, path data,
// layout and so on, and how an animator or deformer selects and moves.
function kindFields(l) {
  const set = (k, v, dflt) => edit(() => { if (v === dflt) delete l[k]; else l[k] = v; });
  if (l.kind === 'text') {
    field('text', input(l.text, v => edit(() => { l.text = v; }), 'area'));
    field('align', choice(l.align ?? 'center', ['left', 'center', 'right'], v => set('align', v, 'center')));
    // '' is Inter; the rest are the font sources, by id.
    const fonts = ['', ...(S.doc.sources ?? []).filter(m => m.kind === 'font').map(m => m.id)];
    if (l.font && !fonts.includes(l.font)) fonts.push(l.font);
    const f = choice(l.font ?? '', fonts, v => { set('font', v, ''); loadAssets(); });
    f.options[0].textContent = 'Inter'; f.dataset.font = ''; f.title = 'The font: Inter, or a font source';
    field('font', f);
  }
  if (l.kind === 'comp') {
    // Only scenes that do not comp this one, so no cycle can be picked.
    const ok = compable(scene().name);
    const c = choice(l.scene, ok.includes(l.scene) ? ok : [l.scene, ...ok], v => edit(() => { l.scene = v; }));
    c.dataset.compScene = ''; c.title = 'The scene this layer plays (double-click the layer to open it)';
    field('scene', c);
  }
  if (l.kind === 'group') {
    const b = document.createElement('button'); b.textContent = 'Ungroup'; b.dataset.ungroup = '';
    b.title = 'Hand the children to the parent where they are (Ctrl+Shift+G)';
    b.onclick = ungroupSelected;
    field('children', b);
  }
  if (l.kind === 'patch') field('of', input(l.of, v => edit(() => { l.of = v; })));
  if (['image', 'svg', 'lottie', 'model', 'audio'].includes(l.kind)) field('path', input(l.path, v => edit(() => { l.path = v; loadAssets(); })));
  if (l.kind === 'lottie') {
    field('speed', input(l.speed ?? 1, v => set('speed', Number(v), 1), 'number'));
    field('loop', choice(l.loop ?? true, ['true', 'false'], v => set('loop', v === 'true', true)));
  }
  if (l.kind === 'path') field('d', input(l.d, v => edit(() => { l.d = v; }), 'area'));
  if (l.kind === 'camera') {
    field('look_at', choice(l.look_at ?? '', ['', ...scene().layers.filter(o => o !== l).map(o => o.id)], v => set('look_at', v, '')));
    field('path', input(l.path ?? '', v => set('path', v, ''), 'area'));
  }
  if (l.kind === 'light') field('type', choice(l.type ?? 'directional', ['directional', 'spot', 'point', 'ambient'], v => set('type', v, 'directional')));
  if (l.kind === 'plugin') {
    const json = (v, k) => { try { const o = JSON.parse(v); edit(() => { l[k] = o; }); } catch (e) { showError(`${k}: ${e}`); } };
    field('source', input(JSON.stringify(l.source), v => json(v, 'source'), 'area'));
    field('select', input((l.select ?? []).join(', '), v => set('select', v.split(',').map(s => s.trim()).filter(Boolean), undefined)));
    const b = document.createElement('button');
    b.textContent = 'Explode / collapse'; b.dataset.explode = '';
    b.title = 'Key explode at the playhead: 0.5 when collapsed, 0 when exploded';
    b.onclick = () => edit(() => setValue(l, 'explode', (now(l, 'explode') ?? 0) > 0.01 ? 0 : 0.5));
    field('parts', b);
    const lv = input(l.explode_levels ?? 1, v => set('explode_levels', Math.min(8, Math.max(1, Math.round(Number(v)) || 1)), 1), 'number');
    lv.dataset.levels = ''; lv.min = 1; lv.max = 8; lv.step = 1;
    lv.title = 'Explode levels: 1 the panels, 2 the panels and then their controls (captured this deep)';
    field('explode levels', lv);
    const st = input(l.explode_stagger ?? 0, v => set('explode_stagger', Math.max(0, Number(v) || 0), 0), 'number');
    st.title = 'Seconds each level of explode runs behind the one above';
    field('level stagger', st);
  }
  if (l.kind === 'duplicator') {
    // Instance another layer (and its subtree) instead of a shape.
    const src = choice(l.source ?? '', ['', ...scene().layers.filter(o => o !== l).map(o => o.id)], v => set('source', v, ''));
    src.options[0].textContent = '(a shape)'; src.dataset.source = ''; src.title = 'A layer (a group with its children) drawn at every copy';
    field('source', src);
    // The instance picked in the viewport: which copy, where its animators
    // put it, and a way to its source.
    const f = l.source && S.inst?.id === l.id && frameNow()?.layers.find(d => d.id === l.id);
    if (f) {
      const c = S.inst.c, fx = f.fx?.[c];
      const info = input(`copy ${c} of ${f.count}` + (fx ? ` · x ${round(fx.x)} y ${round(fx.y)} rot ${round(fx.rotation)}° scale ${round(fx.scale)} opacity ${round(fx.opacity)}` : ''), () => {});
      info.readOnly = true; info.dataset.instance = String(c); info.title = 'The picked copy: its index and the offsets its animators give it now';
      field('instance', info);
      const go = document.createElement('button'); go.textContent = `Edit source · ${l.source}`; go.dataset.editSource = '';
      go.title = 'Select the source layer: every copy follows it';
      go.onclick = () => { S.inst = null; S.sel = l.source; S.selKey = null; refresh(); };
      field('', go);
    }
    if (l.source) {
      const c = document.createElement('input'); c.type = 'checkbox'; c.checked = !!l.show_source; c.dataset.showSource = '';
      c.title = 'Keep drawing the source where it is too'; c.onchange = () => set('show_source', c.checked, false);
      field('show source', c);
    } else field('shape', choice(l.shape ?? 'rect', ['rect', 'ellipse', 'path'], v => set('shape', v, 'rect')));
    if (!l.source && l.shape === 'path') field('d', input(l.d ?? '', v => set('d', v, ''), 'area'));
    field('layout', choice(l.layout ?? 'grid', ['grid', 'radial', 'linear', 'path'], v => set('layout', v, 'grid')));
    if (l.layout === 'path') field('along', input(l.along ?? '', v => set('along', v, ''), 'area'));
    field('orient', choice(l.orient ?? false, ['false', 'true'], v => set('orient', v === 'true', false)));
  }
  // 3D: drawn flat over the shot and its effects (a caption, a logo).
  if (scene().mode === '3d' && !['camera', 'light', 'model', 'audio'].includes(l.kind)) {
    const c = document.createElement('input'); c.type = 'checkbox';
    c.checked = !!l.overlay; c.dataset.overlay = '';
    c.title = 'Draw flat over the 3D shot and its effects, as in 2D: a caption or a logo';
    c.onchange = () => set('overlay', c.checked, false);
    field('overlay', c);
  }
}
function groupHeader(l, group, i) {
  const list = l[group], item = list[i];
  const remove = () => edit(() => { list.splice(i, 1); if (!list.length) delete l[group]; S.selKey = null; });
  const set = (k, v, dflt) => edit(() => { if (v === dflt) delete item[k]; else item[k] = v; });
  if (group === 'animators') {
    const c = [];
    if (l.kind === 'text') c.push(choice(item.by ?? 'char', ['char', 'word', 'line'], v => set('by', v, 'char')));
    c.push(choice(item.shape ?? 'square', ['square', 'ramp_up', 'ramp_down', 'triangle', 'round', 'smooth'], v => set('shape', v, 'square')));
    c.push(choice(item.ease ?? 'linear', ['linear', 'in', 'out', 'in_out', 'step'], v => set('ease', v, 'linear')));
    const order = choice(item.order ?? 'forward', ['forward', 'reverse', 'random', 'distance'], v => set('order', v, 'forward'));
    order.title = 'Order: distance ranks by distance from the effector (or the origin)'; c.push(order);
    const se = choice(item.stagger_ease ?? 'linear', ['linear', 'in', 'out', 'in_out', 'step'], v => set('stagger_ease', v, 'linear'));
    se.title = 'Stagger ease: how the delays spread over the ranks'; se.dataset.staggerEase = ''; c.push(se);
    const s = input(item.seed ?? 0, v => set('seed', Math.max(0, Math.round(Number(v))) || 0, 0), 'number'); s.title = 'Seed: the random order and jitter'; c.push(s);
    section(`Animator ${i + 1}`, c, remove);
    {
      // The effector: a field over the copies, children or glyphs, drawn
      // (and dragged) in the viewport.
      const on = document.createElement('input'); on.type = 'checkbox'; on.checked = !!item.falloff; on.dataset.effector = String(i);
      on.title = 'A spatial effector: weigh units by where they sit (drag its centre and radius in the viewport)';
      on.onchange = () => set('falloff', on.checked ? {} : undefined, undefined);
      field('effector', on);
      if (item.falloff) {
        const f = item.falloff, fset = (k, v, d) => edit(() => { if (v === d) delete f[k]; else f[k] = v; });
        const sh = choice(f.shape ?? 'sphere', ['sphere', 'box', 'linear'], v => fset('shape', v, 'sphere')); sh.dataset.effectorShape = String(i);
        field('field', sh);
        const inv = document.createElement('input'); inv.type = 'checkbox'; inv.checked = !!f.invert; inv.onchange = () => fset('invert', inv.checked, false);
        field('invert', inv);
      }
    }
  } else if (group === 'behaviours') {
    const p = choice(item.prop, BEHAVE, v => set('prop', v, undefined)); p.dataset.behaviourProp = String(i); p.title = 'The property it moves';
    const k = choice(item.kind ?? 'wiggle', ['wiggle', 'oscillate', 'spring'], v => set('kind', v, 'wiggle')); k.dataset.behaviourKind = String(i);
    k.title = 'Spring: chases the property\'s own keys, overshooting and settling (freq, damping)';
    const c = [p, k];
    if ((item.kind ?? 'wiggle') === 'wiggle') { const sd = input(item.seed ?? 0, v => set('seed', Math.max(0, Math.round(Number(v))) || 0, 0), 'number'); sd.title = 'seed'; c.push(sd); }
    section(`Behaviour ${i + 1}`, c, remove);
  } else {
    const c = [];
    if (item.kind === 'noise') { const s = input(item.seed ?? 0, v => set('seed', Math.max(0, Math.round(Number(v))) || 0, 0), 'number'); s.title = 'seed'; c.push(s); }
    section(`${item.kind} ${i + 1}`, c, remove);
  }
}
// What a behaviour moves (motion.rs BEHAVIOUR_PROPS).
const BEHAVE = ['x', 'y', 'z', 'scale', 'rotation', 'rx', 'ry', 'opacity', 'width', 'height', 'radius', 'font_size', 'tracking', 'stroke_width', 'path_offset', 'ring_radius'];
// Options are values, or `[value, label]`.
export function adder(label, options, add) {
  const s = choice('', ['', ...options.map(o => [o].flat()[0])], v => { if (v) add(v); });
  s.options[0].textContent = label;
  options.forEach((o, i) => { if (Array.isArray(o)) s.options[i + 1].textContent = o[1]; });
  s.classList.add('wide'); s.dataset.adder = label;
  const box = $('#inspector'); box.append(document.createElement('span'), s);
}
// A bound field shows its value and stays read-only.
export function lock(i, v) {
  if (!isBind(v) && !(isKeys(v) && v.some(k => isBind(k.v)))) return i;
  i.disabled = true;
  i.title = `bound to the variable \`${(isBind(v) ? v : v.find(k => isBind(k.v)).v).var}\``;
  return i;
}
// The project's variables, as the chosen variant sets them: edits go to the
// variant, or to the declared value with no variant chosen.
function variablesSection() {
  const vars = Object.entries(S.doc.variables ?? {});
  if (!vars.length) return;
  const box = $('#inspector'), head = document.createElement('h3');
  head.className = 'fx-head'; head.textContent = S.variant ? `Variables · ${S.variant}` : 'Variables';
  box.append(head);
  const v = S.doc.variants?.find(v => v.name === S.variant);
  for (const [name, d] of vars) {
    const cur = v?.vars?.[name] ?? d.value;
    const set = x => edit(() => { if (v) (v.vars ??= {})[name] = x; else d.value = x; });
    let i;
    if (d.type === 'enum') {
      i = document.createElement('select');
      i.append(...d.options.map(o => new Option(o, o)));
      i.value = cur; i.onchange = () => set(i.value);
    } else if (d.type === 'bool') {
      i = document.createElement('input'); i.type = 'checkbox';
      i.checked = cur; i.onchange = () => set(i.checked);
    } else i = input(cur, x => set(d.type === 'number' ? Number(x) : x), d.type === 'number' ? 'number' : 'text');
    i.dataset.var = name;
    field(name, i);
  }
}
export function refreshInspector() {
  S.framed = undefined;
  const box = $('#inspector'); box.replaceChildren();
  const l = layer();
  if (!l) {
    const s = scene();
    $('#insp-title').textContent = 'Scene';
    // A rename carries the comps that play this scene along.
    field('name', input(s.name, v => { if (v && !S.doc.scenes.some(o => o.name === v)) edit(() => {
      for (const c of S.doc.scenes.flatMap(o => o.layers)) if (c.kind === 'comp' && c.scene === s.name) c.scene = v;
      s.name = v;
    }); }));
    field('duration', lock(input(S.R.scenes[S.si].duration, v => edit(() => { s.duration = Math.max(0.05, Number(v) || 1); }), 'number'), s.duration));
    const bg = lock(input(frameNow()?.background ?? s.background ?? '#101014', v => edit(() => { s.background = v; })), s.background);
    bg.dataset.bg = '';
    field('background', bg);
    const mode = choice(s.mode ?? '2d', ['2d', '3d'], setMode);
    mode.dataset.mode = ''; mode.title = '3D keeps the layout (its default camera sees the 2D frame); back to 2D, layers go where the camera showed them';
    field('mode', mode);
    field('project', input(`${S.R.size[0]}×${S.R.size[1]} @ ${S.R.fps} fps`, () => {}));
    if (s.mode === '3d') sceneLook(s);
    variablesSection();
    fxSection(s, () => frameNow()?.effects ?? []);
    return;
  }
  const many = selectedLayers();
  if (many.length > 1 && !S.selPart) { multiInspector(many); return; }
  $('#insp-title').textContent = S.selPart ? `Part · ${S.selPart}` : `Layer · ${l.kind}`;
  if (!S.selPart) {
    field('id', input(l.id, v => { if (v && !scene().layers.some(o => o.id === v)) edit(() => {
      for (const o of scene().layers) if (o.parent === l.id) o.parent = v;
      l.id = v; S.sel = v;
    }); }));
    const others = scene().layers.filter(o => o !== l).map(o => o.id);
    const par = choice(l.parent ?? '', ['', ...others], v => parentTo(l.id, v));
    par.dataset.parent = ''; par.title = 'Attach to another layer: it follows the parent, and keeps its place on screen now';
    field('parent', par);
    // In and out points, in scene time: empty is the scene's start or end.
    const dur = S.R.scenes[S.si].duration, fr = 1 / S.R.fps;
    const span = (k, lo, hi) => {
      const i = input(l[k] ?? '', v => edit(() => { if (v === '') delete l[k]; else l[k] = tidy(Math.max(lo(), Math.min(hi(), Number(v) || 0))); }), 'number');
      i.placeholder = k === 'start' ? '0' : String(round(dur)); i.dataset.span = k;
      return i;
    };
    field('in', span('start', () => -Infinity, () => (l.end ?? dur) - fr));
    field('out', span('end', () => (l.start ?? 0) + fr, () => Infinity));
    const hid = document.createElement('input'); hid.type = 'checkbox'; hid.checked = !!l.hidden; hid.dataset.hidden = '';
    hid.onchange = () => edit(() => { if (hid.checked) l.hidden = true; else delete l.hidden; });
    field('hidden', hid);
    kindFields(l);
  }
  const reset = document.createElement('button');
  reset.textContent = 'Reset to default'; reset.dataset.reset = '';
  reset.title = S.selPart ? 'Back to where the plugin puts this part: its offsets and keys cleared' : 'Back to where it was placed: transform keys and offsets cleared';
  reset.onclick = () => resetSelected();
  field('layout', reset);
  let group = '';
  // A plugin's part rows show when that part is selected, and only then.
  const mine = p => S.selPart ? p.startsWith(`parts.${S.selPart}.`) : !p.startsWith('parts.');
  for (const { p, v } of propsOf(l).filter(r => mine(r.p))) {
    const parts = p.split('.');
    const g = parts[0] === 'material' ? 'material' : parts.length > 1 ? parts.slice(0, 2).join('.') : '';
    if (g !== group) {
      group = g;
      if (parts[0] === 'parts') section(`Part ${S.selPart}`, [], () => edit(() => { delete l.parts[S.selPart]; if (!Object.keys(l.parts).length) delete l.parts; S.selPart = null; S.selKey = null; }));
      else if (parts[0] === 'params') { const q = l.params[+parts[1]]; section(`${q.id} · ${q.field}`, [], () => edit(() => { l.params.splice(+parts[1], 1); if (!l.params.length) delete l.params; S.selKey = null; })); }
      else if (g === 'material') materialHeader(l);
      else if (g) groupHeader(l, parts[0], +parts[1]);
    }
    const color = typeof v === 'string';
    const i = input(color ? v : round(v), x => edit(() => setValue(l, p, color ? x : Number(x))), color ? 'text' : 'number');
    i.dataset.prop = p;
    lock(i, getp(l, p));
    const k = document.createElement('button');
    k.className = 'key'; k.textContent = '◆'; k.dataset.key = p;
    k.title = 'Add or remove a key at the playhead';
    k.onclick = () => { edit(() => toggleKey(l, p)); if (!color) S.prop = p; refresh(); };
    // Below an animator or behaviour, its own name (`falloff.x`).
    field(parts.length > 2 && g ? parts.slice(2).join('.') : parts.at(-1), i, k);
  }
  const presets = { text: ['plain', 'typewriter', 'cascade', 'pop'], duplicator: ['plain', 'cascade', 'pop', ['ripple', 'Ripple from point']],
    group: ['plain', ['cascade_children', 'Cascade children'], 'pop', ['ripple', 'Ripple from point']] }[l.kind];
  if (presets) adder('+ animator', presets, v => edit(() => {
    const a = v === 'plain' ? {} : JSON.parse(cut.preset(v, snap(S.t), 1));
    (l.animators ??= []).push(a);
  }));
  adder('+ behaviour', [['wiggle', 'Wiggle'], ['oscillate', 'Oscillate'], ['spring', 'Spring']], v => edit(() => {
    // Wiggle: the position wanders; oscillate: it bobs up and down;
    // spring: its position keys overshoot and settle.
    const add = v === 'wiggle' ? [{ prop: 'x', amount: 20, freq: 1.5 }, { prop: 'y', amount: 20, freq: 1.5 }]
      : v === 'spring' ? [{ prop: 'x', kind: 'spring', freq: 2, damping: 0.3 }, { prop: 'y', kind: 'spring', freq: 2, damping: 0.3 }]
      : [{ prop: 'y', kind: 'oscillate', amount: 20 }];
    (l.behaviours ??= []).push(...add);
  }));
  if (!S.selPart) layerLook(l, scene());
  if (VECTOR.includes(l.kind)) adder('+ deformer', ['noise', 'twist', 'bend', 'wave'], v => edit(() => { (l.deformers ??= []).push({ kind: v }); }));
  fxSection(l, () => frameNow()?.layers.find(d => d.id === l.id)?.effects ?? []);
  updateInspector();
}
// Several layers: the properties they all have, one field each. An edit
// sets every one; a field they differ in shows "mixed" until then. ◆ keys
// them all at the playhead, or takes the key there off all of them.
function multiInspector(ls) {
  $('#insp-title').textContent = `${ls.length} layers`;
  const reset = document.createElement('button');
  reset.textContent = 'Reset to default'; reset.dataset.reset = '';
  reset.title = 'Each back to where it was placed: transform keys and offsets cleared';
  reset.onclick = () => resetSelected();
  field('layout', reset);
  const rows = ls.map(l => propsOf(l));
  const shared = rows.at(-1).filter(r => !r.p.includes('.') && rows.every(rs => rs.some(o => o.p === r.p)));
  for (const { p, v } of shared) {
    const color = typeof v === 'string';
    const i = input('', x => edit(() => { for (const l of ls) setValue(l, p, color ? x : Number(x)); }), color ? 'text' : 'number');
    i.dataset.prop = p;
    const k = document.createElement('button');
    k.className = 'key'; k.textContent = '◆'; k.dataset.key = p;
    k.title = 'Add or remove a key at the playhead, on every selected layer';
    const here = l => { const keys = getp(l, p); return isKeys(keys) && keyAt(keys, snap(S.t)) >= 0; };
    k.onclick = () => {
      const on = here(ls.at(-1));
      edit(() => { for (const l of ls) if (here(l) === on) toggleKey(l, p); });
      if (!color) S.prop = p;
      refresh();
    };
    field(p, i, k);
  }
  fxFields = [];
  updateInspector();
}
// An effect stack (a layer's or the scene's): each effect's parameters are
// properties like any other, typed and keyed from the inspector.
let fxFields = [];
function fxSection(owner, live) {
  const box = $('#inspector');
  fxFields = [];
  const head = document.createElement('h3');
  head.className = 'fx-head'; head.textContent = 'Effects';
  const add = document.createElement('select');
  add.id = 'add-effect'; add.setAttribute('aria-label', 'Add an effect');
  // A scene has nothing under it: no backdrop effects there.
  const fits = type => !(Array.isArray(owner.layers) && FX.find(d => d.name === type)?.backdrop);
  const looks = document.createElement('optgroup'); looks.label = 'Looks';
  looks.append(...LOOKS.filter(k => fits(k.fx.type)).map(k => new Option(k.name, 'look:' + k.name)));
  add.append(new Option('+ add', ''), ...FX.filter(d => fits(d.name)).map(d => new Option(d.name, d.name)), looks);
  add.onchange = () => {
    const v = add.value, look = LOOKS.find(k => 'look:' + k.name === v);
    if (v) edit(() => { (owner.effects ??= []).push(look ? structuredClone(look.fx) : { type: v }); });
  };
  box.append(head, add);
  (owner.effects ?? []).forEach((e, i) => {
    const def = FX.find(d => d.name === e.type);
    const title = document.createElement('div');
    title.className = 'fx-title'; title.textContent = e.type; title.title = def?.about ?? '';
    const rm = document.createElement('button');
    rm.className = 'key'; rm.textContent = '✕'; rm.title = 'Remove the effect';
    rm.onclick = () => edit(() => { owner.effects.splice(i, 1); if (!owner.effects.length) delete owner.effects; });
    box.append(title, rm);
    if (def?.modes) {
      const m = choice(e.mode ?? def.modes[0], def.modes, v => edit(() => { e.mode = v; }));
      m.dataset.fx = `${i}.mode`;
      field('mode', m);
    }
    for (const p of def?.params ?? []) {
      const color = typeof p.default === 'string';
      const get = () => live()[i]?.[p.name] ?? p.default;
      const inp = input(color ? get() : round(get()), v => edit(() => setValue(e, p.name, color ? v : Number(v))), color ? 'text' : 'number');
      inp.dataset.fx = `${i}.${p.name}`;
      lock(inp, e[p.name]);
      if (!color) { inp.min = p.min; inp.max = p.max; }
      const k = document.createElement('button');
      k.className = 'key'; k.textContent = '◆'; k.title = 'Add or remove a key at the playhead';
      k.onclick = () => edit(() => toggleKey(e, p.name, get()));
      field(p.name, inp, k);
      fxFields.push({ inp, k, get, keys: () => e[p.name], color });
    }
  });
}
// Reset to default: a part back where the plugin puts it, a layer back to
// its captured layout (`Cut.reset`).
function resetSelected() {
  const l = layer();
  if (!l) return;
  if (S.selPart) { edit(() => { (l.parts ??= {})[S.selPart] = {}; S.selKey = null; }); return; }
  let json = JSON.stringify(S.doc);
  try { for (const o of selectedLayers()) json = cut.reset(json, S.si, o.id); } catch (e) { showError(String(e)); return; }
  edit(() => { S.doc = JSON.parse(json); S.selKey = null; });
}
// The scene's 2D/3D switch. Into 3D nothing moves: the default camera sees
// the z = 0 plane as the 2D frame. Out of 3D, layers go where the camera
// shows them at the playhead (`Cut.flatten`).
function setMode(v) {
  const s = scene();
  if ((s.mode ?? '2d') === v) return;
  if (v === '3d') { edit(() => { s.mode = '3d'; }); return; }
  let json;
  try { json = cut.flatten(JSON.stringify(S.doc), S.si, S.t); } catch (e) { showError(String(e)); return; }
  edit(() => { S.doc = JSON.parse(json); S.selKey = null; });
}
// Values follow the playhead without rebuilding the panel.
export function updateInspector() {
  S.framed = undefined;
  const l = layer();
  if (!l) {
    const bg = $('#inspector [data-bg]');
    if (bg?.disabled) bg.value = frameNow()?.background ?? bg.value;
    updateFx(); return;
  }
  // With several layers selected, a value they do not share is null.
  const ls = S.selPart ? [l] : selectedLayers();
  const vals = {};
  for (const [n, o] of ls.entries()) for (const { p, v } of propsOf(o)) {
    const x = typeof v === 'string' ? v : round(v);
    vals[p] = n === 0 || vals[p] === x ? x : null;
  }
  for (const i of document.querySelectorAll('#inspector input[data-prop]')) {
    if (document.activeElement === i) continue;
    const v = vals[i.dataset.prop];
    i.value = v ?? '';
    i.placeholder = v === null ? 'mixed' : '';
  }
  for (const b of document.querySelectorAll('#inspector [data-key]')) {
    const v = getp(l, b.dataset.key);
    b.className = 'key' + (isKeys(v) ? (keyAt(v, snap(S.t)) >= 0 ? ' here' : ' animated') : '');
  }
  updateFx();
}
function updateFx() {
  for (const f of fxFields) {
    if (document.activeElement !== f.inp) f.inp.value = f.color ? f.get() : round(f.get());
    const v = f.keys();
    f.k.className = 'key' + (isKeys(v) ? (keyAt(v, snap(S.t)) >= 0 ? ' here' : ' animated') : '');
  }
}

