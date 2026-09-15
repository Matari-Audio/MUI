export type Align = "start" | "center" | "end" | "stretch" | "baseline";
export type Overflow = "fit" | "clip" | "scroll";
export type Justify = "start" | "center" | "end" | "space-between";

export type Insets = number | Spacing | { left: number; right: number; top: number; bottom: number };
export interface LayoutProps {
  gap?: number | Spacing;
  padding?: Insets;
  min?: [number, number];
  max?: [number, number];
  grow?: number;
  shrink?: number;
  axis?: "row" | "column" | "auto";
  width?: number | "hug" | "fill";
  height?: number | "hug" | "fill";
  wrap?: boolean;
  overflow?: Overflow;
  scope?: string;
  align?: Align;
  justify?: Justify;
}

export type Node =
  | { kind: "leaf"; id: string; size: [number, number]; props: LayoutProps }
  | { kind: "row" | "column" | "stack"; id: string; children: Node[]; props: LayoutProps };

const props = (p?: LayoutProps): LayoutProps => p ?? {};
export const leaf = (id: string, size: [number, number], p?: LayoutProps): Node => ({ kind: "leaf", id, size, props: props(p) });
export const row = (id: string, children: Node[], p?: LayoutProps): Node => ({ kind: "row", id, children, props: props(p) });
export const column = (id: string, children: Node[], p?: LayoutProps): Node => ({ kind: "column", id, children, props: props(p) });
export const flow = (children: Node[], p?: LayoutProps): Node => row("", children, p);
export const stack = (id: string, children: Node[], p?: LayoutProps): Node => ({ kind: "stack", id, children, props: props(p) });

export type Spacing = { kind: "px"; value: number } | { kind: "token"; value: "xs" | "s" | "m" | "l" | "xl" };
export const px = (value: number): Spacing => ({ kind: "px", value });
export const space = {
  xs: { kind: "token", value: "xs" } as const,
  s: { kind: "token", value: "s" } as const,
  m: { kind: "token", value: "m" } as const,
  l: { kind: "token", value: "l" } as const,
  xl: { kind: "token", value: "xl" } as const,
};

export type FrameRadius =
  | { kind: "global" }
  | { kind: "absolute"; value: number }
  | { kind: "parent-normalized"; parent: string; scale: number };
export const radius = {
  global: { kind: "global" } as const,
  absolute: (value: number): FrameRadius => ({ kind: "absolute", value }),
  parentNormalized: (parent: string, scale = 1): FrameRadius => ({ kind: "parent-normalized", parent, scale }),
};

export type CornerRule =
  | { kind: "global" }
  | { kind: "absolute"; convex: number; concave: number }
  | { kind: "global-scaled"; scale: number };
export const corners = {
  global: { kind: "global" } as const,
  absolute: (convex: number, concave: number): CornerRule => ({ kind: "absolute", convex, concave }),
  scaled: (scale: number): CornerRule => ({ kind: "global-scaled", scale }),
};

export type Surface =
  | { kind: "frame"; id: string; layout: string; radius: FrameRadius; extension?: {edge: "top" | "right" | "bottom" | "left"; target: string} }
  | { kind: "merge"; id: string; inputs: string[]; corners: CornerRule }
  | { kind: "inset" | "outset"; id: string; parent: string; distance: Spacing };

export const frameSurface = (id: string, layout: string, r: FrameRadius = radius.global): Extract<Surface, {kind:"frame"}> => ({ kind: "frame", id, layout, radius: r });
export const mergeSurface = (id: string, inputs: string[], c: CornerRule = corners.global): Surface => ({ kind: "merge", id, inputs, corners: c });
export const insetSurface = (id: string, parent: string, distance: Spacing): Surface => ({ kind: "inset", id, parent, distance });
export const outsetSurface = (id: string, parent: string, distance: Spacing): Surface => ({ kind: "outset", id, parent, distance });

export interface ColorSeeds { primary: [number, number, number]; neutral: number; status?: [number, number, number, number] }
export interface Theme {
  contrast?: { text: number; graphics: number };
  hoverShift?: number;
  mode?: "light" | "dark";
  colors?: ColorSeeds;
  darkColors?: ColorSeeds;
  corners?: { convex: number; concave: number };
  spacing?: Partial<Record<"xs" | "s" | "m" | "l" | "xl", number>>;
  strokeWidth?: number;
}
export interface Scene { root: Node; surfaces: Surface[]; theme?: Theme; offered?: [number, number]; availableWidth?: number }
export const defineScene = <T extends Scene>(scene: T): T => scene;

export const extendTo = (frame: Extract<Surface, {kind:"frame"}>, edge: "top" | "right" | "bottom" | "left", target: string): Extract<Surface, {kind:"frame"}> => ({...frame, extension:{edge,target}});

export { Item, item, container, grid, defineUi } from "./items.js";
export type { ItemDocument, ItemProps, Flow, Track, Horizontal, Vertical, Pack, Color, Round } from "./items.js";
