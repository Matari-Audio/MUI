import type { Theme, Spacing, Align } from "./index.js";
import { q, n } from "./literals.js";
import { validateScene } from "./validate.js";
import { compileTheme } from "./theme-compiler.js";

export type Flow = "row" | "column" | "auto" | "overlay" | { grid: number };
export type Track = "hug" | number | { fraction: number };
export type Horizontal = "left" | "center" | "right" | "stretch";
export type Vertical = "top" | "middle" | "bottom" | "stretch";
export type Pack = "start" | "center" | "end" | "between" | "evenly" | "around";
export type Color = "canvas" | "panel" | "raised" | "text" | "muted" | "outline" | { primary: number } | { primarySoft: number } | { status: number };
export type Round = number | Spacing | { outer: number | Spacing; inner: number | Spacing };
type Dimension = number | "fill" | "hug";
export interface ItemProps {
  layout?: Flow;
  scope?: string;
  text?: string;
  tap?: string;
  hoverable?: boolean;
  hoverColor?: Color;
  extend?: { target: string; direction?: "up" | "right" | "down" | "left" };
  round?: Round;
  color?: Color;
  stroke?: [Color, number];
  gap?: number | Spacing;
  pad?: number | Spacing;
  width?: Dimension;
  height?: Dimension;
  min?: [number, number];
  max?: [number, number];
  grow?: number;
  shrink?: number;
  wrap?: boolean;
  pack?: Pack;
  position?: [Horizontal, Vertical];
  align?: Align;
  alignSelf?: Align;
  columns?: Track[];
  rows?: Track[];
  cell?: [number, number];
  span?: [number, number];
  place?: [Horizontal, Vertical];
}
/** Immutable build-time item; generates ordinary Rust with no JS runtime. */
export class Item {
  constructor(readonly id: string, readonly props: ItemProps = {}, readonly contents: Item[] = [], readonly merges: string[][] = []) {}
  private with(p: Partial<ItemProps>): Item { return new Item(this.id, { ...this.props, ...p }, this.contents, this.merges); }
  children(contents: Item[]): Item { return new Item(this.id, this.props, contents, this.merges); }
  layout(v: Flow): Item { return this.with({ layout: v }); }
  scope(v: string): Item { return this.with({ scope: v }); }
  text(v: string): Item { return this.with({ text: v }); }
  onTap(v: string): Item { return this.with({ tap: v }); }
  hoverable(): Item { return this.with({ hoverable: true }); }
  hoverColor(color: Color): Item { return this.with({ hoverable: true, hoverColor: color }); }
  extendTo(target: string): Item { return this.with({ extend: { target } }); }
  extendToward(direction: "up" | "right" | "down" | "left", target: string): Item { return this.with({ extend: { target, direction } }); }
  merge(ids: string[]): Item { return new Item(this.id, this.props, this.contents, [...this.merges, ids]); }
  round(v: Round): Item { return this.with({ round: v }); }
  color(v: Color): Item { return this.with({ color: v }); }
  stroke(color: Color, width: number): Item { return this.with({ stroke: [color, width] }); }
  gap(v: number | Spacing): Item { return this.with({ gap: v }); }
  pad(v: number | Spacing): Item { return this.with({ pad: v }); }
  width(v: Dimension): Item { return this.with({ width: v }); }
  height(v: Dimension): Item { return this.with({ height: v }); }
  min(w: number, h: number): Item { return this.with({ min: [w, h] }); }
  max(w: number, h: number): Item { return this.with({ max: [w, h] }); }
  grow(v: number): Item { return this.with({ grow: v }); }
  shrink(v: number): Item { return this.with({ shrink: v }); }
  wrap(): Item { return this.with({ wrap: true }); }
  pack(v: Pack): Item { return this.with({ pack: v }); }
  position(x: Horizontal, y: Vertical): Item { return this.with({ position: [x, y] }); }
  center(): Item { return this.position("center", "middle"); }
  align(v: Align): Item { return this.with({ align: v }); }
  alignSelf(v: Align): Item { return this.with({ alignSelf: v }); }
  columns(v: Track[]): Item { return this.with({ columns: v }); }
  rows(v: Track[]): Item { return this.with({ rows: v }); }
  cell(column: number, row: number): Item { return this.with({ cell: [column, row] }); }
  span(columns: number, rows: number): Item { return this.with({ span: [columns, rows] }); }
  place(x: Horizontal, y: Vertical): Item { return this.with({ place: [x, y] }); }
}
export const item = (id: string): Item => new Item(id);
export const container = (children: Item[]): Item => new Item("").children(children);
export const grid = (columns: number): Flow => ({ grid: columns });
export interface ItemDocument { root: Item; theme?: Theme; availableWidth?: number; offered?: [number, number] }
export const defineUi = (doc: ItemDocument): ItemDocument => doc;

