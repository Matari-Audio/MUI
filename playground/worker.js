import init, { render } from './pkg/mui_playground.js';
const ready = init();
self.onmessage = async ({data: {id, source}}) => {
  try {
    await ready;
    const start = performance.now();
    const frame = render(source, 640, 480);
    const surfaces = frame.surfaces, ops = frame.paint_ops;
    const pixels = frame.pixels();
    self.postMessage({id, pixels, surfaces, ops, ms: performance.now() - start}, [pixels.buffer]);
  } catch (error) {
    self.postMessage({id, error: String(error)});
  }
};
