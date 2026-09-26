// mui-track.js -- read a mui-reel track.json at any time t. No dependencies.
// Classic script (sets globalThis.MuiTrack) and CommonJS (module.exports), so
// it loads from a HyperFrames <script> and from a Remotion/webpack import.
//
//   const track = MuiTrack.load(json);    // also makes MuiTrack.rect(...) etc. use it
//   track.rect("gain", t)   -> {x, y, w, h} in video px after the camera, or null
//   track.pointer(t)        -> {x, y, down} or null
//   track.camera(t)         -> {x, y, zoom}
//   track.beat(n)           -> seconds of beat n at the reel's bpm
(function (root) {
  "use strict";
  function load(json) {
    var d = typeof json === "string" ? JSON.parse(json) : json;
    var last = Math.max(0, d.frames - 1);
    // Frame pair around t and the blend between them; clamped to the take.
    function at(t) {
      var f = Math.min(last, Math.max(0, t * d.fps));
      var i = Math.floor(f);
      return [i, Math.min(i + 1, last), f - i];
    }
    function mix(a, b, u) {
      if (!a) return null;
      if (!b) return a;
      return a.map(function (v, k) {
        return typeof v === "number" ? v + (b[k] - v) * u : v;
      });
    }
    var track = {
      data: d,
      fps: d.fps,
      duration: d.duration,
      rect: function (id, t) {
        var s = d.surfaces[id];
        if (!s) return null;
        var p = at(t), r = mix(s[p[0]], s[p[1]], p[2]);
        return r && { x: r[0], y: r[1], w: r[2], h: r[3] };
      },
      pointer: function (t) {
        var p = at(t), r = mix(d.pointer[p[0]], d.pointer[p[1]], p[2]);
        return r && { x: r[0], y: r[1], down: r[2] };
      },
      camera: function (t) {
        var p = at(t), r = mix(d.camera[p[0]], d.camera[p[1]], p[2]);
        return { x: r[0], y: r[1], zoom: r[2] };
      },
      beat: function (n) {
        return (n * 60) / d.bpm;
      },
      events: function (kind) {
        return kind ? d.events.filter(function (e) { return e.kind === kind; }) : d.events;
      },
    };
    for (var k in track) MuiTrack[k] = track[k];
    return track;
  }
  var MuiTrack = { load: load };
  root.MuiTrack = MuiTrack;
  if (typeof module === "object" && module.exports) module.exports = MuiTrack;
})(typeof globalThis !== "undefined" ? globalThis : this);
