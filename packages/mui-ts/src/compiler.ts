// Build-time TS -> typed Rust builders. No JavaScript runtime is required in the plugin.
import { writeFile } from "node:fs/promises";
import { pathToFileURL } from "node:url";
import { validateScene } from "./validate.js";
import type { Align, CornerRule, FrameRadius, Insets, Justify, Node, Scene, Spacing, Surface, ColorSeeds } from "./index.js";


const q = (s: string) => {
  let out = '"';
  for (const ch of s) {
    const code = ch.codePointAt(0)!;
    if (code >= 0xd800 && code <= 0xdfff) throw new Error("unpaired surrogate in string");
    if (ch === '"' || ch === "\\") out += "\\" + ch;
    else if (code < 32 || code === 127) out += `\\u{${code.toString(16)}}`;
    else out += ch;
  }
  return out + '"';
};
const n = (v: number) => {
  if (!Number.isFinite(v)) throw new Error(`non-finite number: ${v}`);
  return `${Object.is(v,-0) ? "-0" : String(v)}f64`;
};
const align = (a?: Align) => a ? `mui_layout::Align::${({start:"Start",center:"Center",end:"End",stretch:"Stretch"} as const)[a]}` : null;
const justify = (j?: Justify) => j ? `mui_layout::Justify::${({start:"Start",center:"Center",end:"End","space-between":"SpaceBetween"} as const)[j]}` : null;

function insets(v: Insets): string {
  if (typeof v === "number" || "kind" in v) return `.padding(${lengthValue(v)})`;
  return `.insets(mui_layout::Insets { left: ${n(v.left)}, right: ${n(v.right)}, top: ${n(v.top)}, bottom: ${n(v.bottom)} })`;
}
function decorate(base: string, p: Node["props"] = {}): string {
  let s = base;
  if (p.gap !== undefined) s += `.gap(${lengthValue(p.gap)})`;
  if (p.padding !== undefined) s += insets(p.padding);
  if (p.min) s += `.min_size(mui_layout::Size::new(${n(p.min[0])}, ${n(p.min[1])}))`;
  if (p.max) s += `.max_size(mui_layout::Size::new(${n(p.max[0])}, ${n(p.max[1])}))`;
  if (p.axis) s += `.axis(mui_layout::Axis::${p.axis === "auto" ? "Auto" : p.axis === "row" ? "Row" : "Column"})`;
  if (p.scope !== undefined) s += `.scope(${q(p.scope)})`;
  if (p.shrink !== undefined) s += `.shrink(${n(p.shrink)})`;
  for (const axis of ["width", "height"] as const) {
    const v = p[axis];
    if (v !== undefined) s += `.${axis}(${typeof v === "number" ? n(v) : `mui_layout::Sizing::${v === "fill" ? "Fill" : "Hug"}`})`;
  }
  if (p.wrap) s += `.wrap()`;
  if (p.grow !== undefined) s += `.grow(${n(p.grow)})`;
  const a = align(p.align); if (a) s += `.align(${a})`;
  const j = justify(p.justify); if (j) s += `.justify(${j})`;
  return s;
}
function node(v: Node, depth = 1): string {
  const pad = "    ".repeat(depth);
  if (v.kind === "leaf") return decorate(`mui_layout::Node::leaf(${q(v.id)}, mui_layout::Size::new(${n(v.size[0])}, ${n(v.size[1])}))`, v.props);
  const ctor = v.kind === "row" ? "row" : v.kind === "column" ? "column" : "overlay";
  const children = v.children.map(c => `${pad}    ${node(c, depth + 1)}`).join(",\n");
  return decorate(`mui_layout::Node::${ctor}(${q(v.id)}, vec![\n${children}\n${pad}])`, v.props);
}
function lengthValue(v: number | Spacing): string { return typeof v === "number" ? n(v) : spacing(v); }
function spacing(v: Spacing): string {
  return v.kind === "px" ? `mui_core::Spacing::px(${n(v.value)})` : `mui_core::Spacing::${v.value}()`;
}
function radius(v: FrameRadius): string {
  switch (v.kind) {
    case "global": return `mui_core::FrameRadius::Global`;
    case "absolute": return `mui_core::FrameRadius::Absolute(${n(v.value)})`;
    case "parent-normalized": return `mui_core::FrameRadius::ParentNormalized { parent: ${q(v.parent)}.into(), scale: ${n(v.scale)} }`;
  }
}
function cornerRule(v: CornerRule): string {
  switch (v.kind) {
    case "global": return `mui_core::CornerRule::Global`;
    case "absolute": return `mui_core::CornerRule::Absolute(mui_core::CornerProfile::new(${n(v.convex)}, ${n(v.concave)}))`;
    case "global-scaled": return `mui_core::CornerRule::GlobalScaled(${n(v.scale)})`;
  }
}
function surface(v: Surface): string {
  switch (v.kind) {
    case "frame": return `mui_core::SurfaceSpec::frame(${q(v.id)}, ${q(v.layout)}).radius(${radius(v.radius)})` + (v.extension ? `.extend_to(mui_core::Edge::${v.extension.edge[0].toUpperCase()+v.extension.edge.slice(1)}, ${q(v.extension.target)})` : "");
    case "merge": return `mui_core::SurfaceSpec::merge(${q(v.id)}, [${v.inputs.map(q).join(", ")}]).corners(${cornerRule(v.corners)})`;
    case "inset": return `mui_core::SurfaceSpec::inset(${q(v.id)}, ${q(v.parent)}, ${spacing(v.distance)})`;
    case "outset": return `mui_core::SurfaceSpec::outset(${q(v.id)}, ${q(v.parent)}, ${spacing(v.distance)})`;
  }
}
const rgb = (v:number) => `mui_core::Rgb::new(${(v >>> 16) & 255}, ${(v >>> 8) & 255}, ${v & 255})`;
const seeds = (v:ColorSeeds, fallback:string) => `mui_core::Seeds { primary: [${v.primary.map(rgb).join(", ")}], neutral: ${rgb(v.neutral)}, ${v.status ? `status: [${v.status.map(rgb).join(", ")}],` : `..${fallback}`} }`;
export function compile(scene: Scene): string {
  validateScene(scene);
  const t = scene.theme ?? {};
  let out = `// @generated by @matari/mui. DO NOT EDIT BY HAND.\n`;
  out += `pub fn generated_scene() -> mui_core::SceneSpec {\n`;
  out += `    let root = ${node(scene.root, 1)};\n`;
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
  out += `    let theme = mui_core::Theme { ${fields.join(", ")}${fields.length ? ", " : ""}${fields.length < 5 ? "..Default::default()" : ""} };\n`;
  out += `    mui_core::SceneSpec::new(root).theme(theme)`;
  if (scene.offered) out += `.offered(mui_layout::Size::new(${n(scene.offered[0])}, ${n(scene.offered[1])}))`;
  if (scene.availableWidth !== undefined) out += `.available_width(${n(scene.availableWidth)})`;
  for (const s of scene.surfaces) out += `\n        .surface(${surface(s)})`;
  out += `\n}\n`;
  return out;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const [, , input, output] = process.argv;
  if (!input || !output) throw new Error("usage: compiler.js <scene.js> <output.rs>");
  const mod = await import(pathToFileURL(input).href);
  await writeFile(output, compile(mod.default), "utf8");
}
