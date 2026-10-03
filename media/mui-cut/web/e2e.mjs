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
import { spawn, spawnSync } from 'node:child_process';
import { inflateSync } from 'node:zlib';
import { copyFileSync, existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { createInterface } from 'node:readline';
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
const cpu = expected === 'CPU';
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
// The live sound on the null device: a real clock, no speaker.
const server = spawn(bin, ['serve', file, '--port', String(port)], { stdio: 'inherit', env: { ...process.env, MUI_CUT_AUDIO: 'null' } });
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
  // `free` holds Ctrl once moving: no snapping, for exact distances.
  const drag = async (x0, y0, x1, y1, free = false) => {
    await mouse('mousePressed', x0, y0);
    for (let i = 1; i <= 8; i++) { await mouse('mouseMoved', x0 + (x1 - x0) * i / 8, y0 + (y1 - y0) * i / 8, free ? 2 : 0); await sleep(20); }
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
  // The sound: play starts `serve`'s audio clock where the playhead is,
  // the editor's playhead follows it, and pause stops it.
  const transport = async () => (await fetch(`http://127.0.0.1:${port}/transport`)).json();
  await click('#play'); await sleep(400);
  const [a0, e0] = [await transport(), await js('cutTime()')]; await sleep(1000);
  const [a1, e1] = [await transport(), await js('cutTime()')];
  await click('#play');
  const ran = a1.t - a0.t;
  check(a1.playing && a1.device === 'null' && Math.abs(ran - 1) < 0.08, `playing runs the audio clock (${ran.toFixed(3)} s in 1 s on ${a1.device})`);
  check(Math.abs((e1 - a1.t) - (e0 - a0.t)) < 0.1 && Math.abs(e1 - a1.t) < 0.15, `the playhead keeps with it (editor ${e1.toFixed(3)}, audio ${a1.t.toFixed(3)})`);
  check(a1.latency_ms < 50 && /null · \d+ ms/.test(await js(`document.querySelector('#audio-state').textContent`)), `the editor shows the latency (${a1.latency_ms.toFixed(1)} ms)`);
  await sleep(300);
  check(!(await transport()).playing, 'pause stops the audio clock');
  await key('Home', 'Home');

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

  // An agent (`mui-cut mcp`, finding this editor by its discovery file)
  // edits while the person drags the card: its edit to another layer
  // merges, its edit to the dragged field is noticed and the drag, later,
  // wins; undo takes back only the drag.
  const agent = spawn(bin, ['mcp'], { stdio: ['pipe', 'pipe', 'inherit'], env: { ...process.env, MUI_CUT_AUDIO: 'null' } });
  const replies = createInterface({ input: agent.stdout })[Symbol.asyncIterator]();
  let rpc = 0;
  const mcp = async (name, args) => {
    agent.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: ++rpc, method: 'tools/call', params: { name, arguments: args } }) + '\n');
    const r = JSON.parse((await replies.next()).value).result;
    if (r.isError) throw new Error(`mcp ${name}: ${r.content[0].text}`);
    return r.content[0].text;
  };
  agent.stdin.write(JSON.stringify({ jsonrpc: '2.0', id: ++rpc, method: 'initialize', params: { protocolVersion: '2025-06-18' } }) + '\n');
  await replies.next();
  await mcp('open', { path: file });
  const y0 = read().scenes[1].layers.find(l => l.id === 'card').y;
  const [dx0, dy0] = [vx + 240 / 1280 * vw, vy + y0 / 720 * vh];
  await mouse('mousePressed', dx0, dy0);
  for (let i = 1; i <= 4; i++) { await mouse('mouseMoved', dx0, dy0 + i * 10, 2); await sleep(20); }
  const rev0 = await js('cutRev()');
  const said = await mcp('set', { scene: 'shapes', layer: 'ball', prop: 'fill', value: '#00ff00' });
  check(/through the editor on port/.test(said), `an agent's edit goes through the editor's server (${said.split('\n')[0]})`);
  await mcp('set', { scene: 'shapes', layer: 'card', prop: 'y', value: 123 });
  await sleep(500);
  check(await js('cutRev()') >= rev0 + 2, 'the editor takes both of the agent\'s edits mid-drag');
  const note = await js(`document.querySelector('#merge').hidden ? '' : document.querySelector('#merge').textContent`);
  check(/card\/y/.test(note) && /later edit wins/.test(note), `it notices the field both changed (${note})`);
  for (let i = 5; i <= 8; i++) { await mouse('mouseMoved', dx0, dy0 + i * 10, 2); await sleep(20); }
  await mouse('mouseReleased', dx0, dy0 + 80); await sleep(800);
  let both = read().scenes[1].layers;
  const dragged = both.find(l => l.id === 'card').y;
  check(both.find(l => l.id === 'ball').fill === '#00ff00', 'the agent\'s edit to another layer survives the drag');
  check(Math.abs(dragged - (y0 + 80 / vh * 720)) < 2, `the drag, the later edit, wins its field (${y0} -> ${dragged})`);
  await key('z', 'KeyZ', 2);
  both = read().scenes[1].layers;
  check(both.find(l => l.id === 'card').y === 123 && both.find(l => l.id === 'ball').fill === '#00ff00',
    `undo takes back the drag only (${both.find(l => l.id === 'card').y}, ${both.find(l => l.id === 'ball').fill})`);
  agent.stdin.end(); agent.kill();

  // A scene effect from the inspector: levels with no saturation greys the
  // frame on WebGPU; the CPU viewport says it draws without effects.
  await mouse('mousePressed', ox + 4, oy + 4); await mouse('mouseReleased', ox + 4, oy + 4); await sleep(300);
  await js(`(() => { const s = document.querySelector('#add-effect'); s.value = 'levels'; s.onchange(); })()`);
  await sleep(1000);
  check(read().scenes[1].effects?.[0]?.type === 'levels', 'adding an effect writes it to the file');
  await js(`(() => { const i = document.querySelector('[data-fx="0.saturation"]'); i.value = '0'; i.onchange(); })()`);
  await sleep(1200);
  check(read().scenes[1].effects[0].saturation === 0, 'an effect parameter is saved');
  const grey = pixel(Buffer.from((await send('Page.captureScreenshot', { format: 'png', clip })).data, 'base64'));
  if (cpu) check(await js(`document.querySelector('#backend').textContent`) === 'CPU · effects off', 'the CPU viewport says effects are off');
  else check(Math.abs(grey[0] - grey[1]) <= 2 && Math.abs(grey[1] - grey[2]) <= 2 && grey[0] > 0x10, `the viewport draws the effect (${grey}) ${await js(`document.querySelector('#error').textContent`)}`);
  await shot('editor-effect.png');
  // Looks: a preset effect from the picker, its mode a choice; a scene's
  // picker leaves out the effects that read what is under a layer.
  const picks = await js(`[...document.querySelectorAll('#add-effect option')].map(o => o.textContent)`);
  check(picks.includes('Soft bloom') && picks.includes('Neon') && !picks.includes('glass') && !picks.includes('Frosted glass'),
    `the scene's picker has the looks it can run (${picks.join(', ')})`);
  await js(`(() => { const s = document.querySelector('#add-effect'); s.value = 'look:Soft bloom'; s.onchange(); })()`);
  await sleep(1000);
  const bloom = read().scenes[1].effects?.[1];
  check(bloom?.type === 'glow' && bloom.mode === 'bloom' && bloom.radius === 90, `a look adds its effect, set (${JSON.stringify(bloom)})`);
  await js(`(() => { const m = document.querySelector('[data-fx="1.mode"]'); m.value = 'neon'; m.onchange(); })()`);
  await sleep(1000);
  check(read().scenes[1].effects[1].mode === 'neon', 'an effect mode is saved');
  if (!cpu) check(!(await js(`document.querySelector('#error').textContent`)), 'the viewport draws the glow');
  // Export: WebCodecs H.264 in the worker, muxed to MP4, every frame of
  // every scene, with the server's mix of the sound (a tone on an audio
  // layer here); a cancel stops a second run.
  const tone = Buffer.alloc(44 + 2 * 96000);
  tone.write('RIFF', 0); tone.writeUInt32LE(36 + 2 * 96000, 4); tone.write('WAVEfmt ', 8);
  tone.writeUInt32LE(16, 16); tone.writeUInt16LE(1, 20); tone.writeUInt16LE(1, 22); tone.writeUInt32LE(48000, 24);
  tone.writeUInt32LE(96000, 28); tone.writeUInt16LE(2, 32); tone.writeUInt16LE(16, 34); tone.write('data', 36); tone.writeUInt32LE(2 * 96000, 40);
  for (let i = 0; i < 96000; i++) tone.writeInt16LE(Math.round(12000 * Math.sin(2 * Math.PI * 440 * i / 48000)), 44 + 2 * i);
  writeFileSync(join(out, 'tone.wav'), tone);
  const withTone = read();
  withTone.scenes[0].layers.push({ id: 'tone', kind: 'audio', path: 'tone.wav' });
  writeFileSync(file, JSON.stringify(withTone, null, 2));
  await sleep(1200);
  await send('Page.setDownloadBehavior', { behavior: 'deny' });
  await js(`document.querySelector('#export').click()`);
  for (let i = 0; i < 50 && await js(`!document.querySelector('#export-dialog').open || document.querySelector('#ex-start').disabled`); i++) await sleep(100);
  await js(`(() => { document.querySelector('#ex-codec').value = 'avc1.640028'; document.querySelector('#ex-mb').value = '${cpu ? 1 : 4}'; document.querySelector('#ex-start').click(); })()`);
  const t0 = Date.now();
  let ex = null;
  for (let i = 0; i < 600 && !ex; i++) {
    await sleep(100);
    ex = await js(`window.lastExport ? { frames: lastExport.frames, size: lastExport.blob.size, sound: lastExport.sound, hardware: lastExport.hardware } : null`);
    const st = await js(`document.querySelector('#ex-status').textContent`);
    if (st.startsWith('failed')) throw new Error('export ' + st);
  }
  check(ex && ex.frames === 225, `export renders every frame (${ex?.frames}, ${((Date.now() - t0) / 1000).toFixed(1)} s, ${ex?.hardware})`);
  await shot('editor-export.png');
  const b64 = await js(`lastExport.blob.arrayBuffer().then(b => { let s = ''; const u = new Uint8Array(b); for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000)); return btoa(s); })`);
  const mp4 = join(out, 'export.mp4');
  writeFileSync(mp4, Buffer.from(b64, 'base64'));
  const probe = spawnSync('ffprobe', ['-v', 'error', '-count_frames', '-select_streams', 'v:0', '-show_entries', 'stream=codec_name,width,height,nb_read_frames:format=duration', '-of', 'default=nw=1', mp4], { encoding: 'utf8' });
  check(probe.status === 0 && /codec_name=h264/.test(probe.stdout) && /nb_read_frames=225/.test(probe.stdout) && /width=1280/.test(probe.stdout), `ffprobe reads the export (${probe.stdout.replace(/\n/g, ' ')}${probe.stderr})`);
  const dur = +/duration=([\d.]+)/.exec(probe.stdout)?.[1];
  check(Math.abs(dur - 7.5) < 0.05, `the export lasts 7.5 s (${dur})`);
  const aprobe = spawnSync('ffprobe', ['-v', 'error', '-select_streams', 'a:0', '-show_entries', 'stream=codec_name,channels,duration', '-of', 'default=nw=1', mp4], { encoding: 'utf8' });
  const adur = +/duration=([\d.]+)/.exec(aprobe.stdout)?.[1];
  check(/codec_name=(aac|opus)/.test(aprobe.stdout) && /channels=2/.test(aprobe.stdout) && Math.abs(adur - 7.5) < 0.1,
    `the export carries the sound, 7.5 s long (${ex.sound}: ${aprobe.stdout.replace(/\n/g, ' ')}${aprobe.stderr})`);
  const vol = spawnSync('ffmpeg', ['-v', 'info', '-i', mp4, '-af', 'volumedetect', '-vn', '-f', 'null', '-'], { encoding: 'utf8' });
  const peak = +/max_volume: (-?[\d.]+) dB/.exec(vol.stderr)?.[1];
  check(peak > -20, `the export's sound is the tone, not silence (max ${peak} dB)`);
  // The other codecs this browser encodes, gated by isConfigSupported.
  const others = await js(`[...document.querySelectorAll('#ex-codec option')].filter(o => !o.disabled && o.value !== 'avc1.640028').map(o => o.value)`);
  console.log(`    browser also encodes: ${others.join(', ') || 'nothing else'}`);
  for (const codec of others) {
    await js(`(() => { window.lastExport = null; document.querySelector('#ex-codec').value = '${codec}'; document.querySelector('#ex-mb').value = '1'; document.querySelector('#ex-start').click(); })()`);
    let done = null;
    for (let i = 0; i < 1200 && !done; i++) { await sleep(100); done = await js(`window.lastExport?.frames ?? (document.querySelector('#ex-status').textContent.startsWith('failed') ? -1 : null)`); }
    const file = join(out, `export-${codec}.mp4`);
    writeFileSync(file, Buffer.from(await js(`lastExport.blob.arrayBuffer().then(b => { let s = ''; const u = new Uint8Array(b); for (let i = 0; i < u.length; i += 0x8000) s += String.fromCharCode(...u.subarray(i, i + 0x8000)); return btoa(s); })`), 'base64'));
    const p = spawnSync('ffprobe', ['-v', 'error', '-count_frames', '-select_streams', 'v:0', '-show_entries', 'stream=codec_name,nb_read_frames', '-of', 'default=nw=1', file], { encoding: 'utf8' });
    const name = { av01: 'av1', hvc1: 'hevc' }[codec.slice(0, 4)];
    check(done === 225 && p.stdout.includes(`codec_name=${name}`) && p.stdout.includes('nb_read_frames=225'), `a ${codec} export plays (${p.stdout.replace(/\n/g, ' ')}${p.stderr})`);
  }

  // A slow run (32 subframes a frame) to cancel part way.
  await js(`(() => { window.lastExport = null; document.querySelector('#ex-mb').value = '32'; document.querySelector('#ex-start').click(); })()`);
  await sleep(300);
  await js(`document.querySelector('#ex-cancel').click()`);
  for (let i = 0; i < 100 && await js(`document.querySelector('#ex-status').textContent`) !== 'cancelled'; i++) await sleep(100);
  const st = await js(`document.querySelector('#ex-status').textContent`);
  check(st === 'cancelled' && !(await js(`window.lastExport`)), `cancel stops an export (${st})`);
  await js(`document.querySelector('#ex-cancel').click()`);

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
  // Its environment and occlusion show on the scene and switch off and on.
  for (let i = 0; i < 50 && !(await js(`!!document.querySelector('[data-look="ao"]')`)); i++) await sleep(100);
  check(await js(`document.querySelector('[data-look="environment"]')?.checked && document.querySelector('[data-look="ao"]')?.checked`), 'the scene shows its environment and occlusion');
  await js(`(c => { c.checked = false; c.dispatchEvent(new Event('change')); })(document.querySelector('[data-look="ao"]'))`); await sleep(800);
  check(!read().scenes[0].ao && read().scenes[0].environment, 'occlusion switches off in the file');
  await js(`(c => { c.checked = true; c.dispatchEvent(new Event('change')); })(document.querySelector('[data-look="ao"]'))`); await sleep(800);
  check(read().scenes[0].ao, 'and back on');
  await js(`document.querySelector('#layers button:last-child').click()`); await sleep(400);
  const hasDistance = () => js(`[...document.querySelector('#graph-prop').options].some(o => o.value === 'distance')`);
  for (let i = 0; i < 50 && !(await hasDistance()); i++) await sleep(100);
  check(await hasDistance(), 'the camera\'s distance is in the graph');
  const notice = await js(`document.querySelector('#notice').hidden ? '' : document.querySelector('#notice').textContent`);
  check(backend === 'WebGPU' ? notice === '' : /flat/.test(notice), `3D draws ${notice || 'in 3D'}`);
  await shot('editor-3d.png');
  if (cpu) {
    // An export never quietly flattens a 3D shot.
    await js(`document.querySelector('#export').click()`);
    for (let i = 0; i < 50 && await js(`!document.querySelector('#export-dialog').open || document.querySelector('#ex-start').disabled`); i++) await sleep(100);
    await js(`(() => { window.lastExport = null; document.querySelector('#ex-mb').value = '1'; document.querySelector('#ex-start').click(); })()`);
    let st = '';
    for (let i = 0; i < 100 && !st.startsWith('failed'); i++) { await sleep(100); st = await js(`document.querySelector('#ex-status').textContent`); }
    check(/failed: .*3D/.test(st) && !(await js(`window.lastExport`)), `the CPU refuses to export a 3D shot flat (${st})`);
    await js(`document.querySelector('#ex-cancel').click()`);
  }
  if (backend === 'WebGPU') {
    const before = readFileSync(file, 'utf8'), d3 = await draws();
    await click('#orbit');
    check(await js(`document.querySelector('#orbit').getAttribute('aria-pressed')`) === 'true', 'the orbit preview turns on');
    const [qx, qy] = await rect('#overlay');
    await drag(qx + 400, qy + 250, qx + 600, qy + 300);
    check(await draws() > d3 && readFileSync(file, 'utf8') === before, 'orbiting redraws and leaves the file alone');
    await shot('editor-3d-orbit.png');
    await click('#orbit');
    // Beauty refines a paused 3D frame sample by sample, and stops at
    // any change to start over.
    await click('#beauty');
    let n = 0;
    for (let i = 0; i < 100 && n < 32; i++) { await sleep(100); n = +(await js(`document.querySelector('#view').dataset.samples`)); }
    check(n >= 32 && readFileSync(file, 'utf8') === before, `the Beauty preview accumulates samples (${n})`);
    await shot('editor-3d-beauty.png');
    await click('#beauty');
  }
  // The scene's Look: a physical sky turned on, its sun dragged on the
  // dial, the sky lighting the scene in place of the environment, and the
  // project's glass mode; a slab takes a glass material from its preset.
  const set = async (sel, v) => { await js(`(s => { s.value = ${JSON.stringify(v)}; s.dispatchEvent(new Event('change')); })(document.querySelector(${JSON.stringify(sel)}))`); await sleep(800); };
  const tick = async (sel, on) => { await js(`(c => { c.checked = ${on}; c.dispatchEvent(new Event('change')); })(document.querySelector(${JSON.stringify(sel)}))`); await sleep(800); };
  await click('#scenes button:nth-child(1)'); await sleep(500);
  if (read().scenes[0].sky) await tick('[data-look="sky"]', false);
  await tick('[data-look="sky"]', true);
  check(read().scenes[0].sky?.model === 'physical', 'the Look section adds a physical sky');
  await js(`document.querySelector('[data-sun-dial]').scrollIntoView({ block: 'center' })`); await sleep(200);
  const [dx, dy, dw] = await js(`(r => [r.left, r.top, r.width])(document.querySelector('[data-sun-dial]').getBoundingClientRect())`);
  // From overhead out to the right on the horizon ring: the sun sets due right.
  await drag(dx + dw / 2, dy + dw / 2, dx + dw / 2 + dw * 0.4, dy + dw / 2);
  const sky = read().scenes[0].sky;
  check(Math.abs(sky.elevation) <= 3 && Math.abs(sky.azimuth - 90) <= 3, `the sun dial sets the sun (${sky.elevation}°, ${sky.azimuth}°)`);
  await set('[data-look="lit"]', 'sky');
  check(!read().scenes[0].environment && read().scenes[0].sky, 'the sky lights the scene in place of the environment');
  await set('[data-look="glass"]', 'rt-path');
  await set('[data-look="glass_samples"]', '64');
  check(read().render?.glass === 'rt-path' && read().render.glass_samples === 64, 'the project\'s glass mode and samples');
  await shot('editor-3d-look.png');
  await set('[data-look="glass"]', 'raster');
  check(!read().render, 'raster glass leaves the file as it was');
  const slab = read().scenes[0].layers.find(l => ['rect', 'ellipse', 'text'].includes(l.kind) && !l.material);
  await click(`#layers [data-layer="${slab.id}"]`);
  for (let i = 0; i < 50 && !(await js(`!!document.querySelector('[data-adder="+ material"]')`)); i++) await sleep(100);
  await set('[data-adder="+ material"]', 'frosted glass');
  const mat = read().scenes[0].layers.find(l => l.id === slab.id).material;
  check(mat?.transmission === 1 && mat.roughness === 0.35, 'a slab takes a frosted glass material');
  check(await js(`[...document.querySelectorAll('#inspector .section span')].some(s => s.textContent === 'Material') && !!document.querySelector('[data-prop="material.roughness"]')`), 'its material shows as keyable rows');
  await js(`document.querySelector('#right').scrollTop = 1e6`); await sleep(200);
  await shot('editor-3d-material.png');
  // A 3D scene's effects run on the 3D pass: levels with no saturation
  // greys a red card on WebGPU (in 3D) and WebGL2 (flat, as before); the
  // CPU draws it red, without effects.
  const redCard = fx => JSON.stringify({ size: [1280, 720], fps: 30, scenes: [{ name: 'fx3d', duration: 1, mode: '3d', background: '#101014',
    layers: [{ id: 'card', kind: 'rect', x: 640, y: 360, width: 900, height: 560, ry: 12, fill: '#e03020' }], ...(fx ? { effects: [{ type: 'levels', saturation: 0 }] } : {}) }] });
  const mid = async () => {
    const [vx, vy, vw, vh] = await js(`(r => [r.left, r.top, r.width, r.height])(document.querySelector('#view').getBoundingClientRect())`);
    const clip = { x: vx + vw / 2, y: vy + vh / 2, width: 1, height: 1, scale: 1 };
    return pixel(Buffer.from((await send('Page.captureScreenshot', { format: 'png', clip })).data, 'base64'));
  };
  writeFileSync(file, redCard(false)); await sleep(1500);
  const red = await mid();
  check(red[0] > red[1] + 80, `the 3D card is red (${red})`);
  writeFileSync(file, redCard(true)); await sleep(1500);
  const fx3 = await mid();
  const greyed = Math.abs(fx3[0] - fx3[1]) <= 3 && Math.abs(fx3[1] - fx3[2]) <= 3 && fx3[0] > 0x20;
  if (cpu) check(!greyed, `the CPU draws the 3D card without effects (${fx3})`);
  else check(greyed, `a 3D scene's effect runs on ${backend} (${fx3})`);
  await shot('editor-3d-effect.png');
  // An overlay layer goes over the 3D shot after its effects: a green
  // caption mid-frame stays green over the greyed card everywhere, and
  // unticked in the inspector it is a 3D layer the effect greys.
  const capped = JSON.parse(redCard(true));
  capped.scenes[0].layers.push({ id: 'cap', kind: 'rect', x: 640, y: 360, width: 240, height: 120, fill: '#20d040', overlay: true });
  writeFileSync(file, JSON.stringify(capped)); await sleep(1500);
  const cap = await mid();
  check(cap[1] > cap[0] + 80, `the overlay is drawn after the 3D pass and its effects (${cap})`);
  await click('#layers [data-layer="cap"]');
  for (let i = 0; i < 50 && !(await js(`!!document.querySelector('[data-overlay]')`)); i++) await sleep(100);
  await js(`(c => { c.checked = false; c.dispatchEvent(new Event('change')); })(document.querySelector('[data-overlay]'))`); await sleep(1500);
  check(!read().scenes[0].layers[1].overlay, 'unticking overlay writes it to the file');
  const flat = await mid();
  if (!cpu) check(Math.abs(flat[0] - flat[1]) <= 3, `a 3D layer goes under the effect (${flat})`);
  await shot('editor-3d-overlay.png');
  // Variables and variants: an agent writes a project with them; the header
  // previews a tall variant, the panel edits that variant's value, and the
  // file keeps its bindings.
  writeFileSync(file, readFileSync(join(here, '../examples/variants.cut.json'), 'utf8'));
  await sleep(1500);
  check(await js(`!document.querySelector('#variant').hidden`), 'a project with variants shows the switcher');
  await js(`(() => { const s = document.querySelector('#variant'); s.value = 'light-tall'; s.onchange(); })()`);
  await sleep(800);
  const [tw, th] = await js(`(r => [r.width, r.height])(document.querySelector('#view').getBoundingClientRect())`);
  check(th > tw * 1.5, `the viewport takes the variant's 1080x1920 (${tw}x${th})`);
  const [lx, ly] = await rect('#view');
  const light = pixel(Buffer.from((await send('Page.captureScreenshot', { format: 'png', clip: { x: lx + 4, y: ly + 4, width: 1, height: 1, scale: 1 } })).data, 'base64'));
  check(Math.abs(light[0] - 0xf4) <= 2 && Math.abs(light[2] - 0xea) <= 2, `the viewport draws the variant's theme (${light})`);
  await shot('editor-variant.png');
  const bgv = await js(`(i => i ? [i.disabled, i.value] : 'none')(document.querySelector('[data-bg]'))`);
  check(bgv[0] === true && bgv[1] === '#f4f1ea', `a bound background shows the variant's value, read-only (${bgv})`);
  await js(`(() => { const i = document.querySelector('[data-var="headline"]'); i.value = 'Hello'; i.onchange(); })()`);
  await sleep(1200);
  const saved = read();
  check(saved.variants.find(v => v.name === 'light-tall').vars.headline === 'Hello' && saved.variables.headline.value === 'Ship it', 'a variable edit goes to the chosen variant');
  check(saved.scenes[0].background.var === 'theme' && saved.scenes[0].layers[0].width.var === 'W', 'the file keeps its bindings');
  await click('#layers button:nth-child(3)');
  check(await js(`document.querySelector('[data-prop="width"]').disabled`), 'a bound property is read-only');


  // A plugin layer: the synth example's real UI, captured by `serve`, its
  // parts in the layer list, one selected and dragged, then exploded.
  // `bin` may be the hashed deps/ copy of the binary; examples sit beside debug/.
  const synth = join(dirname(bin).replace(/\/deps$/, ''), 'examples/synth');
  const plug = JSON.parse(readFileSync(join(here, '../examples/plugin.cut.json'), 'utf8'));
  for (const l of plug.scenes.flatMap(s => s.layers)) if (l.source) l.source = { bin: synth };
  writeFileSync(file, JSON.stringify(plug));
  let parts = 0;
  for (let i = 0; i < 150 && parts < 5; i++) { await sleep(200); parts = await js(`document.querySelectorAll('#layers button.part').length`); }
  check(parts === 5, `the plugin's parts are child layers (${parts})`);
  await fetch(`http://127.0.0.1:${port}/control`, { method: 'POST', body: JSON.stringify({ t: 2 }) });
  await sleep(800);
  await click('#layers button.part[data-part="osc"]');
  check(await js(`document.querySelector('#insp-title').textContent`) === 'Part · osc', 'selecting a part inspects it');
  const q = await js(`cutQuads().find(q => q.id === 'synth#osc').pts`);
  const [px0, py0] = await rect('#overlay');
  const [pw] = await js(`(r => [r.width])(document.querySelector('#overlay').getBoundingClientRect())`);
  const k = pw / 1920, mx = px0 + (q[0][0] + q[2][0]) / 2 * k, my = py0 + (q[0][1] + q[2][1]) / 2 * k;
  // Inside the panel, off its controls: the panel's grey, not the scene's
  // near-black showing through the hole the part leaves in the backdrop.
  const sx = px0 + (q[0][0] + 24 * 1.5) * k, sy = py0 + (q[0][1] + 21 * 1.5) * k;
  const lit = pixel(Buffer.from((await send('Page.captureScreenshot', { format: 'png', clip: { x: sx, y: sy, width: 1, height: 1, scale: 1 } })).data, 'base64'));
  check(lit.every(c => c > 0x18), `the captured part is drawn where its quad is (${lit})`);
  await drag(mx, my, mx + 90 * k * 1.5, my, true);
  const osc = read().scenes[0].layers[0].parts.osc;
  check(typeof osc.x === 'number' && Math.abs(osc.x - 90) < 3, `dragging a part moves it in plugin pixels (${JSON.stringify(osc)})`);
  // The layer's own inspector (not the part's) has the explode button.
  await js(`[...document.querySelectorAll('#layers button:not(.part)')].pop().click()`);
  await sleep(250);
  await click('[data-explode]');
  const burst = read().scenes[0].layers[0].explode;
  check(Array.isArray(burst) && burst.some(e => Math.abs(e.t - 2) < 0.02 && e.v === 0.5), `explode is keyed at the playhead (${JSON.stringify(burst)})`);
  await sleep(600);
  await shot('editor-plugin.png');
  // Two levels: `serve` recaptures, the controls nest under their panels,
  // and the part tree reads the same.
  await js(`(i => { i.value = 2; i.dispatchEvent(new Event('change')); })(document.querySelector('[data-levels]'))`);
  await sleep(600);
  check(read().scenes[0].layers[0].explode_levels === 2, 'explode levels are saved');
  const nested = 'button.part[data-part="filter/filter-cutoff"]';
  for (let i = 0; i < 150 && !(await js(`!!document.querySelector('${nested}')`)); i++) await sleep(200);
  check(await js(`document.querySelector('${nested}')?.dataset.depth`) === '1', 'a control nests one level under its panel');
  const tree = await js(`cutParts('synth')`);
  const filter = tree?.parts.find(p => p.id === 'filter');
  check(filter?.children.map(c => c.id).join() === 'filter/filter-cutoff,filter/filter-res,filter/filter-drive' && filter.children[0].thumb?.startsWith('.cut-cache/img/'),
    `the part tree has the controls with thumbnails (${JSON.stringify(filter?.children.map(c => c.id))})`);
  await click(nested);
  check(await js(`document.querySelector('#insp-title').textContent`) === 'Part · filter/filter-cutoff', 'selecting a control inspects it');
  await sleep(400);
  await shot('editor-plugin-levels.png');
  // Interact: a drag on the cutoff knob keys the plugin's pointer at the
  // playhead, in the UI's pixels, instead of moving the part.
  await click('#interact');
  const kq = await js(`cutQuads().find(q => q.id === 'synth#filter/filter-cutoff').pts`);
  const kx = px0 + (kq[0][0] + kq[2][0]) / 2 * k, ky = py0 + (kq[0][1] + kq[2][1]) / 2 * k;
  await drag(kx, ky, kx, ky - 40);
  await click('#interact');
  const syn = read().scenes[0].layers[0];
  const at2 = keys => keys.find(e => Math.abs(e.t - 2) < 0.02);
  const f = filter.children[0].frame, ux = at2(syn.pointer_x)?.v, uy = at2(syn.pointer_y)?.v;
  check(at2(syn.pointer_down)?.v === 1 && ux > f[0] && ux < f[0] + f[2] && uy > f[1] && uy < f[1] + f[3],
    `interact presses the pointer on the knob at the playhead (${ux}, ${uy} in ${f})`);
  check(syn.pointer_down.some(e => e.t > 2 && e.t < 2.2 && e.v === 0) && syn.pointer_y.some(e => e.t > 2 && e.t < 2.2 && e.v < uy - 10),
    `the drag and release are keyed just after (${JSON.stringify(syn.pointer_y.slice(0, 5))})`);
  check(syn.parts['filter/filter-cutoff']?.x === undefined, 'interact does not move the part');
  // Sources: a file dropped on the panel is imported, the plugin is added
  // through its dialog and opened as a folder of its parts, a part dragged
  // onto the layers becomes a component layer at its spot, which is
  // parented, moved and reset.
  const dt = 'globalThis.e2eDrag ??= new DataTransfer()';
  // At `fy` of the element's height (a layer row's middle parents, its
  // top or bottom quarter reorders).
  const fire = (sel, types, fy = 0.5) => js(`(() => { const dt = ${dt}; const el = document.querySelector(${JSON.stringify(sel)});
    const r = el.getBoundingClientRect(), at = { clientX: r.left + r.width / 2, clientY: r.top + r.height * ${fy} };
    for (const t of ${JSON.stringify(types)}) el.dispatchEvent(new DragEvent(t, { dataTransfer: dt, bubbles: true, cancelable: true, ...at })); })()`);
  const svg = '<svg xmlns="http://www.w3.org/2000/svg" width="40" height="40"><rect width="40" height="40" fill="#fff"/></svg>';
  await js(`(() => { globalThis.e2eDrag = new DataTransfer(); e2eDrag.items.add(new File([${JSON.stringify(svg)}], 'badge.svg', { type: 'image/svg+xml' })); })()`);
  await fire('#sources', ['dragover', 'drop']);
  await sleep(1200);
  check(existsSync(join(out, 'media/badge.svg')) && read().sources?.some(s => s.kind === 'svg' && s.path === 'media/badge.svg'), 'a dropped file is imported as a source');
  check(await js(`!!document.querySelector('#sources [data-source="badge.svg"] img.thumb')`), 'with its thumbnail');
  // A font is a source too; dragged in, it makes a text layer set in it.
  const ttf = readFileSync(join(here, '../../../crates/mui-text/fonts/MaterialSymbolsOutlined-subset.ttf')).toString('base64');
  await js(`(() => { globalThis.e2eDrag = new DataTransfer(); e2eDrag.items.add(new File([Uint8Array.from(atob('${ttf}'), c => c.charCodeAt(0))], 'icons.ttf')); })()`);
  await fire('#sources', ['dragover', 'drop']);
  await sleep(1200);
  check(read().sources?.some(s => s.id === 'icons.ttf' && s.kind === 'font' && s.path === 'media/icons.ttf'), 'a dropped font is imported as a source');
  await js(`globalThis.e2eDrag = new DataTransfer()`);
  await fire('#sources [data-source="icons.ttf"]', ['dragstart']);
  await fire('#layers', ['dragover', 'drop']);
  await sleep(800);
  const typed2 = read().scenes[0].layers.find(l => l.font === 'icons.ttf');
  check(typed2?.kind === 'text', `a dragged font makes a text layer in it (${JSON.stringify(typed2)}; ${await js(`document.querySelector('#status').textContent + ' | ' + cutRev() + ' | ' + JSON.stringify(cutSources().map(m => m.id)) + ' | ' + document.querySelector('#layers').textContent.slice(0, 200)`)})`);
  check(await js(`document.querySelector('[data-font]')?.value`) === 'icons.ttf', 'the inspector picks the font');
  await js(`document.querySelector('#layer-del').click()`); await sleep(400);
  await click('#add-plugin');
  await js(`(() => { document.querySelector('#pl-bin').value = ${JSON.stringify(synth)}; document.querySelector('#pl-id').value = 'synth'; document.querySelector('#pl-add').click(); })()`);
  await sleep(800);
  check(read().sources?.some(s => s.id === 'synth' && s.kind === 'plugin' && s.source.bin === synth), 'the plugin dialog adds a plugin source');
  // Top-level parts (panels); a selected control has unfolded its panel.
  const srcTree = () => js(`[...document.querySelectorAll('#sources [data-source="synth"][data-part]')].map(r => r.dataset.part).filter(p => !p.includes('/'))`);
  let nodes = [];
  for (let i = 0; i < 150 && nodes.length < 5; i++) { await sleep(200); nodes = await srcTree(); }
  check(nodes.length === 5 && nodes.includes('osc'), `the plugin opens as a folder of its parts (${nodes})`);
  check(await js(`[...document.querySelectorAll('#sources [data-source="synth"][data-part]')].every(r => r.querySelector('img.thumb'))`), 'each part has its captured thumbnail');
  const twist = '#sources [data-source="synth"]:not([data-part]) .twist';
  await js(`document.querySelector('${twist}').click()`); await sleep(200);
  check((await srcTree()).length === 0, 'its folder collapses');
  await js(`document.querySelector('${twist}').click()`); await sleep(200);
  check((await srcTree()).length === 5 && await js(`document.querySelector('${twist}').getAttribute('aria-expanded')`) === 'true', 'and expands again');
  // A panel opens onto its controls, from the same tree as `cutParts`.
  await js(`(b => b.getAttribute('aria-expanded') === 'true' || b.click())(document.querySelector('#sources [data-source="synth"][data-part="filter"] .twist'))`); await sleep(200);
  const ctl = '#sources [data-source="synth"][data-part="filter/filter-cutoff"]';
  check(await js(`!!document.querySelector('${ctl} img.thumb') && parseFloat(document.querySelector('${ctl}').style.paddingLeft) > parseFloat(document.querySelector('#sources [data-part="filter"]').style.paddingLeft)`),
    'a panel unfolds onto its controls, indented, with thumbnails');
  check(await js(`document.querySelector('${ctl} .label').textContent`) === 'filter-cutoff', 'named by the part');
  await shot('editor-sources.png');
  // Zero config: a plugin crate's folder (a git-MUI crate, built against
  // this MUI by the generic adapter) opens as its tree of parts.
  const fixture = join(here, '../tests/fixtures/plain');
  await click('#add-plugin');
  await js(`(() => { document.querySelector('#pl-from').value = ${JSON.stringify(fixture)}; document.querySelector('#pl-id').value = 'knobs'; document.querySelector('#pl-add').click(); })()`);
  let added = null;
  for (let i = 0; i < 50 && !added; i++) { await sleep(200); added = read().sources?.find(s => s.id === 'knobs'); }
  check(added?.kind === 'plugin' && added.source.plugin?.endsWith('tests/fixtures/plain'), `Add plugin by folder writes a plugin source (${JSON.stringify(added)})`);
  const knobs = () => js(`[...document.querySelectorAll('#sources [data-source="knobs"][data-part]')].map(r => r.dataset.part)`);
  let kparts = [];
  // The first run builds the adapter.
  for (let i = 0; i < 1500 && !kparts.includes('tone-color'); i++) { await sleep(200); kparts = await knobs(); }
  check(kparts.includes('tone-color') && kparts.includes('space-mix'), `and its part tree appears (${kparts})`);
  // Drag the `osc` part onto the layer list.
  await js(`globalThis.e2eDrag = new DataTransfer()`);
  await fire('#sources [data-source="synth"][data-part="osc"]', ['dragstart']);
  await fire('#layers', ['dragover', 'drop']);
  await sleep(1500);
  const comp = read().scenes[0].layers.find(l => l.show);
  check(comp?.show?.[0] === 'osc' && comp.source.bin === synth && comp.name === 'osc', `a dragged part becomes a component layer (${JSON.stringify(comp)})`);
  const home = await js(`cutSources().find(s => s.id === 'synth').state`);
  const man = await (await fetch(`http://127.0.0.1:${port}/asset/${home}`)).json();
  const [rx, ry] = man.layers.find(f => f.group === 'osc').rect;
  let cq = null;
  for (let i = 0; i < 50 && !cq; i++) { await sleep(100); cq = await js(`cutQuads().find(q => q.id === ${JSON.stringify(comp.id)})?.pts`); }
  const want = [comp.x + comp.scale * (rx - man.width / 2), comp.y + comp.scale * (ry - man.height / 2)];
  check(cq && Math.hypot(cq[0][0] - want[0], cq[0][1] - want[1]) < 0.5, `it shows just that part, at its spot in the plugin (${cq?.[0]} vs ${want})`);
  check(await js(`document.querySelector('#sources [data-source="synth"][data-part="osc"]').classList.contains('on')`), 'the tree highlights the selected component');
  // Parent it to the caption by dragging its row onto the caption's.
  await js(`globalThis.e2eDrag = new DataTransfer()`);
  await fire(`#layers button[data-layer="${comp.id}"]`, ['dragstart']);
  await fire('#layers button[data-layer="caption"]', ['dragover', 'drop']);
  await sleep(1200);
  const kid = read().scenes[0].layers.find(l => l.id === comp.id);
  check(kid.parent === 'caption' && Math.abs(kid.y - (comp.y - 1040)) < 1e-6, `dropping a layer on another parents it (${JSON.stringify({ parent: kid.parent, x: kid.x, y: kid.y })})`);
  const cq2 = await js(`cutQuads().find(q => q.id === ${JSON.stringify(comp.id)})?.pts`);
  check(Math.hypot(cq2[0][0] - cq[0][0], cq2[0][1] - cq[0][1]) < 0.5, `parenting keeps it where it was (${cq2[0]})`);
  const pads = await js(`['caption', ${JSON.stringify(comp.id)}].map(id => parseFloat(document.querySelector('#layers button[data-layer="' + id + '"]').style.paddingLeft))`);
  check(pads[1] > pads[0], 'the layer list nests it under its parent');
  // Move it in the viewport (its part), then reset it.
  const oq = await js(`cutQuads().find(q => q.id === ${JSON.stringify(comp.id + '#osc')}).pts`);
  const [ax, ay] = await rect('#overlay');
  const [aw] = await js(`(r => [r.width])(document.querySelector('#overlay').getBoundingClientRect())`);
  const kk = aw / 1920, cx0 = ax + (oq[0][0] + oq[2][0]) / 2 * kk, cy0 = ay + (oq[0][1] + oq[2][1]) / 2 * kk;
  await drag(cx0, cy0, cx0 + 60, cy0, true);
  const movedPart = read().scenes[0].layers.find(l => l.id === comp.id).parts?.osc;
  check(movedPart && Math.abs(movedPart.x - 60 / kk / comp.scale) < 3, `dragging it moves it (${JSON.stringify(movedPart)})`);
  check(await js(`document.querySelector('#insp-title').textContent`) === 'Part · osc', 'the viewport selects the part');
  await click('[data-reset]');
  const back = read().scenes[0].layers.find(l => l.id === comp.id).parts?.osc ?? {};
  check(!('x' in back) && !('y' in back), `reset puts the part back (${JSON.stringify(back)})`);
  await js(`document.querySelector('#layers button[data-layer="${comp.id}"]').click()`); await sleep(300);
  await js(`(() => { const i = document.querySelector('[data-prop="rotation"]'); i.value = '25'; i.onchange(); })()`); await sleep(600);
  check(read().scenes[0].layers.find(l => l.id === comp.id).rotation === 25, 'the component layer turns');
  await click('[data-reset]');
  const reset = read().scenes[0].layers.find(l => l.id === comp.id);
  check(reset.rotation === undefined && (reset.x ?? 0) === 0 && (reset.y ?? 0) === 0 && reset.parent === 'caption', `reset clears the layer's transform, onto its parent (${JSON.stringify(reset)})`);
  await key('z', 'KeyZ', 2);
  check(read().scenes[0].layers.find(l => l.id === comp.id).rotation === 25, 'and undo brings it back');
  // The tree selects: a part of the whole plugin, on that layer.
  await click('#sources [data-source="synth"][data-part="env"]');
  check(await js(`document.querySelector('#insp-title').textContent`) === 'Part · env', 'clicking a part in the tree selects it');
  await shot('editor-sources-parented.png');

  // Selection, align, the gizmo, snapping and the panels, on rects (and a
  // child of a parent turned a quarter).
  {
    const shapes = { size: [1280, 720], fps: 30, scenes: [{ name: 'sel', duration: 2, background: '#101014', layers: [
      { id: 'a', kind: 'rect', x: 200, y: 200, width: 100, height: 80, fill: '#e03020' },
      { id: 'b', kind: 'rect', x: 520, y: 280, width: 160, height: 60, fill: '#20d040' },
      { id: 'c', kind: 'rect', x: 900, y: 420, width: 60, height: 120, fill: '#2040e0' },
      { id: 'p', kind: 'rect', x: 640, y: 600, width: 40, height: 40, rotation: 90, fill: '#808080' },
      { id: 'k', kind: 'rect', parent: 'p', x: 0, y: -100, width: 30, height: 30, fill: '#ffffff' }] }] };
    writeFileSync(file, JSON.stringify(shapes)); await sleep(1500);
    const lay = id => read().scenes[0].layers.find(l => l.id === id);
    const [ex, ey] = await rect('#overlay');
    const [ew] = await js(`(r => [r.width])(document.querySelector('#overlay').getBoundingClientRect())`);
    const sk = ew / 1280, at2d = (x, y) => [ex + x * sk, ey + y * sk];
    const quad = id => js(`(q => q && [Math.min(...q.pts.map(p => p[0])), Math.min(...q.pts.map(p => p[1])), Math.max(...q.pts.map(p => p[0])), Math.max(...q.pts.map(p => p[1]))])(cutQuads().find(q => q.id === '${id}'))`);
    const tap = async ([x, y], modifiers = 0) => { await mouse('mousePressed', x, y, modifiers); await mouse('mouseReleased', x, y, modifiers); await sleep(300); };
    // Modifiers held once moving: at the press, Ctrl or Shift would pick.
    const pull = async ([x0, y0], [x1, y1], modifiers = 0) => {
      await mouse('mousePressed', x0, y0);
      for (let i = 1; i <= 8; i++) { await mouse('mouseMoved', x0 + (x1 - x0) * i / 8, y0 + (y1 - y0) * i / 8, modifiers); await sleep(20); }
      await mouse('mouseReleased', x1, y1, modifiers); await sleep(700);
    };
    const picked = () => js(`[...document.querySelectorAll('#layers button.on')].map(b => b.dataset.layer).sort().join()`);
    await click('#layers [data-layer="a"]');
    await tap(at2d(520, 280), 8);
    check(await picked() === 'a,b' && await js(`document.querySelector('#insp-title').textContent`) === '2 layers', `Shift-click in the viewport adds to the selection (${await picked()})`);
    await pull(at2d(5, 5), at2d(1000, 500));
    check(await picked() === 'a,b,c', `a marquee selects what it touches (${await picked()})`);
    check(await js(`document.querySelector('[data-prop="x"]').placeholder`) === 'mixed', 'the inspector shows a value they differ in as mixed');
    await js(`(i => { i.value = '0.5'; i.onchange(); })(document.querySelector('[data-prop="opacity"]'))`); await sleep(800);
    check(['a', 'b', 'c'].every(id => lay(id).opacity === 0.5), 'an inspector edit sets every selected layer');
    await click('[data-align="left"]'); await sleep(400);
    check(lay('a').x === 200 && lay('b').x === 230 && lay('c').x === 180, `align left lines up their left edges (${['a', 'b', 'c'].map(id => lay(id).x)})`);
    await key('V', 'KeyV', 1 | 8);
    check(lay('b').y === 300 && lay('a').y === 200 && lay('c').y === 420, `Alt+Shift+V evens the gaps down (${['a', 'b', 'c'].map(id => lay(id).y)})`);
    await shot('editor-align.png');
    // Snapping: b's middle 3 px off the canvas's catches it; with Ctrl, not.
    await click('#layers [data-layer="b"]');
    await pull(at2d(230, 300), at2d(230, 363));
    check(lay('b').y === 360, `a move snaps onto the canvas's middle (${lay('b').y})`);
    await key('z', 'KeyZ', 2);
    check(lay('b').y === 300, 'undo takes the move back in one step');
    await mouse('mousePressed', ...at2d(230, 300));
    for (let i = 1; i <= 8; i++) { await mouse('mouseMoved', ...at2d(230, 300 + 63 * i / 8)); await sleep(20); }
    check(await js(`import('/snap.js').then(m => m.guides.y.join())`) === '360', 'a guide marks the line it caught on');
    await shot('editor-snap.png');
    await mouse('mouseReleased', ...at2d(230, 363)); await sleep(600);
    await key('z', 'KeyZ', 2);
    await pull(at2d(230, 300), at2d(230, 363), 2);
    check(Math.abs(lay('b').y - 363) < 1.5 && lay('b').y !== 360, `holding Ctrl drags freely (${lay('b').y})`);
    await key('z', 'KeyZ', 2);
    // Ctrl+D copies the selection in place; Delete takes it away.
    await key('d', 'KeyD', 2);
    check(lay('b2')?.y === 300 && await picked() === 'b2', 'Ctrl+D duplicates the selection and selects the copy');
    await key('Delete', 'Delete');
    check(!lay('b2') && lay('b'), 'Delete removes the selected layer');
    // The gizmo: a's bottom right handle stretches it from its top left
    // corner; Shift on the knob turns it a quarter exactly.
    await click('#layers [data-layer="a"]');
    const [qa] = [await quad('a')];
    await pull(at2d(qa[2], qa[3]), [at2d(qa[2], qa[3])[0] + 50, at2d(qa[2], qa[3])[1] + 30], 2);
    const a1 = lay('a'), dw = 50 / sk, dh = 30 / sk;
    check(Math.abs(a1.width - (100 + dw)) < 1.5 && Math.abs(a1.height - (80 + dh)) < 1.5, `a corner handle scales it (${a1.width} x ${a1.height})`);
    check(Math.abs(a1.x - a1.width / 2 - 150) < 0.01 && Math.abs(a1.y - a1.height / 2 - 160) < 0.01, `about the opposite corner (${a1.x}, ${a1.y})`);
    const qa2 = await quad('a'), [mx2, my2] = [(qa2[0] + qa2[2]) / 2, (qa2[1] + qa2[3]) / 2];
    await pull([at2d(mx2, qa2[1])[0], at2d(mx2, qa2[1])[1] - 24], at2d(mx2 + 200, my2), 8);
    check(lay('a').rotation === 90, `the knob turns it, Shift in 15° steps (${lay('a').rotation})`);
    await shot('editor-gizmo.png');
    // A child of a turned parent moves where it is dragged.
    await click('#layers [data-layer="k"]');
    const qk = await quad('k'), [kx, ky] = [(qk[0] + qk[2]) / 2, (qk[1] + qk[3]) / 2];
    await pull(at2d(kx, ky), [at2d(kx, ky)[0] + 60, at2d(kx, ky)[1]], 2);
    const qk2 = await quad('k'), D = 60 / sk;
    check(Math.abs(qk2[0] - qk[0] - D) < 1 && Math.abs(qk2[1] - qk[1]) < 1 && Math.abs(lay('k').y - (-100 - D)) < 0.5,
      `a child of a turned parent moves with the pointer (${lay('k').x}, ${lay('k').y})`);
    // Panels: a dragged gutter's size is kept over a reload; a double-click
    // puts it back.
    const [gx0, gy0] = await rect('[data-gutter="left"]');
    await pull([gx0 + 3, gy0 + 200], [gx0 + 83, gy0 + 200]);
    const wide = await js(`document.querySelector('#left').getBoundingClientRect().width`);
    check(Math.abs(wide - 320) < 2, `dragging a gutter resizes the panel (${wide})`);
    await send('Page.reload');
    for (let i = 0; i < 100 && (await js(`document.querySelector('#status')?.textContent`)) !== 'loaded'; i++) await sleep(100);
    check(await js(`document.querySelector('#left').getBoundingClientRect().width`) === wide, 'the panel size is kept over a reload');
    await shot('editor-panels.png');
    await js(`document.querySelector('[data-gutter="left"]').dispatchEvent(new MouseEvent('dblclick', { bubbles: true }))`); await sleep(300);
    check(await js(`document.querySelector('#left').getBoundingClientRect().width`) === 240 && await js(`localStorage.getItem('mui-cut.panels')`) === '{}', 'double-clicking the gutter resets it');
  }

  // The layer tree, comps, the timeline's bars, zoom, markers and keys.
  {
    const doc = { size: [1280, 720], fps: 30, scenes: [{ name: 'main', duration: 4, background: '#101014', layers: [
      { id: 'a', kind: 'rect', x: 200, y: 200, width: 100, height: 80, fill: '#e03020', rotation: [{ t: 0.5, v: 0 }, { t: 2.5, v: 90 }] },
      { id: 'b', kind: 'rect', x: [{ t: 1, v: 500 }, { t: 2, v: 700 }], y: 300, width: 160, height: 60, fill: '#20d040' },
      { id: 'c', kind: 'rect', x: 900, y: 420, width: 60, height: 120, fill: '#2040e0' }] }] };
    writeFileSync(file, JSON.stringify(doc)); await sleep(1500);
    const lay = (id, si = 0) => read().scenes[si].layers.find(l => l.id === id);
    const row = id => `#layers button[data-layer="${id}"]`;
    const pick = async (...ids) => { await js(`document.querySelector('${row(ids[0])}').click()`); for (const id of ids.slice(1)) await js(`document.querySelector('${row(id)}').dispatchEvent(new MouseEvent('click', { bubbles: true, shiftKey: true }))`); await sleep(300); };
    const quadOf = id => js(`cutQuads().find(q => q.id === '${id}')?.pts`);
    // Group: Ctrl+G puts a and b in a group where they are.
    const qa = await quadOf('a');
    await pick('a', 'b'); await key('g', 'KeyG', 2);
    const g = read().scenes[0].layers.find(l => l.kind === 'group');
    check(g && lay('a').parent === g.id && lay('b').parent === g.id, `Ctrl+G groups the selection (${g?.id})`);
    await sleep(300);
    const qa2 = await quadOf('a');
    check(Math.hypot(qa2[0][0] - qa[0][0], qa2[0][1] - qa[0][1]) < 0.5, 'grouping keeps the layers where they are');
    const pad = id => js(`parseFloat(document.querySelector('${row(id)}').style.paddingLeft)`);
    check(await pad('a') > await pad(g.id), 'the layer tree nests them under the group');
    // Fold: the twist hides the children, and stays folded over a reload.
    await js(`document.querySelector('${row(g.id)} [data-twist]').click()`); await sleep(300);
    check(!await js(`!!document.querySelector('${row('a')}')`), 'folding the group hides its children');
    await send('Page.reload');
    for (let i = 0; i < 100 && (await js(`document.querySelector('#status')?.textContent`)) !== 'loaded'; i++) await sleep(100);
    check(!await js(`!!document.querySelector('${row('a')}')`) && /main\/group/.test(await js(`JSON.stringify(Object.entries(localStorage))`)), 'the fold is kept over a reload');
    await js(`document.querySelector('${row(g.id)} [data-twist]').click()`); await sleep(300);
    check(await js(`!!document.querySelector('${row('a')}')`), 'unfolding shows them again');
    // Reorder: c dropped on a's top quarter goes above a, inside the group.
    await js(`globalThis.e2eDrag = new DataTransfer()`);
    await fire(row('c'), ['dragstart']);
    await fire(row('a'), ['dragover', 'drop'], 0.1);
    await sleep(1000);
    const ids = () => read().scenes[0].layers.map(l => l.id);
    check(lay('c').parent === g.id && ids().indexOf('c') === ids().indexOf('a') + 1, `dropping on a row's top edge puts it above, among its siblings (${ids()})`);
    // Ungroup hands the children back, where they are.
    await pick(g.id); await key('G', 'KeyG', 2 | 8);
    check(!lay(g.id) && !lay('a').parent && !lay('c').parent, 'Ctrl+Shift+G ungroups');
    const qa3 = await quadOf('a');
    check(Math.hypot(qa3[0][0] - qa[0][0], qa3[0][1] - qa[0][1]) < 0.5, 'and they stay where they are');
    // Precompose c: a new scene with c in it, a comp of it in its place.
    await pick('c'); await key('C', 'KeyC', 2 | 8);
    const pre = read(), inner = pre.scenes.find(s => s.name === 'c comp'), comp = pre.scenes[0].layers.find(l => l.kind === 'comp');
    check(inner?.layers.some(l => l.id === 'c') && comp?.scene === 'c comp' && !lay('c'), `Ctrl+Shift+C precomposes (${comp?.id} of ${comp?.scene})`);
    check(await js(`[...document.querySelectorAll('#scenes button')].some(b => b.title.startsWith('Comped'))`), 'the scene list marks the comped scene');
    await key('z', 'KeyZ', 2);
    check(lay('c') && read().scenes.length === 1, 'undo takes the precompose back in one step');

    // The timeline: b's bar slides with its keys, its start trims.
    const T = async (t, id, p) => js(`(r => { const m = document.querySelector('#timeline')._map; return [r.left + m.x(${t}), r.top + m.y(${JSON.stringify(id)}, ${JSON.stringify(p ?? null)})]; })(document.querySelector('#timeline').getBoundingClientRect())`);
    await pick('b'); await sleep(200);
    const [bx0, by] = await T(0.5, 'b'), [bx1] = await T(1, 'b');
    await drag(bx0, by, bx1, by, true);
    check(lay('b').start === 0.5 && lay('b').end === undefined && lay('b').x.map(k => k.t).join() === '1.5,2.5', `sliding a bar moves its in point and keys (${JSON.stringify({ start: lay('b').start, keys: lay('b').x.map(k => k.t) })})`);
    await key('z', 'KeyZ', 2);
    check(lay('b').start === undefined && lay('b').x[0].t === 1, 'one undo takes the slide back');
    await key('y', 'KeyY', 2);
    const [sx0] = await T(0.5, 'b'), [sx1] = await T(0.8, 'b');
    await drag(sx0, by, sx1, by, true);
    check(Math.abs(lay('b').start - 0.8) < 1e-6 && lay('b').x[0].t === 1.5, `dragging a bar's edge trims it, keys stay (${lay('b').start})`);
    await js(`import('/transport.js').then(m => m.seek(1))`);
    await key('[', 'BracketLeft', 1);
    check(lay('b').start === 1, `Alt+[ trims the in point to the playhead (${lay('b').start})`);
    // Zoom: Ctrl+wheel keeps the time under the pointer.
    const [zx, zy] = await T(2, 'b');
    const at = () => js(`(m => m.x(2))(document.querySelector('#timeline')._map)`), span = () => js(`import('/state.js').then(m => m.S.view.t1 - m.S.view.t0)`);
    const [x2, w0] = [await at(), await span()];
    await send('Input.dispatchMouseEvent', { type: 'mouseWheel', x: zx, y: zy, deltaX: 0, deltaY: -240, modifiers: 2 }); await sleep(300);
    check(await span() < w0 * 0.9 && Math.abs(await at() - x2) < 0.5, `Ctrl+wheel zooms about the pointer (${(await span()).toFixed(3)} s in view)`);
    await key('f', 'KeyF');
    // Markers: M at the playhead, dragged to 1.5 s.
    await key('m', 'KeyM');
    check(JSON.stringify(read().scenes[0].markers) === '[{"t":1}]', `M adds a marker at the playhead (${JSON.stringify(read().scenes[0].markers)})`);
    const [mx0] = await T(1, 'b'), [mx1] = await T(1.5, 'b'), rulerY = (await rect('#timeline'))[1] + 12;
    await drag(mx0, rulerY, mx1, rulerY, true);
    check(read().scenes[0].markers?.[0]?.t === 1.5, `dragging a marker moves it (${JSON.stringify(read().scenes[0].markers)})`);
    // Keys: a box over a's and b's key rows picks the keys from 1.2 s to
    // 2.8 s; dragging one moves them all.
    await pick('a', 'b'); await sleep(300);
    const [k0, ky0] = await T(1.2, 'a', 'rotation'), [k1, ky1] = await T(2.8, 'b', 'x');
    await drag(k0, ky0, k1, ky1);
    const nk = await js(`import('/state.js').then(m => m.S.selKeys.map(s => s.l.id + ':' + s.k.t).sort().join())`);
    check(nk === 'a:2.5,b:1.5,b:2.5', `a box picks the keys inside it (${nk})`);
    const [dx0, dy] = await T(1.5, 'b', 'x'), [dx1] = await T(2, 'b', 'x');
    await drag(dx0, dy, dx1, dy, true);
    check(lay('b').x.map(k => k.t).join() === '2,3' && lay('a').rotation.map(k => k.t).join() === '0.5,3', `dragging a picked key moves every picked key (${lay('b').x.map(k => k.t)} / ${lay('a').rotation.map(k => k.t)})`);
    await shot('editor-timeline.png');
  }
  // Procedural: the instancing duplicator's source, the effector gizmo,
  // group animator presets and behaviours.
  {
    writeFileSync(file, readFileSync(join(here, '../examples/procedural.cut.json'), 'utf8')); await sleep(1500);
    const lay = id => read().scenes[0].layers.find(l => l.id === id);
    const row = id => `#layers button[data-layer="${id}"]`;
    const pick = async id => { await js(`document.querySelector('${row(id)}').click()`); await sleep(400); };
    const [ex, ey] = await rect('#overlay');
    const [ew] = await js(`(r => [r.width])(document.querySelector('#overlay').getBoundingClientRect())`);
    const at2d = ([x, y]) => [ex + x * ew / 1280, ey + y * ew / 1280];
    const pull = async ([x0, y0], [x1, y1]) => {
      await mouse('mousePressed', x0, y0);
      for (let i = 1; i <= 8; i++) { await mouse('mouseMoved', x0 + (x1 - x0) * i / 8, y0 + (y1 - y0) * i / 8); await sleep(20); }
      await mouse('mouseReleased', x1, y1); await sleep(700);
    };
    await js(`import('/transport.js').then(m => m.seek(2))`); await sleep(400);
    await pick('array');
    check(await js(`document.querySelector('[data-source]')?.value`) === 'tile', 'the duplicator shows its source');
    const ef = await js('cutEffectors()');
    check(ef.length === 1 && ef[0].i === 0, `the effector is in the viewport (${JSON.stringify(ef)})`);
    await shot('editor-procedural.png');
    // The radius knob: dragged 60 px out, the radius grows by 60.
    const r0 = lay('array').animators[0].falloff.radius;
    const [kx, ky] = ef[0].knob;
    await pull(at2d([kx, ky]), at2d([kx + 60, ky]));
    const r1 = lay('array').animators[0].falloff.radius;
    check(Math.abs(r1 - r0 - 60) < 3, `dragging the radius knob sets the radius (${r0} to ${r1})`);
    // The centre: dragged 100 px down, keyed at the playhead.
    const [cx, cy] = (await js('cutEffectors()'))[0].c;
    await pull(at2d([cx, cy]), at2d([cx, cy + 100]));
    const fy = lay('array').animators[0].falloff.y, k2 = fy.find(k => Math.abs(k.t - 2) < 1e-6);
    check(k2 && Math.abs((await js('cutEffectors()'))[0].c[1] - (cy + 100)) < 3, `dragging the centre keys it at the playhead (${JSON.stringify(fy)})`);
    await key('z', 'KeyZ', 2); await key('z', 'KeyZ', 2);
    check(lay('array').animators[0].falloff.radius === r0, 'undo takes the drags back');
    // An effector switched on for the second animator; its order offers distance.
    await js(`(c => { c.checked = true; c.dispatchEvent(new Event('change')); })(document.querySelector('[data-effector="1"]'))`); await sleep(600);
    check(JSON.stringify(lay('array').animators[1].falloff) === '{}' && (await js('cutEffectors()')).length === 2, 'an animator gets an effector');
    check(await js(`[...document.querySelectorAll('#inspector [data-prop^="animators.1.falloff."]')].length`) === 4, 'its numbers show as keyable rows');
    check(await js(`[...document.querySelectorAll('#inspector [data-prop^="animators.0.jitter_"]')].length`) === 6, 'the jitter rows show');
    // Back to a shape: the shape field returns.
    await js(`(s => { s.value = ''; s.dispatchEvent(new Event('change')); })(document.querySelector('[data-source]'))`); await sleep(600);
    check(lay('array').source === undefined && await js(`[...document.querySelectorAll('#inspector label')].some(l => l.textContent === 'shape')`), 'clearing the source makes copies of a shape again');
    await key('z', 'KeyZ', 2);
    check(lay('array').source === 'tile', 'and undo brings the source back');
    // Picking an instance: a click on a copy picks the duplicator and
    // that copy; the inspector shows it and leads to the source.
    await sleep(600);
    const iq = await js(`cutQuads().find(q => q.id === 'array@37')?.pts`);
    check(!!iq, 'each instanced copy has a quad');
    const ic = at2d([(iq[0][0] + iq[2][0]) / 2, (iq[0][1] + iq[2][1]) / 2]);
    await mouse('mousePressed', ...ic); await mouse('mouseReleased', ...ic); await sleep(600);
    const inst = await js('cutInst()');
    check(inst?.id === 'array' && Number.isInteger(inst.c), `a click on a copy picks that instance (${JSON.stringify(inst)})`);
    check((await js(`document.querySelector('[data-instance]')?.value`))?.startsWith(`copy ${inst.c} of 75`), 'the inspector shows the picked copy');
    await shot('editor-instance.png');
    await js(`document.querySelector('[data-edit-source]').click()`); await sleep(400);
    check(await js(`import('/state.js').then(m => m.S.sel)`) === 'tile' && await js('cutInst()') === null, 'Edit source selects the source');
    // Text: an effector and distance order on glyphs; a spring's rows.
    await pick('title');
    check(await js(`!!document.querySelector('[data-effector="0"]')`) && (await js('cutEffectors()')).length === 1, 'a text animator has an effector');
    check(await js(`!!document.querySelector('#inspector [data-prop="behaviours.0.damping"]') && !document.querySelector('#inspector [data-prop="behaviours.0.amount"]')`), 'a spring shows freq and damping');
    // A group: Cascade children, and a wiggle.
    await pick('words');
    const opts = await js(`[...document.querySelector('[data-adder="+ animator"]').options].map(o => o.textContent).join()`);
    check(opts.includes('Cascade children') && opts.includes('Ripple from point'), `a group offers its presets (${opts})`);
    await js(`(s => { s.value = 'cascade_children'; s.dispatchEvent(new Event('change')); })(document.querySelector('[data-adder="+ animator"]'))`); await sleep(600);
    check(lay('words').animators.length === 2 && lay('words').animators[1].stagger === 0.12, 'Cascade children lands on the group');
    await js(`(s => { s.value = 'wiggle'; s.dispatchEvent(new Event('change')); })(document.querySelector('[data-adder="+ behaviour"]'))`); await sleep(600);
    const bs = lay('words').behaviours;
    check(bs.length === 4 && bs[2].prop === 'x' && bs[3].prop === 'y', `Wiggle adds behaviours (${JSON.stringify(bs)})`);
    await js(`(s => { s.value = 'scale'; s.dispatchEvent(new Event('change')); })(document.querySelector('[data-behaviour-prop="2"]'))`); await sleep(600);
    check(lay('words').behaviours[2].prop === 'scale', 'a behaviour\'s property changes');
    check(await js(`!!document.querySelector('#inspector [data-prop="behaviours.3.amount"]')`), 'its amount is a keyable row');
    await js(`document.querySelector('#right').scrollTop = 1e6`); await sleep(200);
    await shot('editor-behaviours.png');
  }
  check(errors.length === 0, 'no page exceptions ' + errors.join('; '));
  ws.close();
} catch (e) {
  console.error(e.message); failed = true;
} finally {
  chrome.kill(); server.kill();
}
process.exit(failed ? 1 : 0);