const fail = (message: string): never => { throw new Error(`MUI: ${message}`); };
function number(v: number): string {
  if (!Number.isFinite(v) || v < 0) fail("expected a finite nonnegative value");
  return n(v);
}
function integer(v: number, max = 256): string {
  if (!Number.isInteger(v) || v < 1 || v > max) fail(`expected integer in 1..${max}`);
  return String(v);
}
function spacing(v: number | Spacing): string {
  if (typeof v === "number") return number(v);
  if (v.kind === "px") return number(v.value);
  if (!["xs", "s", "m", "l", "xl"].includes(v.value)) fail("invalid spacing token");
  return `mui_core::Spacing::${v.value}()`;
}
function rounding(v: Round): string {
  if (typeof v === "object" && "outer" in v) return `mui_core::Rounding::separate(${spacing(v.outer)}, ${spacing(v.inner)})`;
  return `mui_core::Rounding::Both((${spacing(v)}).into())`;
}
function color(v: Color): string {
  if (typeof v === "string") {
    if (!["canvas", "panel", "raised", "text", "muted", "outline"].includes(v)) fail("invalid color role");
    return `mui_core::Color::${title(v)}`;
  }
  const [key, value] = Object.entries(v)[0];
  if (!Number.isInteger(value) || value < 0 || value >= (key === "status" ? 4 : 3)) fail("invalid color index");
  const name = ({ primary: "Primary", primarySoft: "PrimarySoft", status: "Status" } as Record<string, string>)[key];
  if (!name) fail("invalid color role");
  return `mui_core::Color::${name}(${value})`;
}
function title(v: string): string { return v[0].toUpperCase() + v.slice(1); }
function position(v: [Horizontal, Vertical]): string {
  if (!["left", "center", "right", "stretch"].includes(v[0]) || !["top", "middle", "bottom", "stretch"].includes(v[1])) fail("invalid physical alignment");
  return `mui_layout::Horizontal::${title(v[0])}, mui_layout::Vertical::${title(v[1])}`;
}
function track(v: Track): string {
  if (v === "hug") return "mui_layout::Track::Hug";
  if (typeof v === "number") return `mui_layout::Track::Fixed(${number(v)})`;
  if (v.fraction <= 0) fail("fraction must be positive");
  return `mui_layout::Track::Fraction(${number(v.fraction)})`;
}
export function compileItems(doc: ItemDocument): string {
  validateScene({root:{kind:"leaf",id:"theme-validation",size:[0,0],props:{}},surfaces:[],theme:doc.theme});
  const keys = new Set<string>();
  const references: string[] = [];
  const parents = new Map<string, string | undefined>();
  const groups: {owner: string; members: string[]}[] = [];
  const merged = new Set<string>();
  let count = 0;
  const qualify = (scope: string, id: string) => id.startsWith("/") ? id.slice(1) : scope + id;
  const emit = (item: Item, scope = "", path = "0", depth = 0, parentGrid = false, parent?: string): string => {
    if (++count > 2048 || depth > 64) fail("item budget exceeded");
    const p = item.props;
    if (item.id.includes("/") || item.id.startsWith("@")) fail("invalid item ID");
    if (p.scope !== undefined) {
      if (!p.scope || p.scope.includes("/") || p.scope.startsWith("@")) fail("invalid scope");
      scope += p.scope + "/";
    }
    const id = scope + (item.id || "@" + path);
    if (keys.has(id)) fail(`duplicate item: ${id}`);
    keys.add(id);
    parents.set(id,parent);
    const isGrid = typeof p.layout === "object";
    if ((p.columns || p.rows) && !isGrid) fail("tracks require grid");
    if ((p.cell || p.span || p.place) && !parentGrid) fail("cell, span and place require a grid parent");
    if (p.pack && p.layout === "overlay") fail("overlay does not distribute children");
    if (p.wrap && (isGrid || p.layout === "auto" || p.layout === "overlay")) fail("wrap requires a fixed flex direction");
    if (p.text !== undefined && item.contents.length) fail("text needs its own child");
    let out = `mui_core::item(${q(item.id)})`;
    if (p.layout) {
      const value = isGrid ? `Grid(${integer((p.layout as { grid: number }).grid)})` : title(p.layout as string);
      if (!isGrid && !["row", "column", "auto", "overlay"].includes(p.layout as string)) fail("invalid layout");
      out += `.layout(mui_layout::Flow::${value})`;
    }
    if (p.scope !== undefined) out += `.scope(${q(p.scope)})`;
    if (p.text !== undefined) out += `.text(${q(p.text)})`;
    if (p.tap !== undefined) { if (!p.tap) fail("empty action ID"); out += `.on_tap(${q(p.tap)})`; }
    if (p.hoverable) out += ".hoverable()";
    if (p.hoverColor !== undefined) out += `.hover_color(${color(p.hoverColor)})`;
    if (p.extend) {
      references.push(qualify(scope, p.extend.target));
      if (p.extend.direction && !["up", "right", "down", "left"].includes(p.extend.direction)) fail("invalid direction");
      out += p.extend.direction ? `.extend_toward(mui_core::Direction::${title(p.extend.direction)}, ${q(p.extend.target)})` : `.extend_to(${q(p.extend.target)})`;
    }
    if (p.round !== undefined) out += `.round(${rounding(p.round)})`;
    if (p.color !== undefined) out += `.color(${color(p.color)})`;
    if (p.stroke) out += `.stroke(${color(p.stroke[0])}, ${number(p.stroke[1])})`;
    for (const key of ["gap", "pad"] as const) if (p[key] !== undefined) out += `.${key}(${spacing(p[key]!)})`;
    for (const key of ["width", "height"] as const) if (p[key] !== undefined) {
      const v = p[key]!;
      if (typeof v !== "number" && v !== "fill" && v !== "hug") fail("invalid dimension");
      out += `.${key}(${typeof v === "number" ? number(v) : `mui_layout::Sizing::${title(v)}`})`;
    }
    for (const key of ["min", "max"] as const) if (p[key]) out += `.${key}(${p[key]!.map(number).join(", ")})`;
    for (const key of ["grow", "shrink"] as const) if (p[key] !== undefined) out += `.${key}(${number(p[key]!)})`;
    if (p.wrap) out += ".wrap()";
    if (p.pack) {
      const name = ({ start: "Start", center: "Center", end: "End", between: "SpaceBetween", evenly: "SpaceEvenly", around: "SpaceAround" } as Record<string, string>)[p.pack];
      if (!name) fail("invalid distribution");
      out += `.pack(mui_layout::Justify::${name})`;
    }
    if (p.position) out += `.position(${position(p.position)})`;
    for (const key of ["align", "alignSelf"] as const) if (p[key]) {
      if (!["start", "center", "end", "stretch"].includes(p[key]!)) fail("invalid alignment");
      out += `.${key === "alignSelf" ? "align_self" : key}(mui_layout::Align::${title(p[key]!)})`;
    }
    for (const key of ["columns", "rows"] as const) if (p[key]) {
      integer(p[key]!.length); out += `.${key}([${p[key]!.map(track).join(", ")}])`;
    }
    for (const key of ["cell", "span"] as const) if (p[key]) out += `.${key}(${p[key]!.map(v => integer(v)).join(", ")})`;
    if (p.place) out += `.place(${position(p.place)})`;
    for (const group of item.merges) {
      groups.push({owner:id,members:group.map(m=>qualify(scope,m))});
      if (!group.length) fail("empty merge");
      for (const member of group) {
        const key = qualify(scope, member);
        if (merged.has(key)) fail(`item belongs to multiple merges: ${key}`);
        merged.add(key); references.push(key);
      }
      out += `.merge([${group.map(q).join(", ")}])`;
    }
    if (item.contents.length) out += `.children([\n${item.contents.map((child, i) => emit(child, scope, `${path}.${i}`, depth + 1, isGrid, id)).join(",\n")}\n])`;
    return out;
  };
  const root = emit(doc.root);
  for (const key of references) if (!keys.has(key)) fail(`missing item: ${key}`);
  for (const group of groups) for (const member of group.members) {
    let ancestor: string | undefined = member;
    while (ancestor !== undefined && ancestor !== group.owner) ancestor = parents.get(ancestor);
    if (ancestor === undefined) fail("merge needs a common ancestor");
  }
  if (doc.offered && doc.availableWidth !== undefined) fail("use offered or availableWidth, not both");
  let result = root + ".build_with(theme)?";
  if (doc.availableWidth !== undefined) result += `.available_width(${number(doc.availableWidth)})`;
  if (doc.offered) result += `.offered(${doc.offered.map(number).join(", ")})`;
  return `// @generated by @matari/mui. DO NOT EDIT BY HAND.\npub fn generated_ui() -> Result<mui_core::Ui, mui_core::SceneError> {\n${compileTheme(doc.theme)}    Ok(${result})\n}\n`;
}
