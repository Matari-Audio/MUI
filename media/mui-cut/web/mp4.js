// A minimal MP4 writer for one WebCodecs video track and an optional audio
// track: ftyp, moov (before the data, so the file streams), one mdat with a
// chunk per track. Enough for H.264 (avcC), H.265 (hvcC) and AV1 (av1C)
// from a VideoEncoder, and AAC (esds) or Opus (dOps) from an AudioEncoder;
// no fragments.
//
//   const mux = new Mp4({ codec: 'avc1.640028', width, height, fps });
//   mux.sound({ codec: 'mp4a.40.2', rate: 48000, channels: 2 });   // optional
//   encoder output: (chunk, meta) => mux.add(chunk, meta)
//   audio encoder output: (chunk, meta) => mux.addAudio(chunk, meta)
//   const bytes = mux.finish();   // Uint8Array

const TIMESCALE = 1_000_000;   // microseconds, the unit WebCodecs uses

const u8 = n => [n & 255];
const u16 = n => [(n >> 8) & 255, n & 255];
const u32 = n => [(n >>> 24) & 255, (n >>> 16) & 255, (n >>> 8) & 255, n & 255];
const str = s => [...s].map(c => c.charCodeAt(0));
const flat = parts => parts.flat(Infinity);
function box(type, ...body) {
  const b = flat(body);
  return [...u32(8 + b.length), ...str(type), ...b];
}
const full = (type, version, flags, ...body) => box(type, u8(version), u8(flags >> 16), u16(flags & 0xffff), ...body);
const MATRIX = [0x10000, 0, 0, 0, 0x10000, 0, 0, 0, 0x40000000].flatMap(u32);

export class Mp4 {
  constructor({ codec, width, height, fps }) {
    Object.assign(this, { codec, width, height, fps });
    this.samples = [];   // { data, key, ts, dur }
    this.config = null;  // the codec configuration box's payload
  }

  sound({ codec, rate, channels }) {
    this.audio = { codec, rate, channels, samples: [], config: null };
  }

  addAudio(chunk, meta) {
    const a = this.audio, data = new Uint8Array(chunk.byteLength);
    chunk.copyTo(data);
    const d = meta?.decoderConfig?.description;
    if (!a.config && d) a.config = [...new Uint8Array(ArrayBuffer.isView(d) ? d.buffer.slice(d.byteOffset, d.byteOffset + d.byteLength) : d)];
    // Opus runs on a 48 kHz clock in MP4, whatever the input rate.
    const scale = a.codec === 'opus' ? 48000 : a.rate;
    a.samples.push({ data, dur: Math.round((chunk.duration ?? 0) * scale / TIMESCALE) });
  }

  // The audio sample entry: mp4a with esds (AAC), or Opus with dOps.
  audioEntry() {
    const a = this.audio, opus = a.codec === 'opus';
    let config;
    if (opus) {
      // From the encoder's OpusHead (little-endian) when it gives one.
      const h = a.config && String.fromCharCode(...a.config.slice(0, 8)) === 'OpusHead' ? a.config : null;
      const le = (o, n) => h ? h.slice(o, o + n).reduceRight((v, b) => v * 256 + b, 0) : 0;
      config = box('dOps', u8(0), u8(a.channels), u16(le(10, 2)), u32(h ? le(12, 4) : a.rate), u16(le(16, 2)), u8(0));
    } else {
      const asc = a.config ?? [];
      const len = n => u8(n);
      const dsi = [u8(5), len(asc.length), asc];
      const dcd = [u8(4), len(13 + 2 + asc.length), u8(0x40), u8(0x15), u8(0), u16(0), u32(0), u32(0), dsi];
      const es = [u8(3), len(3 + 2 + 13 + 2 + asc.length + 3), u16(0), u8(0), dcd, u8(6), u8(1), u8(2)];
      config = full('esds', 0, 0, es);
    }
    return box(opus ? 'Opus' : 'mp4a', Array(6).fill(0), u16(1), Array(8).fill(0),
      u16(a.channels), u16(16), u16(0), u16(0), u32((opus ? 48000 : a.rate) * 65536), config);
  }

  add(chunk, meta) {
    const data = new Uint8Array(chunk.byteLength);
    chunk.copyTo(data);
    if (!this.config && meta?.decoderConfig?.description) {
      const d = meta.decoderConfig.description;
      this.config = [...new Uint8Array(ArrayBuffer.isView(d) ? d.buffer.slice(d.byteOffset, d.byteOffset + d.byteLength) : d)];
    }
    this.samples.push({ data, key: chunk.type === 'key', ts: chunk.timestamp, dur: chunk.duration ?? Math.round(TIMESCALE / this.fps) });
  }

