// The viewport renderer, off the main thread, the first that opens of:
// WebGPU (Vello, engine per `WEBGPU_ENGINE`), WebGL2 (vello_gpu: no
// compute shaders there, so classic Vello cannot run), then Vello CPU into
// the canvas's 2D context. `?renderer=classic|gpu|webgl2|cpu` in the
// editor's URL forces one. Draws are requested one at a time; the editor
// sends the next only after `drawn`, so a slow frame never queues up.
import init, { Cut, GpuView } from './pkg/mui_cut.js';

// The WebGPU default, from the benchmark in the README.
const WEBGPU_ENGINE = 'classic';

let gpu = null, cpu = null, ctx = null, canvas = null;
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
    try { gpu = await GpuView.create(canvas, api, engine); return api === 'webgpu' ? 'WebGPU' : 'WebGL2'; }
    catch (e) { console.warn(api, e); break; }
  }
  cpu = new Cut(); ctx = canvas.getContext('2d');
  return 'CPU';
}

self.onmessage = async ({ data: m }) => {
  await ready;
  try {
    if (m.type === 'init') {
      const backend = await open(m.canvas, m.renderer);
      self.postMessage({ type: 'ready', backend, adapter: gpu ? gpu.adapter() : '', engine: gpu ? gpu.engine() : 'vello_cpu' });
    } else if (m.type === 'load') {
      (gpu ?? cpu).load(m.json);
    } else if (m.type === 'orbit') {
      if (gpu) { if (m.on) gpu.set_orbit(m.yaw, m.pitch, m.zoom); else gpu.clear_orbit(); }
    } else if (m.type === 'asset') {
      (gpu ?? cpu).add_asset(m.path, m.bytes);
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
    self.postMessage({ type: m.type === 'draw' ? 'drawn' : 'error', error: String(e) });
  }
};
