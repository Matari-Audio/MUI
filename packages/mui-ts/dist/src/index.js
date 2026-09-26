const props = (p) => p ?? {};
export const leaf = (size, p) => ({ kind: "leaf", size, props: props(p) });
export const row = (children, p) => ({ kind: "row", children, props: props(p) });
export const column = (children, p) => ({ kind: "column", children, props: props(p) });
export const overlay = (children, p) => ({ kind: "overlay", children, props: props(p) });
export const px = (value) => ({ kind: "px", value });
export const space = {
    xs: { kind: "token", value: "xs" },
    s: { kind: "token", value: "s" },
    m: { kind: "token", value: "m" },
    l: { kind: "token", value: "l" },
    xl: { kind: "token", value: "xl" },
};
export const radius = {
    global: { kind: "global" },
    absolute: (value) => ({ kind: "absolute", value }),
    parentNormalized: (parent, scale = 1) => ({ kind: "parent-normalized", parent, scale }),
};
export const corners = {
    global: { kind: "global" },
    absolute: (convex, concave) => ({ kind: "absolute", convex, concave }),
    scaled: (scale) => ({ kind: "global-scaled", scale }),
};
/// The layout node's id is the surface id -- one name per box.
export const frameSurface = (id, r = radius.global) => ({ kind: "frame", id, radius: r });
export const mergeSurface = (id, inputs, c = corners.global) => ({ kind: "merge", id, inputs, corners: c });
export const insetSurface = (id, parent, distance) => ({ kind: "inset", id, parent, distance });
export const outsetSurface = (id, parent, distance) => ({ kind: "outset", id, parent, distance });
export const defineScene = (scene) => scene;
