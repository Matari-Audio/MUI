export type Align = "start" | "center" | "end" | "stretch";
export type Justify = "start" | "center" | "end" | "space-between";
export type Insets = number | {
    left: number;
    right: number;
    top: number;
    bottom: number;
};
export interface LayoutProps {
    id?: string;
    gap?: number;
    padding?: Insets;
    min?: [number, number];
    max?: [number, number];
    grow?: number;
    align?: Align;
    justify?: Justify;
}
export type Node = {
    kind: "leaf";
    size: [number, number];
    props: LayoutProps;
} | {
    kind: "row" | "column" | "overlay";
    children: Node[];
    props: LayoutProps;
};
export declare const leaf: (size: [number, number], p?: LayoutProps) => Node;
export declare const row: (children: Node[], p?: LayoutProps) => Node;
export declare const column: (children: Node[], p?: LayoutProps) => Node;
export declare const overlay: (children: Node[], p?: LayoutProps) => Node;
export type Spacing = {
    kind: "px";
    value: number;
} | {
    kind: "token";
    value: "xs" | "s" | "m" | "l" | "xl";
};
export declare const px: (value: number) => Spacing;
export declare const space: {
    xs: {
        readonly kind: "token";
        readonly value: "xs";
    };
    s: {
        readonly kind: "token";
        readonly value: "s";
    };
    m: {
        readonly kind: "token";
        readonly value: "m";
    };
    l: {
        readonly kind: "token";
        readonly value: "l";
    };
    xl: {
        readonly kind: "token";
        readonly value: "xl";
    };
};
export type Radius = {
    kind: "global";
} | {
    kind: "absolute";
    value: number;
} | {
    kind: "parent-normalized";
    parent: string;
    scale: number;
};
export declare const radius: {
    global: {
        readonly kind: "global";
    };
    absolute: (value: number) => Radius;
    parentNormalized: (parent: string, scale?: number) => Radius;
};
export type CornerRule = {
    kind: "global";
} | {
    kind: "absolute";
    convex: number;
    concave: number;
} | {
    kind: "global-scaled";
    scale: number;
};
export declare const corners: {
    global: {
        readonly kind: "global";
    };
    absolute: (convex: number, concave: number) => CornerRule;
    scaled: (scale: number) => CornerRule;
};
export type Surface = {
    kind: "frame";
    id: string;
    radius: Radius;
} | {
    kind: "merge";
    id: string;
    inputs: string[];
    corners: CornerRule;
} | {
    kind: "inset" | "outset";
    id: string;
    parent: string;
    distance: Spacing;
};
export declare const frameSurface: (id: string, r?: Radius) => Surface;
export declare const mergeSurface: (id: string, inputs: string[], c?: CornerRule) => Surface;
export declare const insetSurface: (id: string, parent: string, distance: Spacing) => Surface;
export declare const outsetSurface: (id: string, parent: string, distance: Spacing) => Surface;
export type Pigment = [hue: number, chroma: number];
export interface Palette {
    mode?: "dark" | "light";
    neutral?: Pigment;
    primary?: Pigment;
    secondary?: Pigment;
    tertiary?: Pigment;
    success?: Pigment;
    warning?: Pigment;
    danger?: Pigment;
    step?: number;
    hover?: number;
}
export interface Theme {
    corners?: {
        convex: number;
        concave: number;
    };
    spacing?: Partial<Record<"xs" | "s" | "m" | "l" | "xl", number>>;
    palette?: Palette;
    strokeWidth?: number;
}
export interface Scene {
    root: Node;
    surfaces: Surface[];
    theme?: Theme;
    offered?: [number, number];
}
export declare const defineScene: <T extends Scene>(scene: T) => T;
