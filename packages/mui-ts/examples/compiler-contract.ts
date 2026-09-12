// A compiled runtime fixture, not only a string snapshot of generated code.
import { defineScene, flow, leaf, frameSurface, extendTo, mergeSurface, space } from "../src/index.js";
export const unusualKey = 'tab\u0001\b\f\\"שלום🎹';
export default defineScene({
  root: flow([leaf(unusualKey, [80, 32]), leaf("panel", [320, 100])], {
    axis: "column", gap: space.m, padding: space.s, width: "fill", height: "hug",
  }),
  availableWidth: 336,
  theme: {
    mode: "light", colors: { primary: [0xaa22cc, 0x44ccaa, 0xdd5522], neutral: 0x888888 },
    darkColors: { primary: [0x8855ff, 0x44bbaa, 0xff8844], neutral: 0x777777 },
    corners: { convex: 1e21, concave: 1e-7 }, spacing: { xs: 4, s: 8, m: 12, l: 18, xl: 28 }, strokeWidth: -0,
  },
  surfaces: [extendTo(frameSurface("tab", unusualKey), "bottom", "panel"), frameSurface("panel", "panel"), mergeSurface("shell", ["tab", "panel"])],
});
