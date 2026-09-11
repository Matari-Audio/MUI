import {
  column, corners, defineScene, frameSurface, insetSurface, leaf,
  mergeSurface, px,
} from "../src/index.js";

export default defineScene({
  theme: {
    corners: { convex: 28, concave: 32 },
    spacing: { xs: 4, s: 8, m: 12, l: 18, xl: 28 },
    strokeWidth: 1.5,
  },

  root: column("root", [
    column("tab-frame", [
      column("pill-frame", [
        column("controls", [
          leaf("plus", [28, 28]),
          leaf("pie-a", [28, 28]),
          leaf("pie-b", [28, 28]),
        ], { gap: 10, align: "center" }),
      ], { padding: 10, align: "center" }),
    ], { padding: 12, min: [92, 0], align: "center" }),
    leaf("panel-frame", [520, 230]),
  ], { gap: 0, align: "start", justify: "start" }),

  surfaces: [
    frameSurface("panel", "panel-frame"),
    frameSurface("tab", "tab-frame"),
    mergeSurface("outer", ["panel", "tab"], corners.global),
    // Exact parallel shell: this derives bounds AND radius from the tab.
    insetSurface("pill-shell", "tab", px(12)),
  ],
});
