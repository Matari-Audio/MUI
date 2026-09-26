// Build-time TS -> typed Rust builders. No JavaScript runtime is required in the plugin.
// @ts-ignore - keep this package dependency-free; Node provides these modules.
import { rename, unlink, writeFile } from "node:fs/promises";
// @ts-ignore
import { randomUUID } from "node:crypto";
// @ts-ignore
import { pathToFileURL } from "node:url";
// JSON's \uXXXX escapes are not Rust string escapes. Keep valid Unicode in the
// source, use Rust's short escapes where they exist, and reject lone UTF-16
// surrogates before they can become invalid UTF-8 output.
function q(s) {
    let out = '"';
    for (let i = 0; i < s.length; i += 1) {
        const unit = s.charCodeAt(i);
        let codePoint = unit;
        if (unit >= 0xd800 && unit <= 0xdbff) {
            const next = s.charCodeAt(i + 1);
            if (!(next >= 0xdc00 && next <= 0xdfff)) {
                throw new Error(`unpaired high surrogate at string index ${i}`);
            }
            codePoint = 0x10000 + ((unit - 0xd800) << 10) + next - 0xdc00;
            i += 1;
        }
        else if (unit >= 0xdc00 && unit <= 0xdfff) {
            throw new Error(`unpaired low surrogate at string index ${i}`);
        }
        switch (codePoint) {
            case 0x08:
                out += "\\u{8}";
                break;
            case 0x09:
                out += "\\t";
                break;
            case 0x0a:
                out += "\\n";
                break;
            case 0x0c:
                out += "\\u{c}";
                break;
            case 0x0d:
                out += "\\r";
                break;
            case 0x22:
                out += '\\"';
                break;
            case 0x5c:
                out += "\\\\";
                break;
            default:
                out += codePoint <= 0x1f || codePoint === 0x7f
                    ? `\\u{${codePoint.toString(16)}}`
                    : String.fromCodePoint(codePoint);
        }
    }
    return `${out}"`;
}
const n = (v) => {
    if (!Number.isFinite(v))
        throw new Error(`non-finite number: ${v}`);
    const text = String(v);
    return /e/i.test(text) ? text : Number.isInteger(v) ? `${text}.0` : text;
};
const align = (a) => a ? `mui_layout::Align::${{ start: "Start", center: "Center", end: "End", stretch: "Stretch" }[a]}` : null;
const justify = (j) => j ? `mui_layout::Justify::${{ start: "Start", center: "Center", end: "End", "space-between": "SpaceBetween" }[j]}` : null;
function insets(v) {
    if (typeof v === "number")
        return `.padding(${n(v)})`;
    return `.insets(mui_layout::Insets { left: ${n(v.left)}, right: ${n(v.right)}, top: ${n(v.top)}, bottom: ${n(v.bottom)} })`;
}
function decorate(base, p) {
    let s = base;
    if (p.id !== undefined)
        s += `.id(${q(p.id)})`;
    if (p.gap !== undefined)
        s += `.gap(${n(p.gap)})`;
    if (p.padding !== undefined)
        s += insets(p.padding);
    if (p.min)
        s += `.min_size(mui_layout::Size::new(${n(p.min[0])}, ${n(p.min[1])}))`;
    if (p.max)
        s += `.max_size(mui_layout::Size::new(${n(p.max[0])}, ${n(p.max[1])}))`;
    if (p.grow !== undefined)
        s += `.grow(${n(p.grow)})`;
    const a = align(p.align);
    if (a)
        s += `.align(${a})`;
    const j = justify(p.justify);
    if (j)
        s += `.justify(${j})`;
    return s;
}
function node(v, depth = 1) {
    const pad = "    ".repeat(depth);
    if (v.kind === "leaf")
        return decorate(`mui_layout::leaf(${n(v.size[0])}, ${n(v.size[1])})`, v.props);
    const children = v.children.map(c => `${pad}    ${node(c, depth + 1)}`).join(",\n");
    return decorate(`mui_layout::${v.kind}(vec![\n${children}\n${pad}])`, v.props);
}
function spacing(v) {
    return v.kind === "px" ? `mui_core::Spacing::px(${n(v.value)})` : `mui_core::Spacing::${v.value}()`;
}
function radius(v) {
    switch (v.kind) {
        case "global": return `mui_core::Radius::Global`;
        case "absolute": return `mui_core::Radius::Absolute(${n(v.value)})`;
        case "parent-normalized": return `mui_core::Radius::ParentNormalized { parent: ${q(v.parent)}.into(), scale: ${n(v.scale)} }`;
    }
}
function cornerRule(v) {
    switch (v.kind) {
        case "global": return `mui_core::CornerRule::Global`;
        case "absolute": return `mui_core::CornerRule::Absolute(mui_core::CornerProfile::new(${n(v.convex)}, ${n(v.concave)}))`;
        case "global-scaled": return `mui_core::CornerRule::GlobalScaled(${n(v.scale)})`;
    }
}
function surface(v) {
    switch (v.kind) {
        case "frame": return `mui_core::SurfaceSpec::frame(${q(v.id)}).radius(${radius(v.radius)})`;
        case "merge": return `mui_core::SurfaceSpec::merge(${q(v.id)}, [${v.inputs.map(q).join(", ")}]).corners(${cornerRule(v.corners)})`;
        case "inset": return `mui_core::SurfaceSpec::inset(${q(v.id)}, ${q(v.parent)}, ${spacing(v.distance)})`;
        case "outset": return `mui_core::SurfaceSpec::outset(${q(v.id)}, ${q(v.parent)}, ${spacing(v.distance)})`;
    }
}
/// Unstated fields fall through to `Palette::NEUTRAL` rather than being restated
/// here, so the two sides cannot drift.
function palette(p) {
    const fields = [];
    if (p.mode)
        fields.push(`mode: mui_core::Mode::${p.mode === "light" ? "Light" : "Dark"}`);
    for (const k of ["neutral", "primary", "secondary", "tertiary", "success", "warning", "danger"]) {
        const v = p[k];
        if (v)
            fields.push(`${k}: mui_core::Pigment::new(${v.map(n).join(", ")})`);
    }
    for (const k of ["step", "hover"]) {
        const v = p[k];
        if (v !== undefined)
            fields.push(`${k}: ${n(v)}`);
    }
    return `mui_core::Palette { ${fields.join(", ")}${fields.length ? ", " : ""}..mui_core::Palette::NEUTRAL }`;
}
function compile(scene) {
    const t = scene.theme ?? {};
    const c = t.corners ?? { convex: 18, concave: 14 };
    const sp = { xs: 4, s: 8, m: 12, l: 18, xl: 28, ...(t.spacing ?? {}) };
    let out = `// @generated by @matari/mui. DO NOT EDIT BY HAND.\n`;
    out += `pub fn generated_scene() -> mui_core::SceneSpec {\n`;
    out += `    let root = ${node(scene.root, 1)};\n`;
    out += `    let theme = mui_core::Theme {\n`;
    out += `        corners: mui_core::CornerProfile::new(${n(c.convex)}, ${n(c.concave)}),\n`;
    out += `        spacing: mui_core::SpacingScale { xs: ${n(sp.xs)}, s: ${n(sp.s)}, m: ${n(sp.m)}, l: ${n(sp.l)}, xl: ${n(sp.xl)} },\n`;
    out += `        palette: ${palette(t.palette ?? {})},\n`;
    out += `        stroke_width: ${n(t.strokeWidth ?? 1.5)},\n    };\n`;
    out += `    mui_core::SceneSpec::new(root).theme(theme)`;
    if (scene.offered)
        out += `.offered(mui_layout::Size::new(${n(scene.offered[0])}, ${n(scene.offered[1])}))`;
    for (const s of scene.surfaces)
        out += `\n        .surface(${surface(s)})`;
    out += `\n}\n`;
    return out;
}
const [, , input, output] = process.argv;
if (!input || !output)
    throw new Error("usage: compiler.js <scene.js> <output.rs>");
const mod = await import(pathToFileURL(input).href);
const scene = mod.default;
// Keep the temporary beside the destination so rename is atomic on one file
// system and an interrupted generation cannot leave a truncated fixture.
const temporary = `${output}.${randomUUID()}.tmp`;
try {
    await writeFile(temporary, compile(scene), "utf8");
    await rename(temporary, output);
}
catch (error) {
    try {
        await unlink(temporary);
    }
    catch { /* preserve the original error */ }
    throw error;
}
