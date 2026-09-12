export type Align = "start" | "center" | "end" | "stretch";
export type Justify = "start" | "center" | "end" | "space-between";

export type Insets = number | { left: number; right: number; top: number; bottom: number };
export interface LayoutProps {
  /// Only a node you intend to look up, or take a surface from, needs a name.
  id?: string;
  gap?: number;
  padding?: Insets;
  min?: [number, number];
  max?: [number, number];
  grow?: number;
  align?: Align;
  justify?: Justify;
}

export type Node =
  | { kind: "leaf"; size: [number, number]; props: LayoutProps }
  | { kind: "row" | "column" | "overlay"; children: Node[]; props: LayoutProps };

const props = (p?: LayoutProps): LayoutProps => p ?? {};
export const leaf = (size: [number, number], p?: LayoutProps): Node => ({ kind: "leaf", size, props: props(p) });
export const row = (children: Node[], p?: LayoutProps): Node => ({ kind: "row", children, props: props(p) });
export const column = (children: Node[], p?: LayoutProps): Node => ({ kind: "column", children, props: props(p) });
export const overlay = (children: Node[], p?: LayoutProps): Node => ({ kind: "overlay", children, props: props(p) });

export type Spacing = { kind: "px"; value: number } | { kind: "token"; value: "xs" | "s" | "m" | "l" | "xl" };
export const px = (value: number): Spacing => ({ kind: "px", value });
export const space = {
  xs: { kind: "token", value: "xs" } as const,
  s: { kind: "token", value: "s" } as const,
  m: { kind: "token", value: "m" } as const,
  l: { kind: "token", value: "l" } as const,
  xl: { kind: "token", value: "xl" } as const,
};

export type Radius =
  | { kind: "global" }
  | { kind: "absolute"; value: number }
  | { kind: "parent-normalized"; parent: string; scale: number };
export const radius = {
  global: { kind: "global" } as const,
  absolute: (value: number): Radius => ({ kind: "absolute", value }),
  parentNormalized: (parent: string, scale = 1): Radius => ({ kind: "parent-normalized", parent, scale }),
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
  | { kind: "frame"; id: string; radius: Radius }
  | { kind: "merge"; id: string; inputs: string[]; corners: CornerRule }
  | { kind: "inset" | "outset"; id: string; parent: string; distance: Spacing };

/// The layout node's id is the surface id -- one name per box.
export const frameSurface = (id: string, r: Radius = radius.global): Surface => ({ kind: "frame", id, radius: r });
export const mergeSurface = (id: string, inputs: string[], c: CornerRule = corners.global): Surface => ({ kind: "merge", id, inputs, corners: c });
export const insetSurface = (id: string, parent: string, distance: Spacing): Surface => ({ kind: "inset", id, parent, distance });
export const outsetSurface = (id: string, parent: string, distance: Spacing): Surface => ({ kind: "outset", id, parent, distance });

/// A colour with no lightness: hue in degrees and chroma 0..~0.4.
///
/// Lightness is missing on purpose. Hue and chroma are what makes a role "our
/// blue" or "the warning amber"; lightness is a consequence of the ground the
/// role is painted on, which `mode` decides. That is why one declaration
/// serves both a dark and a light theme.
export type Pigment = [hue: number, chroma: number];

/// The colours declared by hand. Surfaces, hover, pressed, disabled, dimmed ink
/// and both themes are derived from these, so anything left out keeps the
/// Rust-side default rather than being restated here.
export interface Palette {
  /// Which way the interface is lit. The entire theme switch.
  mode?: "dark" | "light";
  /// The greys: ground, panels, fields, ink. A little chroma tints everything.
  neutral?: Pigment;
  primary?: Pigment;
  secondary?: Pigment;
  tertiary?: Pigment;
  success?: Pigment;
  warning?: Pigment;
  danger?: Pigment;
  /// One layer's worth of perceptual lightness. Always positive; `mode` decides
  /// which way depth points.
  step?: number;
  /// What a control gains under the pointer. Always positive, same reason.
  hover?: number;
}

export interface Theme {
  corners?: { convex: number; concave: number };
  spacing?: Partial<Record<"xs" | "s" | "m" | "l" | "xl", number>>;
  palette?: Palette;
  strokeWidth?: number;
}
export interface Scene { root: Node; surfaces: Surface[]; theme?: Theme; offered?: [number, number] }
export const defineScene = <T extends Scene>(scene: T): T => scene;
