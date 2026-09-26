//! The DSL v2 migration rules, as data.
//!
//! Adding a rule is one line in `RULES` (order matters only among `Call`
//! rules for the same method: the first match wins):
//!
//! - `Method { old, new, gate }`     `.old(` / `.old::<` -> `.new(`
//! - `Function { old, new }`         `old(` (free fn, `mui::old(`, `use mui::{old}`) -> `new`
//! - `FnCall { name, args, to, needs }` call-shape rewrite of a free fn: `name(args)` (or a
//!   mui path to it) becomes template `to`, with `$n` as for `Call` and `$path` the
//!   path before the name (`mui::widgets::`); the first matching shape wins
//! - `Type { old, new }`             path/type ident, only where it resolves to a mui crate
//! - `Macro { old, new }`            `old!` -> `new!`
//! - `MacroHead { old, new, heads }` `old![a, b; ..]` -> `new![<heads[n-1] with $1 = a, $2 = b>; ..]`,
//!   the template picked by the head's argument count n
//! - `Call { chain, to, gate, needs }` call-shape rewrite: match `.m(args)` (or a
//!   chain of them) with arg patterns, replace with template `to` (`$1`.. are
//!   the `Any`/`Has` args in order, `$!1` is the logical negation of `$1`). `needs`
//!   names idents the output uses so a missing import is warned about.
//!   `Arg::After(p)` matches an argument starting with `p` and captures the rest.
//! - `DropImport { name }`           remove `name` from `use <mui>::..` lists
//! - `Qualify { names, prefix }`     bare glob-imported `Name` -> `prefix` + `Name`
//! - `Assoc { ty, name, args, to, bare }` call-shape rewrite of an associated fn
//!   `Ty::name(args)` (optionally path-qualified) into template `to`; `$path` is
//!   the path before `Ty`, or `bare` when `Ty` was named bare
//! - `Moved { names, from, to }`     `from::Name` -> `to::Name` for each moved `Name`,
//!   in paths and `use` trees (a mixed brace list is split into two `use`s)
//! - `Field { recv, name, read, write }` a public field that became accessors:
//!   `<recv>.name = e` -> `<recv>` + `write` (`$1` = e), any other `<recv>.name`
//!   -> `<recv>` + `read` (`""`: reads are left for rustc to report); the
//!   receiver's last segment must be one of `recv`
//! - `Corner { field, axis, to }`    `.field.axis` -> `.to` where the receiver is
//!   clearly a `Rect`/`Bounds` (see `rect_receiver`)
//! - `Retype { field, from, to }`    `field: From::..`, `field = From::..` and
//!   `field ==/!= From::..` -> `To::..`: a field whose type changed
//! - `Manual { pattern, note }`      report `file:line: note` wherever the token
//!   sequence `pattern` appears in a file that uses mui (no rewrite)
//!
//! `Gate::Any` applies everywhere; `Gate::Mui` only in files that import a
//! mui crate; `Gate::MuiChain` additionally needs the receiver to end in
//! `)` / `]` (a builder chain, not `handle.join()`).
//!
//! Tuple results that became named structs: add a line to `TUPLES`.
//! Moved crates: add `(old path suffix, new path suffix)` to `CARGO_MOVES`.

#[derive(Clone, Copy, Debug)]
pub enum Gate {
    Any,
    Mui,
    MuiChain,
}

