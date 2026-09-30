// The viewport renderer, off the main thread: MUI's Vello GPU renderer on
// WebGPU into the transferred canvas, or Vello CPU into its 2D context when
// the browser has no WebGPU. Draws are requested one at a time; the editor
// sends the next only after `drawn`, so a slow frame never queues up.
import init, { Cut, GpuView } from './pkg/mui_cut.js';

let gpu = null, cpu = null, ctx = null, canvas = null;
const ready = init();

async function open(c) {
  canvas = c;
  // Ask for an adapter first: a failed GpuView must not leave a WebGPU
  // context on the canvas, or the 2D fallback cannot get one.
  if (self.navigator.gpu && await self.navigator.gpu.requestAdapter().catch(() => null)) {
    try { gpu = await GpuView.create(canvas); return 'WebGPU'; } catch (e) { console.warn('WebGPU:', e); }
  }
  cpu = new Cut(); ctx = canvas.getContext('2d');
  return 'CPU';
}

self.onmessage = async ({ data: m }) => {
  await ready;
  try {
    if (m.type === 'init') {
      const backend = await open(m.canvas);
      self.postMessage({ type: 'ready', backend, adapter: gpu ? gpu.adapter() : '' });
    } else if (m.type === 'load') {
      (gpu ?? cpu).load(m.json);
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
      self.postMessage({ type: 'drawn', quads, ms: performance.now() - start });
    }
  } catch (e) {
    self.postMessage({ type: m.type === 'draw' ? 'drawn' : 'error', error: String(e) });
  }
};
