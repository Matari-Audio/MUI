import { $, S } from './state.js';

// ---------- field-level patches: JSON Pointer ops that address layers,
// scenes and sources by id or name, as `serve` merges them (serve.rs).
export const clone = v => v === undefined ? undefined : structuredClone(v);
export const plain = v => JSON.parse(JSON.stringify(v));
const obj = v => v !== null && typeof v === 'object' && !Array.isArray(v);
export function same(a, b) {
  if (a === b) return true;
  if (Array.isArray(a)) return Array.isArray(b) && a.length === b.length && a.every((v, i) => same(v, b[i]));
  if (!obj(a) || !obj(b)) return false;
  const ka = Object.keys(a);
  return ka.length === Object.keys(b).length && ka.every(k => k in b && same(a[k], b[k]));
}
const esc = k => String(k).replace(/~/g, '~0').replace(/\//g, '~1');
function itemKeys(a) {
  const f = ['id', 'name'].find(f => a.every(v => typeof v?.[f] === 'string'));
  const ks = f && a.map(v => v[f]);
  return ks && ks.every(k => k !== '-' && !/^\d+$/.test(k)) && new Set(ks).size === ks.length ? ks : null;
}
export function diff(a, b, path = '', out = []) {
  if (same(a, b)) return out;
  if (obj(a) && obj(b)) {
    for (const k in a) {
      if (k in b) diff(a[k], b[k], `${path}/${esc(k)}`, out); else out.push({ op: 'remove', path: `${path}/${esc(k)}` });
    }
    for (const k in b) if (!(k in a)) out.push({ op: 'add', path: `${path}/${esc(k)}`, value: b[k] });
    return out;
  }
  const ka = Array.isArray(a) && Array.isArray(b) && itemKeys(a), kb = ka && itemKeys(b);
  if (kb && ka.filter(k => kb.includes(k)).join('\0') === kb.filter(k => ka.includes(k)).join('\0')) {
    for (const k of ka) if (!kb.includes(k)) out.push({ op: 'remove', path: `${path}/${esc(k)}` });
    ka.forEach((k, i) => { const j = kb.indexOf(k); if (j >= 0) diff(a[i], b[j], `${path}/${esc(k)}`, out); });
    kb.forEach((k, j) => { if (!ka.includes(k)) out.push({ op: 'add', path: `${path}/${j}`, value: b[j] }); });
    return out;
  }
  out.push({ op: 'replace', path, value: b });
  return out;
}
// A pointer's tokens, items named by id (first) or name turned into indices.
function resolve(root, path) {
  if (path === '') return [];
  let node = root;
  return path.slice(1).split('/').map(raw => {
    let tok = raw.replace(/~1/g, '/').replace(/~0/g, '~');
    if (Array.isArray(node) && tok !== '-' && !/^\d+$/.test(tok)) {
      let i = node.findIndex(v => v?.id === tok);
      if (i < 0) i = node.findIndex(v => v?.name === tok);
      if (i < 0) throw new Error(`nothing named ${tok}`);
      tok = String(i);
    }
    node = node?.[tok];
    return tok;
  });
}
export const at = (root, path) => { try { return resolve(root, path).reduce((o, k) => o?.[k], root); } catch { return undefined; } };
// One op, in place (objects off its path keep their identity, so a drag in
// progress keeps writing into the layer it holds).
function applyOp(root, { op, path, value }) {
  const ts = resolve(root, path), last = ts.pop(), parent = ts.reduce((o, k) => o?.[k], root);
  if (last === undefined || !parent || typeof parent !== 'object') throw new Error('no place for ' + path);
  value = clone(value);
  if (Array.isArray(parent)) {
    const i = last === '-' ? parent.length : +last;
    if (op === 'add' ? i > parent.length : i >= parent.length) throw new Error('nothing at ' + path);
    // An item added by id (or name) that is there already, e.g. our own
    // in-flight add back in the server's doc, replaces it: never twice.
    const k = op === 'add' && ['id', 'name'].find(f => typeof value?.[f] === 'string');
    const j = k ? parent.findIndex(v => v?.[k] === value[k]) : -1;
    if (j >= 0) { parent[j] = value; return; }
    if (op === 'add') parent.splice(i, 0, value); else if (op === 'remove') parent.splice(i, 1); else parent[i] = value;
  } else {
    if (op !== 'add' && !(last in parent)) throw new Error('nothing at ' + path);
    if (op === 'remove') delete parent[last]; else parent[last] = value;
  }
}
// Every op that still applies; the paths of the ones that do not.
export function applyAll(root, ops) {
  const failed = [];
  for (const o of ops) try { applyOp(root, o); } catch { failed.push(o.path); }
  return failed;
}
export const touch = (a, b) => a === b || a.startsWith(b + '/') || b.startsWith(a + '/');
export const who = by => by === 'disk' ? 'the file on disk' : by?.startsWith('editor-') ? 'another editor' : 'the agent';
export const where = paths => [...new Set(paths)].map(p => p.replace(/^\/scenes\//, '')).join(', ');
// A short notice of a merge, cleared after a while.
let noticeTimer = 0;
export function notice(text) {
  const n = $('#merge');
  n.textContent = text; n.hidden = !text;
  clearTimeout(noticeTimer);
  if (text) noticeTimer = setTimeout(() => { n.hidden = true; }, 8000);
}
globalThis.cutRev = () => S.rev; // the e2e waits for a merge with it