#[derive(Clone, Copy, Debug)]
pub enum Arg {
    /// Any argument, captured as `$n`.
    Any,
    /// Exactly this text (whitespace-insensitive).
    Is(&'static str),
    /// Argument text contains this (whitespace-insensitive).
    Has(&'static str),
    /// Argument starts with this (whitespace-insensitive); the rest is captured.
    After(&'static str),
    /// Any argument that does not contain this (whitespace-insensitive), captured.
    Not(&'static str),
    /// A shared borrow `&x` (not `&mut x`); `x` is captured.
    Shared,
}

#[derive(Clone, Copy, Debug)]
pub enum Rule {
    Method { old: &'static str, new: &'static str, gate: Gate },
    Function { old: &'static str, new: &'static str },
    FnCall { name: &'static str, args: &'static [Arg], to: &'static str, needs: &'static [&'static str] },
    Type { old: &'static str, new: &'static str },
    #[expect(dead_code, reason = "no plain macro renames in the spec yet")]
    Macro { old: &'static str, new: &'static str },
    MacroHead { old: &'static str, new: &'static str, heads: &'static [&'static str] },
    Call { chain: &'static [(&'static str, &'static [Arg])], to: &'static str, gate: Gate, needs: &'static [&'static str] },
    DropImport { name: &'static str },
    Qualify { names: &'static [&'static str], prefix: &'static str },
    Assoc { ty: &'static str, name: &'static str, args: &'static [Arg], to: &'static str, bare: &'static str },
    Moved { names: &'static [&'static str], from: &'static str, to: &'static str },
    Field { recv: &'static [&'static str], name: &'static str, read: &'static str, write: &'static str },
    Corner { field: &'static str, axis: &'static str, to: &'static str },
    Retype { field: &'static str, from: &'static str, to: &'static str },
    Manual { pattern: &'static str, note: &'static str },
}

use Arg::{After, Any as A, Has, Is, Not, Shared};
use Gate::{Any as G, Mui, MuiChain};
use Rule::*;

const SS: &str = "Align::Start";
const CC: &str = "Align::Center";

pub const RULES: &[Rule] = &[
    // Constructors.
    Function { old: "leaf", new: "block" },
    Function { old: "column", new: "col" },
    Function { old: "overlay", new: "stack" },
    Function { old: "canvas_cached", new: "canvas_keyed" },
    // Text roles.
    Function { old: "label", new: "body" },
    // Layout sugar: absolute placement.
    Call { chain: &[("anchor", &[Is(SS), Is(SS)]), ("offset", &[A, A])], to: ".at($1, $2)", gate: Mui, needs: &[] },
    Call { chain: &[("offset", &[A, A]), ("anchor", &[Is(SS), Is(SS)])], to: ".at($1, $2)", gate: Mui, needs: &[] },
    Call { chain: &[("anchor", &[Is(CC), Is(CC)]), ("offset", &[A, A])], to: ".centered_at($1, $2)", gate: Mui, needs: &[] },
    Call { chain: &[("offset", &[A, A]), ("anchor", &[Is(CC), Is(CC)])], to: ".centered_at($1, $2)", gate: Mui, needs: &[] },
    DropImport { name: "Sugar" },
    DropImport { name: "IntoLen" },
    Manual { pattern: "Sugar", note: "`Sugar` is gone: layout verbs are inherent on `Node`" },
    Manual { pattern: "IntoLen", note: "`IntoLen` is gone: `Len` has `From<i32/f32/f64>`" },
    // Accessibility.
    Type { old: "Kind", new: "A11y" },
    Call { chain: &[("role", &[Has("Kind::")])], to: ".a11y($1)", gate: Mui, needs: &[] },
    Call { chain: &[("role", &[Has("A11y::")])], to: ".a11y($1)", gate: Mui, needs: &[] },
    Call { chain: &[("label", &[A])], to: ".named($1)", gate: Mui, needs: &[] },
    Qualify {
        names: &["Background", "Surface", "Raised", "Field", "Level", "Primary", "Secondary", "Tertiary", "Success", "Warning", "Danger", "Ink", "Dim"],
        prefix: "Role::",
    },
    // Switches.
    // On builder chains only: `palette.disabled(color)` is a colour, not a switch.
    Call { chain: &[("disabled", &[Is("true")])], to: ".disabled()", gate: MuiChain, needs: &[] },
    Call { chain: &[("disabled", &[Is("false")])], to: "", gate: MuiChain, needs: &[] },
    Call { chain: &[("disabled", &[A])], to: ".when($1, Styled::disabled)", gate: MuiChain, needs: &["Styled"] },
    // The overlay scrollbar is on by default, so the switch is `no_scrollbar`.
    Call { chain: &[("scroll_bar", &[Is("true")])], to: "", gate: G, needs: &[] },
    Call { chain: &[("scroll_bar", &[Is("false")])], to: ".no_scrollbar()", gate: G, needs: &[] },
    Call { chain: &[("scroll_bar", &[A])], to: ".when($!1, Styled::no_scrollbar)", gate: G, needs: &["Styled"] },
    // Motion.
    Call { chain: &[("transition", &[A])], to: ".animate_with($1)", gate: Mui, needs: &[] },
    Method { old: "layout_transition", new: "animate_layout_with", gate: G },
    // Weld and material.
    Method { old: "weld_with", new: "weld", gate: G },
    // `.weld_morph(p)` / `.weld_quality(q)` kept the weld set before them:
    // once both sides are `.weld(..)`, the second folds into the first.
    Call { chain: &[("weld", &[A]), ("weld", &[After("Weld::default()")])], to: ".weld($1$2)", gate: G, needs: &[] },
    Call { chain: &[("weld_shape", &[])], to: ".weld(Weld::shape())", gate: G, needs: &["Weld"] },
    Call { chain: &[("weld_borders", &[])], to: ".weld(Weld::borders())", gate: G, needs: &["Weld"] },
    Call { chain: &[("weld_morph", &[A])], to: ".weld(Weld::default().morph($1))", gate: G, needs: &["Weld"] },
    Call { chain: &[("weld_quality", &[A])], to: ".weld(Weld::default().quality($1))", gate: G, needs: &["Weld"] },
    Call { chain: &[("without_weld", &[])], to: ".weld(Weld::off())", gate: G, needs: &["Weld"] },
    Method { old: "exclude_from_weld", new: "unwelded", gate: G },
    Manual { pattern: ". gpu_weld", note: "`.gpu_weld(w)` became `.weld(w)`: pick the backend with `SceneSpec::weld_backend(WeldBackend::..)`" },
    Manual { pattern: ". reference_weld", note: "`.reference_weld(w)` became `.weld(w)`: pick the backend with `SceneSpec::weld_backend(WeldBackend::..)`" },
    Method { old: "gpu_weld", new: "weld", gate: G },
    Method { old: "reference_weld", new: "weld", gate: G },
    MacroHead { old: "weld_morph", new: "weld", heads: &["Weld::default().morph($1)", "$1.morph($2)"] },
    Call { chain: &[("join", &[])], to: ".segmented()", gate: MuiChain, needs: &[] },
    // Icons.
    Manual { pattern: "material_symbols :: codepoint", note: "use the `mui_symbols::sym::NAME` char consts instead of `codepoint(\"name\")`" },
    // Resolving.
    Function { old: "resolve_scene", new: "resolve" },
    Type { old: "TextCache", new: "Resolver" },
    FnCall { name: "resolve_scene_with", args: &[A, After("&mut")], to: "$2.resolve($1)", needs: &[] },
    DropImport { name: "resolve_scene_with" },
    DropImport { name: "resolve_scene_cached" },
    DropImport { name: "resolve_scene_animated" },
    DropImport { name: "resolve_scene_retained" },
    Manual { pattern: "resolve_scene_cached", note: "use `Resolver`: `let mut r = Resolver::new(); r.resolve(&spec)`, `r.welds` is the weld cache" },
    Manual { pattern: "resolve_scene_animated", note: "use `Resolver`: `r.resolve_animated(&spec, glide, None)`" },
    Manual { pattern: "resolve_scene_retained", note: "use `Resolver`: `r.resolve_animated(&spec, glide, prev)`" },
    Manual { pattern: ". resolve_animated (", note: "the glide callback's key is `&Id` (was `&str`): an explicit `|key: &str, ..|` becomes `|key: &Id, ..|`, `key.as_str()` where a `&str` is needed" },
    // mui-truce: `Bridge::bind` derives the widget id from the parameter.
    // ponytail: `args` splits the old `|ui, v|` closure at its comma ($4, $5), which is
    // what lets the rule insert `id`; if `args` learns closures, this becomes 4 args.
    Call { chain: &[("bind", &[A, Has("\""), A, A, A])], to: ".bind($1, $3, $4, id, $5)", gate: Mui, needs: &[] },
    Manual { pattern: ". bind (", note: "`Bridge::bind`'s closure is `|ui, id, v|`: pass `id` to the widget; a toggle can use `bind_bool(ui, P, |ui, id, on| ..)`" },
    // Geometry: kurbo is the one Point/Vec2/Rect/Affine (mui-geometry, mui-weld re-export it).
    // `Rect::from_points` is kurbo's two-corner constructor, so the ring form is flagged too.
    Assoc { ty: "Bounds", name: "from_points", args: &[A], to: "$pathbounds($1)", bare: "mui_geometry::" },
    Type { old: "Bounds", new: "Rect" },
    Corner { field: "min", axis: "x", to: "x0" },
    Corner { field: "min", axis: "y", to: "y0" },
    Corner { field: "max", axis: "x", to: "x1" },
    Corner { field: "max", axis: "y", to: "y1" },
    Manual { pattern: ". min . x", note: "`Bounds { min, max }` became kurbo `Rect { x0, y0, x1, y1 }`: `.min.x` -> `.x0`, `.min.y` -> `.y0`, `.min` -> `.origin()`" },
    Manual { pattern: ". max . x", note: "`Bounds { min, max }` became kurbo `Rect { x0, y0, x1, y1 }`: `.max.x` -> `.x1`, `.max.y` -> `.y1`" },
    Manual { pattern: ". translated (", note: "`Bounds::translated(d)` is `rect + d`; `RoundedRect::translated` takes a `Vec2`" },
    Method { old: "finite", new: "is_finite", gate: Mui },
    Method { old: "perpendicular", new: "turn_90", gate: Mui },
    Manual { pattern: ". rotated (", note: "`Point::rotated(a)` is gone: `Vec2::from_angle(a) * r`, or `Affine::rotate(a) * p`" },
    Assoc { ty: "Affine", name: "translation", args: &[A, A], to: "$pathAffine::translate(($1, $2))", bare: "" },
    Assoc { ty: "Affine", name: "scale", args: &[A, A], to: "$pathAffine::scale_non_uniform($1, $2)", bare: "" },
    Assoc { ty: "Affine", name: "rotation", args: &[A], to: "$pathAffine::rotate($1)", bare: "" },
    Assoc { ty: "Affine", name: "rotation_about", args: &[A, A], to: "$pathAffine::rotate_about($1, $2)", bare: "" },
    // `.then` and `.apply` are too common a name to rewrite without the receiver's type.
    Manual { pattern: "Affine :: translation", note: "kurbo `Affine`: `a.then(b)` -> `b * a`, `t.apply(p)` -> `t * p`" },
    Manual { pattern: "Affine :: rotation", note: "kurbo `Affine`: `a.then(b)` -> `b * a`, `t.apply(p)` -> `t * p`" },
    Manual { pattern: ". rigid_transform (", note: "`Path::rigid_transform` and `Path::translate` take a `Vec2` (`p.to_vec2()`)" },
    Manual { pattern: "drag_delta : Point", note: "`Response::drag_delta` and `drag_total` are `Vec2` (a kurbo `Point` has no `+ Point`)" },
    Manual { pattern: "drag_total : Point", note: "`Response::drag_total` is a `Vec2`" },
    Manual { pattern: "Edge :: Arc {", note: "build a weld arc with `Edge::arc(center, radius, start, sweep)`; the variant also carries unit `from`/`to` vectors, so match it with `..`" },
    // Spacing tokens moved crates.
    Moved { names: &["Spacing", "SpacingScale", "SpacingToken"], from: "mui_geometry", to: "mui_layout" },
    // Element's switches are one `flags` field; a surface's are unchanged, so only
    // reads through `payload()` / `payload_mut()` are flagged.
    Manual { pattern: "payload ( ) . focusable", note: "`Element::focusable` is a flag: `e.has(Element::FOCUSABLE)`" },
    Manual { pattern: "payload_mut ( ) . focusable", note: "`Element::focusable` is a flag: `e.set(Element::FOCUSABLE, on)`" },
    Manual { pattern: "payload ( ) . captures_wheel", note: "`Element::captures_wheel` is a flag: `e.has(Element::CAPTURES_WHEEL)`" },
    Manual { pattern: "payload_mut ( ) . captures_wheel", note: "`Element::captures_wheel` is a flag: `e.set(Element::CAPTURES_WHEEL, on)`" },
    Manual { pattern: "payload ( ) . tracks_pointer", note: "`Element::tracks_pointer` is a flag: `e.has(Element::TRACKS_POINTER)`" },
    Manual { pattern: "payload_mut ( ) . tracks_pointer", note: "`Element::tracks_pointer` is a flag: `e.set(Element::TRACKS_POINTER, on)`" },
    Manual { pattern: "payload ( ) . disabled", note: "`Element::disabled` is a flag: `e.has(Element::DISABLED)`" },
    Manual { pattern: "payload_mut ( ) . disabled", note: "`Element::disabled` is a flag: `e.set(Element::DISABLED, on)`" },
    Manual { pattern: "payload ( ) . baseline", note: "`Element::baseline` is a flag: `e.has(Element::BASELINE)`" },
    Manual { pattern: "payload_mut ( ) . baseline", note: "`Element::baseline` is a flag: `e.set(Element::BASELINE, on)`" },
    Manual { pattern: "payload ( ) . segmented", note: "`Element::segmented` is a flag: `e.has(Element::SEGMENTED)`" },
    Manual { pattern: "payload_mut ( ) . segmented", note: "`Element::segmented` is a flag: `e.set(Element::SEGMENTED, on)`" },
    Manual { pattern: "payload ( ) . weld_excluded", note: "`Element::weld_excluded` is a flag: `e.has(Element::WELD_EXCLUDED)`" },
    Manual { pattern: "payload_mut ( ) . weld_excluded", note: "`Element::weld_excluded` is a flag: `e.set(Element::WELD_EXCLUDED, on)`" },
    Manual { pattern: "payload ( ) . scroll_bar_off", note: "`Element::scroll_bar_off` is a flag: `e.has(Element::SCROLL_BAR_OFF)`" },
    Manual { pattern: "payload_mut ( ) . scroll_bar_off", note: "`Element::scroll_bar_off` is a flag: `e.set(Element::SCROLL_BAR_OFF, on)`" },
    Manual { pattern: ". scroll_bar_heat", note: "scrollbar heat is runtime state: `SceneSpec::scroll_bars` holds it by node key" },
    // Capture moved to mui-material (reported, not rewritten, as above). The
    // `Material` / `Capture` methods need no rule: rustc names the trait to import.
    Manual { pattern: "mui_scene :: resize_capture", note: "`resize_capture` lives in `mui_material` (`mui::material`)" },
    Manual { pattern: "mui_scene :: CaptureError", note: "`CaptureError` lives in `mui_material` (`mui::material`)" },
    Manual { pattern: "mui_scene :: CaptureLayer", note: "`CaptureLayer` lives in `mui_material` (`mui::material`)" },
    Manual { pattern: "scene :: resize_capture", note: "`mui::scene::resize_capture` is `mui::material::resize_capture`" },
    Manual { pattern: "scene :: CaptureError", note: "`mui::scene::CaptureError` is `mui::material::CaptureError`" },
    Manual { pattern: "scene :: CaptureLayer", note: "`mui::scene::CaptureLayer` is `mui::material::CaptureLayer`" },
    // `Ui`'s settings are accessors; the builders (`.font(f)`, `.gpu_welding()`) stay.
    Field { recv: &["ui"], name: "theme", read: ".theme()", write: ".set_theme($1)" },
    Field { recv: &["ui"], name: "scale", read: ".scale()", write: ".set_scale($1)" },
    Field { recv: &["ui"], name: "font", read: "", write: ".set_font($1)" },
    Field { recv: &["ui"], name: "double_click", read: "", write: ".set_double_click($1)" },
    Manual { pattern: "ui . fallback_fonts", note: "`Ui::fallback_fonts` is private: add faces with the `.fallback_font(f)` builder" },
    Manual { pattern: "ui . weld_backend", note: "`Ui::weld_backend` is private: build with `.gpu_welding()`" },
    // The wheel is a delta, not a place.
    Retype { field: "wheel", from: "Point", to: "Vec2" },
];

/// The widget phase of the spec (`Response`, option structs, argument
/// order). Applied, with [`TUPLES`], unless `--no-widgets`.
pub const WIDGET_RULES: &[Rule] = &[
    // Every control takes a label after its id (its accessible name).
    FnCall { name: "toggle", args: &[A, A, A], to: "$pathtoggle($1, $2, \"\", $3)", needs: &[] },
    FnCall { name: "drag_value", args: &[A, A, A, A], to: "$pathdrag_value($1, $2, \"\", $3, $4)", needs: &[] },
    // Positional options became option structs.
    FnCall { name: "color_picker", args: &[A, A, A, Is("true")], to: "$pathcolor_picker($1, $2, $3, ColorOpts { alpha: true })", needs: &["ColorOpts"] },
    FnCall { name: "color_picker", args: &[A, A, A, Is("false")], to: "$pathcolor_picker($1, $2, $3, ColorOpts::default())", needs: &["ColorOpts"] },
    FnCall { name: "color_picker", args: &[A, A, A, Not("ColorOpts")], to: "$pathcolor_picker($1, $2, $3, ColorOpts { alpha: $4 })", needs: &["ColorOpts"] },
    // `bins` edits in place, as `curve` does.
    FnCall { name: "bins", args: &[A, A, Shared], to: "$pathbins($1, $2, &mut $3)", needs: &[] },
];

/// Calls whose tuple result became a named struct. `f(..).0` -> `.el`,
/// `let (a, b) = f(..)` -> `let Response { el: a, changed: b } = f(..)`.
/// `method: true` matches `.name(..)` (in mui files) instead of a free fn.
pub struct Tuple {
    pub name: &'static str,
    pub method: bool,
    pub ty: &'static str,
    pub fields: &'static [&'static str],
}

const fn widget(name: &'static str) -> Tuple {
    Tuple { name, method: false, ty: "Response", fields: &["el", "changed"] }
}

pub const TUPLES: &[Tuple] = &[
    widget("knob"),
    widget("slider"),
    widget("toggle"),
    widget("button"),
    widget("text_input"),
    widget("text_edit"),
    widget("color_picker"),
    widget("bins"),
    widget("curve"),
    widget("drag_value"),
    Tuple { name: "state", method: true, ty: "Interaction", fields: &["hover", "press"] },
];

/// Crate package names that make an import a "mui" import. A dependency
/// renamed with `package = "<one of these>"` becomes a root too.
pub const MUI_CRATES: &[&str] = &[
    "mui", "mui-scene", "mui-layout", "mui-style", "mui-text", "mui-vello", "mui-truce", "mui-geometry",
    "mui-input", "mui-motion", "mui-access", "mui-weld", "mui-material", "mui-symbols",
];

/// Extra path roots always treated as mui (KURV imports `mui` as `mui2`).
pub const EXTRA_ROOTS: &[&str] = &["mui2"];

/// Path dependencies that moved: a `path = ".../<old>"` becomes `.../<new>`.
pub const CARGO_MOVES: &[(&str, &str)] = &[
    ("crates/mui-stage", "media/mui-stage"),
    ("crates/mui-reel", "media/mui-reel"),
    ("crates/mui-motion-bridge", "media/mui-motion-bridge"),
];
