const $ = id => document.getElementById(id);
const source = $('source'), canvas = $('canvas'), status = $('status'), error = $('error');
const context = canvas.getContext('2d');
let worker, timer, debounce, revision = 0, selection = 0, baseline = '', ready = false;
const storageKey = 'mui-playground-scene-v1';
function showError(message) { error.textContent = message; error.hidden = false; status.textContent = 'Scene needs a fix'; $('loading').hidden = true; }
function startWorker() {
  worker?.terminate();
  worker = new Worker('./worker.js', {type: 'module'});
  worker.onmessage = ({data}) => {
    if (data.id !== revision) return;
    clearTimeout(timer);
    $('run').disabled = false;
    $('loading').hidden = true;
    if (data.error) { showError(data.error); return; }
    error.hidden = true;
    context.putImageData(new ImageData(new Uint8ClampedArray(data.pixels), 640, 480), 0, 0);
    status.textContent = `${data.surfaces} surfaces · ${data.ops} draws · ${data.ms.toFixed(1)} ms`;
    ready = true;
  };
  worker.onerror = () => { clearTimeout(timer); worker.terminate(); worker = null; $('run').disabled = false; showError('The WASM engine could not start. Reload the page or try Run again.'); };
}
function run() {
  clearTimeout(debounce); clearTimeout(timer);
  const id = ++revision;
  if (source.value.length > 16384) { worker?.terminate(); worker = null; $('run').disabled = false; showError('Keep the scene below 16 KiB.'); return; }
  if (!worker) startWorker();
  // Replacing a busy worker bounds pathological geometry and stale edit queues.
  if ($('run').disabled) startWorker();
  $('run').disabled = true;
  status.textContent = 'Rendering…';
  try { localStorage.setItem(storageKey, source.value); } catch { /* storage may be disabled */ }
  worker.postMessage({id, source: source.value});
  timer = setTimeout(() => { worker.terminate(); worker = null; $('run').disabled = false; showError('This scene exceeded the time limit. Reduce its geometry and run again.'); }, ready ? 6000 : 20000);
}
async function example(name) {
  const request = ++selection;
  const response = await fetch(`./examples/${name}.mui`);
  if (!response.ok) throw new Error('Could not load the example.');
  const text = await response.text();
  if (request !== selection) return;
  baseline = text; source.value = baseline;
  document.querySelectorAll('[data-example]').forEach(button => button.setAttribute('aria-pressed', String(button.dataset.example === name)));
  run();
}
source.addEventListener('input', () => { clearTimeout(debounce); debounce = setTimeout(run, 350); });
source.addEventListener('keydown', event => {
  if ((event.ctrlKey || event.metaKey) && event.key === 'Enter') { event.preventDefault(); run(); }
});
$('run').onclick = run;
$('reset').onclick = () => { source.value = baseline; run(); };
document.querySelectorAll('[data-example]').forEach(button => button.onclick = () => example(button.dataset.example).catch(e => showError(e.message)));
$('export').onclick = () => {
  if (!ready) return;
  canvas.toBlob(blob => { if (!blob) return; const link = document.createElement('a'); link.href = URL.createObjectURL(blob); link.download = 'mui-scene.png'; link.click(); setTimeout(() => URL.revokeObjectURL(link.href), 1000); });
};
$('share').onclick = async () => {
  const url = new URL(location.href); url.hash = new URLSearchParams({scene: source.value});
  try { await navigator.clipboard.writeText(url.href); $('share').textContent = 'Scene link copied ✓'; }
  catch { showError('Clipboard unavailable. Your scene is still saved locally.'); }
};
try {
  const response = await fetch('./examples/weld.mui');
  if (!response.ok) throw new Error('Could not load the starter scene.');
  baseline = await response.text();
  let saved; try { saved = localStorage.getItem(storageKey); } catch { /* optional */ }
  const shared = new URLSearchParams(location.hash.slice(1)).get('scene');
  source.value = shared ?? saved ?? baseline;
  if (shared || saved) document.querySelector('[data-example="weld"]').setAttribute('aria-pressed', 'false');
  run();
} catch (e) { showError(e.message); }