  // The sample entry: avc1 / hvc1 / av01 with its configuration box.
  entry() {
    const kind = this.codec.slice(0, 4);
    let config;
    if (kind === 'avc1') config = box('avcC', this.config);
    else if (kind === 'hvc1' || kind === 'hev1') config = box('hvcC', this.config);
    else if (kind === 'av01' && this.config?.[0] === 0x81) config = box('av1C', this.config);
    else if (kind === 'av01') {
      // av1C from the codec string, av01.P.LLT.DD: the sequence header rides
      // in the first key frame, so no config OBUs are needed.
      const [, p, lt, dd] = this.codec.split('.');
      const level = parseInt(lt, 10), tier = lt.endsWith('H') ? 1 : 0, deep = +dd > 8 ? 1 : 0;
      config = box('av1C', u8(0x81), u8((+p << 5) | level), u8((tier << 7) | (deep << 6) | 0b1100), u8(0));
    } else throw new Error('mp4: unsupported codec ' + this.codec);
    // BT.709 limited range, as mui-cut renders.
    const colr = box('colr', str('nclx'), u16(1), u16(1), u16(1), u8(0));
    return box(kind === 'hev1' ? 'hev1' : kind,
      Array(6).fill(0), u16(1), Array(16).fill(0), u16(this.width), u16(this.height),
      u32(0x480000), u32(0x480000), u32(0), u16(1), Array(32).fill(0), u16(0x18), u16(0xffff),
      config, colr);
  }

  // A track: its header, media header and handler, and its sample table
  // (every sample in one chunk at `offset`).
  trak({ id, scale, samples, entry, video, offset }) {
    const dur = samples.reduce((a, x) => a + x.dur, 0);
    const stts = [];
    for (const x of samples) {
      const last = stts[stts.length - 1];
      if (last && last[1] === x.dur) last[0]++; else stts.push([1, x.dur]);
    }
    let extra = [];
    if (video) {
      // Decode order is output order; any reordering shows as ctts offsets.
      let dts = 0;
      const cts = samples.map(x => { const o = x.ts - samples[0].ts - dts; dts += x.dur; return o; });
      const keys = samples.flatMap((x, i) => x.key ? [i + 1] : []);
      extra = [
        cts.some(c => c) ? full('ctts', 1, 0, u32(samples.length), cts.map(c => [u32(1), u32(c)])) : [],
        keys.length < samples.length ? full('stss', 0, 0, u32(keys.length), keys.map(u32)) : [],
      ];
    }
    const stbl = box('stbl',
      full('stsd', 0, 0, u32(1), entry),
      full('stts', 0, 0, u32(stts.length), stts.map(([n, d]) => [u32(n), u32(d)])),
      extra,
      full('stsc', 0, 0, u32(1), u32(1), u32(samples.length), u32(1)),
      full('stsz', 0, 0, u32(0), u32(samples.length), samples.map(x => u32(x.data.length))),
      full('stco', 0, 0, u32(1), u32(offset)));
    const ms = Math.round(dur * 1000 / scale);   // tkhd in the movie's milliseconds
    return [ms, box('trak',
      full('tkhd', 0, 3, u32(0), u32(0), u32(id), u32(0), u32(ms), Array(8).fill(0), u16(0), u16(0), u16(video ? 0 : 0x100), u16(0), MATRIX,
        u32(video ? this.width << 16 : 0), u32(video ? this.height << 16 : 0)),
      box('mdia',
        full('mdhd', 0, 0, u32(0), u32(0), u32(scale), u32(dur), u16(0x55c4), u16(0)),
        full('hdlr', 0, 0, u32(0), str(video ? 'vide' : 'soun'), Array(12).fill(0), str('mui-cut'), u8(0)),
        box('minf',
          video ? full('vmhd', 0, 1, Array(8).fill(0)) : full('smhd', 0, 0, u16(0), u16(0)),
          box('dinf', full('dref', 0, 0, u32(1), full('url ', 0, 1))),
          stbl)))];
  }

  moov(offset) {
    const [vms, video] = this.trak({ id: 1, scale: TIMESCALE, samples: this.samples, entry: this.entry(), video: true, offset });
    const tracks = [video];
    let ms = vms;
    if (this.audio?.samples.length) {
      const a = this.audio, at = offset + this.samples.reduce((n, x) => n + x.data.length, 0);
      const [ams, trak] = this.trak({ id: 2, scale: a.codec === 'opus' ? 48000 : a.rate, samples: a.samples, entry: this.audioEntry(), video: false, offset: at });
      tracks.push(trak);
      ms = Math.max(ms, ams);
    }
    return box('moov',
      full('mvhd', 0, 0, u32(0), u32(0), u32(1000), u32(ms), u32(0x10000), u16(0x100), Array(10).fill(0), MATRIX, Array(24).fill(0), u32(tracks.length + 1)),
      tracks);
  }

  finish() {
    if (!this.samples.length) throw new Error('mp4: no frames');
    const brand = { avc1: 'avc1', hvc1: 'hvc1', hev1: 'hvc1', av01: 'av01' }[this.codec.slice(0, 4)];
    const ftyp = box('ftyp', str('isom'), u32(0x200), str('isom'), str('iso2'), str(brand), str('mp41'));
    const all = [...this.samples, ...(this.audio?.samples ?? [])];
    const size = all.reduce((a, x) => a + x.data.length, 0);
    // moov's size does not depend on the offset it holds.
    const moov = this.moov(ftyp.length + this.moov(0).length + 8);
    const out = new Uint8Array(ftyp.length + moov.length + 8 + size);
    out.set(ftyp, 0); out.set(moov, ftyp.length);
    let at = ftyp.length + moov.length;
    out.set([...u32(8 + size), ...str('mdat')], at); at += 8;
    for (const x of all) { out.set(x.data, at); at += x.data.length; }
    return out;
  }
}
