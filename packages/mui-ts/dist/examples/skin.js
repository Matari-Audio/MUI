export const skin = {
    corners: { convex: 28, concave: 32 },
    spacing: { xs: 4, s: 8, m: 12, l: 18, xl: 28 },
    // Only what differs from `Palette::NEUTRAL`. Everything else -- hover,
    // pressed, dimmed ink, every surface layer, and the whole light theme -- is
    // derived, so there is nothing here a mode could contradict.
    palette: { primary: [242, 0.131], step: 0.045, hover: 0.11 },
    strokeWidth: 1.5,
};
