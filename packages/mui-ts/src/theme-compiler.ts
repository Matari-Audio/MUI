import type { ColorSeeds } from "./index.js";
import { n } from "./literals.js";
const rgb = (v:number) => `mui_core::Rgb::new(${(v >>> 16) & 255}, ${(v >>> 8) & 255}, ${v & 255})`;
const seeds = (v:ColorSeeds, fallback:string) => `mui_core::Seeds { primary: [${v.primary.map(rgb).join(", ")}], neutral: ${rgb(v.neutral)}, ${v.status ? `status: [${v.status.map(rgb).join(", ")}],` : `..${fallback}`} }`;
export function compileTheme(t: import("./index.js").Theme = {}): string {
  const fields: string[] = [];
  if (t.mode) fields.push(`mode: mui_core::Mode::${t.mode === "light" ? "Light" : "Dark"}`);
  if (t.colors || t.darkColors) {
    const light = t.colors ? seeds(t.colors, "mui_core::Seeds::default()") : "mui_core::Seeds::default()";
    fields.push(`palette: mui_core::Palette { light: ${light}, dark: ${t.darkColors ? `Some(${seeds(t.darkColors, light)})` : "None"} }`);
  }
  if (t.corners) fields.push(`corners: mui_core::CornerProfile::new(${n(t.corners.convex)}, ${n(t.corners.concave)})`);
  const spacingFields = (["xs","s","m","l","xl"] as const).filter(key => t.spacing?.[key] !== undefined).map(key => `${key}: ${n(t.spacing![key]!)}`);
  if (spacingFields.length) fields.push(`spacing: mui_core::SpacingScale { ${spacingFields.join(", ")}, ${spacingFields.length < 5 ? "..Default::default()" : ""} }`);
  if (t.strokeWidth !== undefined) fields.push(`stroke_width: ${n(t.strokeWidth)}`);
  return `    let theme = mui_core::Theme { ${fields.join(", ")}${fields.length ? ", " : ""}${fields.length < 5 ? "..Default::default()" : ""} };\n`;
}
