// The viewport renderer, off the main thread, the first that opens of:
// WebGPU (Vello, engine per `WEBGPU_ENGINE`), WebGL2 (vello_gpu: no
// compute shaders there, so classic Vello cannot run), then Vello CPU into
// the canvas's 2D context. `?renderer=classic|gpu|webgl2|cpu` in the
// editor's URL forces one. Draws are requested one at a time; the editor
// sends the next only after `drawn`, so a slow frame never queues up.
//
// Exports run here too: every frame of every scene drawn at project size
// (on WebGPU with the motion-blur shutter, else on the CPU), encoded by a
// WebCodecs VideoEncoder and muxed to MP4 by mp4.js.
import init, { Cut, GpuView, lastPanic } from './pkg/mui_cut.js';
import { Mp4 } from './mp4.js';

// The WebGPU default, from the benchmark in the README.
const WEBGPU_ENGINE = 'classic';

let gpu = null, cpu = null, ctx = null, canvas = null;
let gpuKind = null;     // [api, engine] the viewport opened with; exports use the same
let json = null, variant = '';
const assets = new Map();   // every asset file, for export renderers
let exporting = null;   // { cancelled }
// The export renderer, kept across exports of one size: a new WebGPU
// device per export stalled the second one.
let exportGpu = null;   // { view, out, w, h }
const ready = init();

// A canvas holds one kind of context for life, so each API is probed on a
// scratch canvas (or adapter) first: a failed GpuView must not leave a
// context behind that stops the next fallback from getting its own.
const hasWebGpu = async () => !!(self.navigator.gpu && await self.navigator.gpu.requestAdapter().catch(() => null));
const hasWebGl2 = () => { try { return !!new OffscreenCanvas(1, 1).getContext('webgl2'); } catch { return false; } };

async function open(c, renderer) {
  canvas = c;
  const tries = {
    classic: [['webgpu', 'classic']], gpu: [['webgpu', 'gpu']], webgl2: [['webgl2', 'gpu']], cpu: [],
  }[renderer] ?? [['webgpu', WEBGPU_ENGINE], ['webgl2', 'gpu']];
  for (const [api, engine] of tries) {
    if (!(api === 'webgpu' ? await hasWebGpu() : hasWebGl2())) continue;
    try { gpu = await GpuView.create(canvas, api, engine); gpuKind = [api, engine]; return api === 'webgpu' ? 'WebGPU' : 'WebGL2'; }
    catch (e) { console.warn(api, e); break; }
  }
  cpu = new Cut(); ctx = canvas.getContext('2d');
  return 'CPU';
}

const tick = () => new Promise(r => setTimeout(r, 0));

