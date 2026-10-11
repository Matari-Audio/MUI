// Looks: named starting points in the effect picker, an effect with its
// parameters set. Edit any of them after; they are ordinary effects.
export const LOOKS = [
  { name: 'Soft bloom', fx: { type: 'glow', mode: 'bloom', threshold: 0.55, knee: 0.5, radius: 90, intensity: 0.8, falloff: 0.6 } },
  { name: 'Neon', fx: { type: 'glow', mode: 'neon', radius: 36, intensity: 1.4 } },
  { name: 'Outer glow', fx: { type: 'glow', mode: 'outer', radius: 20, intensity: 1 } },
  { name: 'Frosted glass', fx: { type: 'glass', frost: 18, refraction: 10, bevel: 18, tint_amount: 0.12, saturation: 1.3 } },
  { name: 'Liquid glass', fx: { type: 'glass', frost: 0, refraction: 30, bevel: 40, dispersion: 0.35, tint_amount: 0.03, saturation: 1.5, highlight: 0.9, shadow: 0.15, grain: 0 } },
];
