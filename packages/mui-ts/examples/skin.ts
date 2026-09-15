// Everything an application decides about how it looks, and nothing else.
//
// One file, one object, imported by every scene. The Rust side of this is a
// `const SKIN: Theme` in one module; the shape is the same on purpose, because
// which of the two you author in should be a build decision, not a design one.
import type { Theme } from "../src/index.js";

export const skin: Theme = {
  corners: { convex: 28, concave: 32 },
  spacing: { xs: 4, s: 8, m: 12, l: 18, xl: 28 },
  // Only what differs from `Palette::NEUTRAL`. Everything else -- hover,
  // pressed, dimmed ink, every surface layer, and the whole light theme -- is
  // derived, so there is nothing here a mode could contradict.
  palette: { primary: [242, 0.131], step: 0.045, hover: 0.11 },
  strokeWidth: 1.5,
};