// m: { w, h, fps, mb, codec, bitrate, scenes: [{ duration }] }
async function exportVideo(m) {
  const job = exporting = { cancelled: false };
  let view = null, out = null, cut = null;
  if (gpu) {
    if (exportGpu?.w !== m.w || exportGpu?.h !== m.h) {
      exportGpu?.view.free();
      const c = new OffscreenCanvas(m.w, m.h);
      exportGpu = { view: await GpuView.create(c, ...gpuKind), out: c, w: m.w, h: m.h };
    }
    ({ view, out } = exportGpu);
    view.load(json, variant || undefined);
    for (const [path, bytes] of assets) view.add_asset(path, bytes);
  } else {
    cut = new Cut(); cut.load(json, variant || undefined);
    for (const [path, bytes] of assets) cut.add_asset(path, bytes);
  }
  const mux = new Mp4({ codec: m.codec, width: m.w, height: m.h, fps: m.fps });
  let failed = null;
  const encoder = new VideoEncoder({ output: (c, meta) => mux.add(c, meta), error: e => { failed = e; } });
  const config = { codec: m.codec, width: m.w, height: m.h, bitrate: m.bitrate, framerate: m.fps, hardwareAcceleration: 'prefer-hardware' };
  if (m.codec.startsWith('avc1')) config.avc = { format: 'avc' };
  if (m.codec.startsWith('hvc1')) config.hevc = { format: 'hevc' };
  if (!(await VideoEncoder.isConfigSupported(config)).supported) config.hardwareAcceleration = 'no-preference';
  encoder.configure(config);
  const total = m.scenes.reduce((a, s) => a + Math.max(1, Math.round(s.duration * m.fps)), 0);
  const dur = Math.round(1e6 / m.fps);
  let n = 0;
  try {
    for (const [si, s] of m.scenes.entries()) {
      // An export never quietly flattens a 3D shot (the GPU view refuses
      // in draw_frame when its 3D pass fails).
      const flat = cut?.notice(si);
      if (flat) throw new Error(flat);
      const count = Math.max(1, Math.round(s.duration * m.fps));
      for (let i = 0; i < count; i++) {
        if (job.cancelled) throw new Error('cancelled');
        if (failed) throw failed;
        const t = i / m.fps, init = { timestamp: Math.round(n * 1e6 / m.fps), duration: dur };
        let frame;
        if (view) {
          view.draw_frame(si, t, m.mb);
          frame = new VideoFrame(out, init);
        } else {
          const px = cut.render(si, t, m.w, m.h);
          frame = new VideoFrame(px, { ...init, format: 'RGBA', codedWidth: m.w, codedHeight: m.h });
        }
        encoder.encode(frame, { keyFrame: n % Math.round(2 * m.fps) === 0 });
        frame.close();
        n++;
        // Let the encoder drain and a cancel arrive.
        while (encoder.encodeQueueSize > 4) await tick();
        if (n % 5 === 0 || n === total) { self.postMessage({ type: 'progress', done: n, total }); await tick(); }
      }
    }
    await encoder.flush();
    if (failed) throw failed;
    const bytes = mux.finish();
    self.postMessage({ type: 'exported', bytes, frames: n, codec: m.codec, hardware: config.hardwareAcceleration }, [bytes.buffer]);
  } catch (e) {
    self.postMessage({ type: 'export-error', error: String(e?.message ?? e) });
  } finally {
    if (encoder.state !== 'closed') encoder.close();
    if (exporting === job) exporting = null;
  }
}

self.onmessage = async ({ data: m }) => {
  await ready;
  try {
    if (m.type === 'init') {
      const backend = await open(m.canvas, m.renderer);
      self.postMessage({ type: 'ready', backend, adapter: gpu ? gpu.adapter() : '', engine: gpu ? gpu.engine() : 'vello_cpu' });
    } else if (m.type === 'load') {
      ({ json, variant } = m);
      (gpu ?? cpu).load(json, variant || undefined);
    } else if (m.type === 'orbit') {
      if (gpu) { if (m.on) gpu.set_orbit(m.yaw, m.pitch, m.zoom); else gpu.clear_orbit(); }
    } else if (m.type === 'asset') {
      assets.set(m.path, m.bytes);
      (gpu ?? cpu).add_asset(m.path, m.bytes);
    } else if (m.type === 'export') {
      exportVideo(m);
    } else if (m.type === 'cancel') {
      if (exporting) exporting.cancelled = true;
    } else if (m.type === 'draw') {
      const start = performance.now();
      let quads;
      if (gpu) quads = gpu.draw(m.si, m.t, m.w, m.h);
      else {
        if (canvas.width !== m.w || canvas.height !== m.h) { canvas.width = m.w; canvas.height = m.h; }
        const px = cpu.render(m.si, m.t, m.w, m.h);
        ctx.putImageData(new ImageData(new Uint8ClampedArray(px.buffer, px.byteOffset, px.length), m.w, m.h), 0, 0);
        quads = cpu.quads();
      }
      const notice = gpu ? gpu.notice() : cpu.notice(m.si);
      self.postMessage({ type: 'drawn', quads, notice, ms: performance.now() - start });
    }
  } catch (e) {
    const why = lastPanic();
    self.postMessage({ type: m.type === 'draw' ? 'drawn' : 'error', error: why ? `${e}: ${why}` : String(e) });
  }
};
