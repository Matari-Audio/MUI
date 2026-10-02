import { $, S, worker } from './state.js';
import { comped } from './tree.js';

// ---------- export: WebCodecs in the worker, MP4 by mp4.js
// Codec strings by what the frame needs: level 4.x up to 1080p, 5.x above.
function codecs() {
  const big = S.R.size[0] * S.R.size[1] > 1920 * 1088;
  return [
    ['H.264', big ? 'avc1.640033' : 'avc1.640028'],
    ['H.265', big ? 'hvc1.1.6.L153.B0' : 'hvc1.1.6.L123.B0'],
    ['AV1', big ? 'av01.0.12M.08' : 'av01.0.08M.08'],
  ];
}
let exportRun = null;   // { start } while an export runs
async function openExport() {
  const [w, h] = S.R.size, dlg = $('#export-dialog');
  const sel = $('#ex-codec'); sel.replaceChildren();
  for (const [name, codec] of codecs()) {
    const ok = (await VideoEncoder.isConfigSupported({ codec, width: w, height: h, bitrate: 8e6, framerate: S.R.fps }).catch(() => ({}))).supported;
    const o = new Option(name + (ok ? '' : ' (not in this browser)'), codec); o.disabled = !ok;
    sel.append(o);
  }
  sel.value = [...sel.options].find(o => !o.disabled)?.value ?? '';
  // About 0.1 bit a pixel a frame, at least 2 Mbit/s.
  $('#ex-bitrate').value = Math.max(2, Math.round(w * h * S.R.fps * 0.1 / 1e5) / 10);
  const gpu = $('#backend').dataset.backend === 'WebGPU';
  $('#ex-mb').value = gpu ? (S.doc.render?.mb ?? 1) : 1;
  $('#ex-mb').disabled = !gpu;
  $('#ex-note').textContent = `${w}×${h} at ${S.R.fps} fps, every scene but the comped ones` + (gpu ? '' : ' · CPU: no effects or motion blur');
  $('#ex-status').textContent = ''; $('#ex-progress').hidden = true;
  $('#ex-start').disabled = !sel.value;
  if (!sel.value) $('#ex-status').textContent = 'This browser encodes none of these codecs.';
  dlg.showModal();
}
$('#export').onclick = () => { if (!exportRun) openExport(); else $('#export-dialog').showModal(); };
if (!('VideoEncoder' in self)) { $('#export').disabled = true; $('#export').title = 'This browser has no WebCodecs'; }
$('#ex-start').onclick = () => {
  const codec = $('#ex-codec').value;
  exportRun = { start: performance.now(), codec };
  $('#ex-start').disabled = true; $('#ex-cancel').textContent = 'Cancel';
  $('#ex-progress').hidden = false; $('#ex-progress').value = 0;
  $('#ex-status').textContent = 'rendering';
  worker.postMessage({
    type: 'export', codec, w: S.R.size[0], h: S.R.size[1], fps: S.R.fps,
    mb: Math.max(1, Math.min(64, Math.round(+$('#ex-mb').value || 1))),
    bitrate: Math.round((+$('#ex-bitrate').value || 8) * 1e6),
    // A scene another scene comps is a part, not a shot: left out, as
    // `render` and `/mix.wav` leave it out.
    scenes: S.R.scenes.map((s, si) => ({ si, duration: s.duration })).filter(s => !comped().has(S.doc.scenes[s.si].name)),
  });
};
$('#ex-cancel').onclick = () => { if (exportRun) worker.postMessage({ type: 'cancel' }); else $('#export-dialog').close(); };
export function exportMessage(m) {
  if (m.type === 'progress') {
    $('#ex-progress').value = m.done / m.total;
    $('#ex-status').textContent = `frame ${m.done} of ${m.total}`;
    return;
  }
  const secs = ((performance.now() - exportRun.start) / 1000).toFixed(1);
  exportRun = null;
  $('#ex-start').disabled = false; $('#ex-cancel').textContent = 'Close';
  if (m.type === 'export-error') { $('#ex-status').textContent = m.error === 'cancelled' ? 'cancelled' : 'failed: ' + m.error; return; }
  const blob = new Blob([m.bytes], { type: 'video/mp4' });
  window.lastExport = { blob, frames: m.frames, codec: m.codec, sound: m.sound, hardware: m.hardware };   // e2e reads it
  const a = document.createElement('a');
  a.href = URL.createObjectURL(blob);
  a.download = ($('#file').textContent || 'cut').replace(/\.cut\.json$|\.json$/, '') + '.mp4';
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 60_000);
  const sound = { 'mp4a.40.2': ', AAC sound', opus: ', Opus sound' }[m.sound] ?? '';
  $('#ex-status').textContent = `${m.frames} frames${sound}, ${(blob.size / 1e6).toFixed(1)} MB in ${secs} s`;
}

