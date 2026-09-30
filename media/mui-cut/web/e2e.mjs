// The web editor end to end in headless Chrome, over the DevTools protocol
// (Node >= 22, no packages): drag a layer, bend a bezier handle, undo/redo,
// and see an outside edit of the file appear. Screenshots land in OUT_DIR.
//
//   media/mui-cut/web/build.sh
//   node media/mui-cut/web/e2e.mjs <path/to/mui-cut binary> <OUT_DIR>
//
// E2E_BACKEND=cpu runs Chrome without WebGPU and expects the CPU renderer,
// E2E_BACKEND=webgl2 without WebGPU and expects WebGL2 (vello_gpu); the
// default expects the viewport on WebGPU. E2E_RENDERER=classic|gpu|...
// forces the editor's `?renderer=`.
import { spawn } from 'node:child_process';
import { inflateSync } from 'node:zlib';
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const [bin, out] = process.argv.slice(2);
if (!bin || !out) { console.error('usage: node e2e.mjs <mui-cut binary> <out dir>'); process.exit(2); }
mkdirSync(out, { recursive: true });
const here = dirname(fileURLToPath(import.meta.url));
const file = join(out, 'e2e.cut.json');
copyFileSync(join(here, '../examples/demo.cut.json'), file);
for (const f of ['mark.svg', 'spin.json']) copyFileSync(join(here, '../examples', f), join(out, f));
const read = () => JSON.parse(readFileSync(file, 'utf8'));
const sleep = ms => new Promise(r => setTimeout(r, ms));
const check = (ok, what) => { if (!ok) throw new Error('FAILED: ' + what); console.log('ok  ' + what); };

const mode = process.env.E2E_BACKEND ?? 'webgpu';
const expected = { cpu: 'CPU', webgl2: 'WebGL2' }[mode] ?? 'WebGPU';
const angle = ['--enable-features=Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist'];
const gpuFlags = { cpu: ['--disable-features=WebGPU'], webgl2: ['--disable-features=WebGPU,Vulkan', '--use-angle=vulkan', '--ignore-gpu-blocklist'] }[mode]
  ?? ['--enable-unsafe-webgpu', ...angle];
// Headless Chrome keeps WebGL2 in workers whatever the switches say, so the
// CPU run asks the editor for its CPU renderer.
const renderer = process.env.E2E_RENDERER || (mode === 'cpu' ? 'cpu' : '');
const query = renderer ? `?renderer=${renderer}` : '';
// E2E_PORT / E2E_CDP_PORT move them when another run holds the defaults.
const port = +(process.env.E2E_PORT ?? 8790), cdpPort = +(process.env.E2E_CDP_PORT ?? 9339);

// The one pixel of a 1x1 PNG screenshot (8-bit RGB or RGBA, one IDAT run).
function pixel(png) {
  const idat = [];
  for (let o = 8; o < png.length;) {
    const n = png.readUInt32BE(o), type = png.toString('ascii', o + 4, o + 8);
    if (type === 'IDAT') idat.push(png.subarray(o + 8, o + 8 + n));
    o += 12 + n;
  }
  return [...inflateSync(Buffer.concat(idat)).subarray(1, 4)];
}
const server = spawn(bin, ['serve', file, '--port', String(port)], { stdio: 'inherit' });
const chrome = spawn(process.env.CHROME ?? 'google-chrome-stable', [
  '--headless=new', `--remote-debugging-port=${cdpPort}`, `--user-data-dir=${join(out, 'chrome-profile')}`,
  '--no-first-run', '--window-size=1600,1000', ...gpuFlags, 'about:blank'], { stdio: 'ignore' });
