import {
  column, corners, defineScene, frameSurface, insetSurface, leaf,
  mergeSurface, px,
} from "../src/index.js";

export default defineScene({
  theme: {
    corners: { convex: 28, concave: 32 },
    spacing: { xs: 4, s: 8, m: 12, l: 18, xl: 28 },
    // Only what differs from the default: everything else -- hover, pressed,
    // dimmed ink, every surface layer -- is derived from these.
    palette: { primary: [242, 0.131], step: 0.045, hover: 0.11 },
    strokeWidth: 1.5,
  },

  root: column([
    column([
      column([
        column([
          leaf([28, 28], { id: "plus" }),
          leaf([28, 28], { id: "pie-a" }),
          leaf([28, 28], { id: "pie-b" }),
        ], { gap: 10, align: "center" }),
      ], { padding: 10, align: "center" }),
    ], { id: "tab", padding: 12, min: [92, 0], align: "center" }),
    leaf([520, 230], { id: "panel" }),
  ], { id: "root", gap: 0, align: "start", justify: "start" }),

  surfaces: [
    frameSurface("panel"),
    frameSurface("tab"),
    mergeSurface("outer", ["panel", "tab"], corners.global),
    // Exact parallel shell: this derives bounds AND radius from the tab.
    insetSurface("pill-shell", "tab", px(12)),
  ],
});
