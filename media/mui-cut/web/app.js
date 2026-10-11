// mui-cut web editor: the entry module. Each panel is its own module
// (state.js holds what they share); importing them wires their controls,
// then the viewport's worker starts and the project loads from `serve`.
import './doc.js';
import './patch.js';
import './edit.js';
import './lists.js';
import './sources.js';
import './inspector.js';
import './viewport.js';
import './timeline.js';
import './graph.js';
import './export.js';
import './transport.js';
import './sound.js';
import './agent.js';
import './panels.js';
import './align.js';
import { control } from './agent.js';
import { adopt, captured, loadAssets, status } from './edit.js';
import { who } from './patch.js';
import { showLive } from './sound.js';
import { $, S, assets, me } from './state.js';
import { loop } from './transport.js';
import { initViewport } from './viewport.js';

await initViewport();
addEventListener('resize', () => { S.need = true; });

$('#file').textContent = await (await fetch('/name')).text();
for (const p of await (await fetch('/captures')).json()) captured.add(p);
await adopt(await (await fetch('/doc')).json(), 'loaded');
const events = new EventSource('/events');
events.addEventListener('doc', e => {
  const m = JSON.parse(e.data);
  adopt(m, m.by === me ? null : m.by === 'disk' ? 'reloaded from disk' : `merged ${who(m.by)}'s edit`);
});
// The file on disk is not JSON: say so, keep the last good revision.
events.onmessage = async () => {
  const text = await (await fetch('/project')).text();
  try { JSON.parse(text); } catch (e) { status('the file on disk has an error: ' + e, true); }
};
events.addEventListener('control', e => control(JSON.parse(e.data)));
// `serve` wrote these plugin states (new, or made again): fetch them.
events.addEventListener('plugin', e => {
  for (const p of JSON.parse(e.data)) { captured.add(p); assets.delete(p); }
  loadAssets();
});
events.addEventListener('live', e => showLive(JSON.parse(e.data)));
requestAnimationFrame(loop);