let failed = false;
try {
  let targets;
  for (let i = 0; i < 100 && !targets; i++) {
    try { targets = await (await fetch(`http://127.0.0.1:${cdpPort}/json`)).json(); } catch { await sleep(100); }
  }
  const ws = new WebSocket(targets.find(t => t.type === 'page').webSocketDebuggerUrl);
  await new Promise(r => ws.onopen = r);
  let id = 0; const pending = new Map(), errors = [];
  ws.onmessage = m => {
    const d = JSON.parse(m.data);
    if (pending.has(d.id)) { pending.get(d.id)(d.result); pending.delete(d.id); }
    if (d.method === 'Runtime.exceptionThrown') errors.push(d.params.exceptionDetails.exception?.description);
  };
  const send = (method, params = {}) => new Promise(r => { pending.set(++id, r); ws.send(JSON.stringify({ id, method, params })); });
  const js = async e => (await send('Runtime.evaluate', { expression: e, returnByValue: true, awaitPromise: true })).result.value;
  const rect = s => js(`(r => [r.left, r.top])(document.querySelector(${JSON.stringify(s)}).getBoundingClientRect())`);
  const mouse = (type, x, y, modifiers = 0) => send('Input.dispatchMouseEvent', { type, x, y, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1, modifiers });
  const click = async s => { const [x, y] = await rect(s); await mouse('mousePressed', x + 12, y + 8); await mouse('mouseReleased', x + 12, y + 8); await sleep(250); };
  const drag = async (x0, y0, x1, y1) => {
    await mouse('mousePressed', x0, y0);
    for (let i = 1; i <= 8; i++) { await mouse('mouseMoved', x0 + (x1 - x0) * i / 8, y0 + (y1 - y0) * i / 8); await sleep(20); }
    await mouse('mouseReleased', x1, y1); await sleep(600);
  };
  const key = async (key, code, modifiers = 0) => {
    await send('Input.dispatchKeyEvent', { type: 'keyDown', key, code, modifiers });
    await send('Input.dispatchKeyEvent', { type: 'keyUp', key, code, modifiers }); await sleep(600);
  };
  const shot = async name => writeFileSync(join(out, name), Buffer.from((await send('Page.captureScreenshot', { format: 'png' })).data, 'base64'));

  await send('Runtime.enable');
  await send('Emulation.setDeviceMetricsOverride', { width: 1600, height: 1000, deviceScaleFactor: 1, mobile: false });
  await send('Page.navigate', { url: `http://127.0.0.1:${port}/${query}` });
  for (let i = 0; i < 100 && (await js(`document.querySelector('#status')?.textContent`)) !== 'loaded'; i++) await sleep(100);
  check(await js(`document.querySelector('#status').textContent`) === 'loaded', 'the editor loads the project');
  const backend = await js(`document.querySelector('#backend').textContent`);
  const engine = await js(`document.querySelector('#backend').dataset.engine`);
  check(backend === expected, `the viewport draws on ${backend} (${engine})`);
  await shot('editor-title.png');

  // Playback: frames the worker finished in two seconds.
  const draws = () => js(`+document.querySelector('#view').dataset.draws`);
  const busy = () => js(`+document.querySelector('#view').dataset.ms`);
  const [d0, b0] = [await draws(), await busy()];
  // Pacing: the gaps between frames coming back while playing.
  const pacing = () => js(`(() => { const s = pacing.shown, d = s.slice(1).map((v, i) => v - s[i]), n = d.length || 1;
    const m = d.reduce((a, b) => a + b, 0) / n, q = x => [...d].sort((a, b) => a - b)[Math.floor(x * (d.length - 1))] ?? 0;
    return { n: d.length, mean: m, sd: Math.sqrt(d.reduce((a, b) => a + (b - m) ** 2, 0) / n), p50: q(0.5), p95: q(0.95), dropped: pacing.dropped }; })()`);
  const report = (what, p) => console.log(`    ${what}: ${p.n} gaps, ${(1000 / p.mean).toFixed(1)} frames/s, gap ${p.mean.toFixed(1)} ± ${p.sd.toFixed(1)} ms (p50 ${p.p50.toFixed(1)}, p95 ${p.p95.toFixed(1)}), ${p.dropped} dropped`);
  await click('#play'); await sleep(2000); const [d1, b1] = [await draws(), await busy()]; const free = await pacing(); await click('#play'); await key('Home', 'Home');
  console.log(`    playback: ${((d1 - d0) / 2).toFixed(0)} frames/s on ${backend} (${engine}), ${((b1 - b0) / (d1 - d0)).toFixed(1)} ms per draw in the worker`);
  report('free-running', free);
  check(d1 - d0 > 20, 'playback keeps drawing');
  // Locked to the project's 30 fps grid: each grid frame is drawn once.
  await click('#lock'); await click('#hud-toggle');
  await click('#play'); await sleep(2000); const locked = await pacing();
  const hud = await js(`document.querySelector('#hud').textContent`);
  await shot('editor-hud.png'); await click('#play'); await click('#lock'); await click('#hud-toggle'); await key('Home', 'Home');
  report('fps-locked', locked);
  check(locked.n > 40 && Math.abs(locked.mean - 1000 / 30) < 6, 'fps-locked playback lands on the 30 fps grid');
  check(/fps .* dropped/.test(hud) && /p50\/p95/.test(hud), 'the pacing HUD shows fps, drops and frame times');

  // The shapes scene, its card selected from the layer list and dragged.
  await click('#scenes button:nth-child(2)');
  await click('#layers button:nth-child(3)');
  const [vx, vy] = await rect('#overlay');
  const [vw, vh] = await js(`(r => [r.width, r.height])(document.querySelector('#overlay').getBoundingClientRect())`);
  const cx = vx + 240 / 1280 * vw, cy = vy + 300 / 720 * vh;
  await drag(cx, cy, cx, cy + 100);
  const card = read().scenes[1].layers.find(l => l.id === 'card');
  check(card.y > 300 && !Array.isArray(card.y), `dragging a plain y sets its value (${card.y})`);
  check(card.x[0].v === 240, 'dragging at a key leaves x on that key');

  // The ball's y in the graph: bend key 1's out handle; the in handle follows.
  await click('#layers button:nth-child(2)');
  check(await js(`document.querySelector('#graph-prop').value`) === 'y', 'the graph shows the first animated property');
  const [gx, gy] = await rect('#graph');
  const [hx, hy, hy2] = await js(`(m => [m.gx(1.0), m.gy(360), m.gy(300)])(document.querySelector('#graph')._map)`);
  await drag(gx + hx, gy + hy, gx + hx, gy + hy2);
  let k1 = read().scenes[1].layers.find(l => l.id === 'ball').y[1];
  check(k1.out[1] < -50 && k1.in[1] > 50, `a handle drag writes mirrored tangents (${JSON.stringify(k1)})`);
  await shot('editor-graph.png');
  await key('z', 'KeyZ', 2);
  k1 = read().scenes[1].layers.find(l => l.id === 'ball').y[1];
  check(k1.out[1] === 0, 'Ctrl+Z restores the file');
  await key('z', 'KeyZ', 2 | 8);
  k1 = read().scenes[1].layers.find(l => l.id === 'ball').y[1];
  check(k1.out[1] < -50, 'Ctrl+Shift+Z redoes it');

  // What an agent sees (`mui-cut mcp` editor_state) and how it points.
  await sleep(400);
  const state = (await (await fetch(`http://127.0.0.1:${port}/state`)).json()).state;
  check(state?.scene === 'shapes' && state.selection === 'ball' && state.prop === 'y', `the editor reports its state (${JSON.stringify(state)})`);
  await fetch(`http://127.0.0.1:${port}/control`, { method: 'POST', body: JSON.stringify({ scene: 'title', t: 1.25, select: 'bar' }) });
  await sleep(600);
  check(await js(`document.querySelector('#time').textContent.startsWith('1.25 s')`), 'an agent moves the playhead');
  check(await js(`document.querySelector('#layers button.on')?.textContent.includes('bar')`), 'and the selection and scene');
  await click('#scenes button:nth-child(2)'); await key('Home', 'Home');

  // An agent edits the file: the open editor reloads it.
  writeFileSync(file, readFileSync(file, 'utf8').replace('"background": "#12131a"', '"background": "#401010"'));
  await sleep(1200);
  check(await js(`document.querySelector('#status').textContent`) === 'reloaded from disk', 'an outside edit reloads the editor');
  const [ox, oy] = await rect('#view');
  const clip = { x: ox + 4, y: oy + 4, width: 1, height: 1, scale: 1 };
  const px = pixel(Buffer.from((await send('Page.captureScreenshot', { format: 'png', clip })).data, 'base64'));
  check(px[0] === 0x40 && px[1] === 0x10, `the viewport shows it (${px})`);
  await shot('editor-reloaded.png');

  // The showcase: SVG and Lottie files reach the viewport, and the inspector
  // edits animators.
  writeFileSync(file, readFileSync(join(here, '../examples/showcase.cut.json'), 'utf8'));
  await sleep(1200);
  await click('#scenes button:nth-child(3)'); await sleep(800);
  const at = async (x, y) => {
    const [ox, oy] = await rect('#view');
    const [w, h] = await js(`(r => [r.width, r.height])(document.querySelector('#view').getBoundingClientRect())`);
    return pixel(Buffer.from((await send('Page.captureScreenshot', { format: 'png', clip: { x: ox + x / 1280 * w, y: oy + y / 720 * h, width: 1, height: 1, scale: 1 } })).data, 'base64'));
  };
  const tri = await at(309, 372);
  check(tri[0] > 0xf0 && tri[1] > 0xb0 && tri[2] < 0x80, `the SVG layer draws its file (${tri})`);
  const dot = await at(760, 360);
  check(dot[0] > 0xf0 && dot[1] < 0x80, `the Lottie layer draws its file (${dot})`);
  await shot('editor-import.png');
  await click('#scenes button:nth-child(1)');
  await click('#layers button:nth-child(2)');
  check(await js(`[...document.querySelectorAll('#inspector .section span')].map(s => s.textContent).join()`) === 'Animator 1', 'the inspector shows the animator');
  check(await js(`[...document.querySelector('#graph-prop').options].some(o => o.value === 'animators.0.start')`), 'the graph offers animator properties');
  await js(`(s => { s.value = 'cascade'; s.dispatchEvent(new Event('change')); })(document.querySelector('[data-adder="+ animator"]'))`);
  await sleep(800);
  const typed = read().scenes[0].layers.find(l => l.id === 'typed');
  check(typed.animators.length === 2 && typed.animators[1].stagger === 0.04, 'a preset animator lands in the file');
  await js(`document.querySelector('#right').scrollTop = 1e6`); await sleep(200);
  await shot('editor-animators.png');

  // A 3D scene: the camera's properties are keyable like any other, the
  // viewport draws the shot (flat with a notice where there is no 3D pass),
  // and the orbit preview moves the view without touching the file.
  copyFileSync(join(here, '../examples/knot.glb'), join(out, 'knot.glb'));
  writeFileSync(file, readFileSync(join(here, '../examples/stage3d.cut.json'), 'utf8'));
  await sleep(1500);
  await click('#scenes button:nth-child(1)'); await sleep(800);
  await js(`document.querySelector('#layers button:last-child').click()`); await sleep(400);
  check(await js(`[...document.querySelector('#graph-prop').options].some(o => o.value === 'distance')`), 'the camera\'s distance is in the graph');
  const notice = await js(`document.querySelector('#notice').hidden ? '' : document.querySelector('#notice').textContent`);
  check(backend === 'WebGPU' ? notice === '' : /flat/.test(notice), `3D draws ${notice || 'in 3D'}`);
  await shot('editor-3d.png');
  if (backend === 'WebGPU') {
    const before = readFileSync(file, 'utf8'), d3 = await draws();
    await click('#orbit');
    check(await js(`document.querySelector('#orbit').getAttribute('aria-pressed')`) === 'true', 'the orbit preview turns on');
    const [qx, qy] = await rect('#overlay');
    await drag(qx + 400, qy + 250, qx + 600, qy + 300);
    check(await draws() > d3 && readFileSync(file, 'utf8') === before, 'orbiting redraws and leaves the file alone');
    await shot('editor-3d-orbit.png');
    await click('#orbit');
  }
  check(errors.length === 0, 'no page exceptions ' + errors.join('; '));
  ws.close();
} catch (e) {
  console.error(e.message); failed = true;
} finally {
  chrome.kill(); server.kill();
}
process.exit(failed ? 1 : 0);
